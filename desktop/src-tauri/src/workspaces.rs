//! Comandos de workspaces y seleccion de carpeta del desktop.

use super::*;

use glory_harness::servicio::{aplicar_en_borrador, comando_desde_payload};

/// Diálogo nativo de carpeta → reabre la sesión sobre ese workspace. Si el
/// usuario cancela, devuelve la sesión actual sin cambios (no es un error).
/// [039A-1 04-09 H3] La ruta elegida se persiste en config (`workspace`) para
/// que el próximo arranque la use y el modal muestre la real.
#[tauri::command]
pub(crate) fn elegir_workspace(
    estado: State<'_, Estado>,
    app: AppHandle,
) -> Result<InfoSesion, String> {
    let actual = sesion_actual(&estado)?;
    let carpeta = rfd::FileDialog::new()
        .set_title("Elegir carpeta de trabajo del agente")
        .pick_folder();
    match carpeta {
        Some(dir) => {
            let ruta = dir.to_string_lossy().into_owned();
            /* Persistir la ruta elegida ANTES de reabrir: el nuevo arranque la
             * usará como workspace por defecto. */
            actual
                .persistencia
                .config_guardar("workspace", &ruta)
                .map_err(|e| e.to_string())?;
            abrir_sesion_interna(
                &estado,
                &app,
                OpcionesApertura {
                    dir: Some(ruta),
                    nueva_conversacion: true,
                    ..Default::default()
                },
            )
        }
        None => info_de_panel(&actual, PANEL_PRINCIPAL),
    }
}

/// [069A-Proyectos] Diálogo nativo de carpeta para el modal "Nuevo proyecto".
/// Solo devuelve la ruta (String vacía si el usuario cancela), sin reabrir la
/// sesión ni persistir nada. El modal decide qué hacer con la ruta.
#[tauri::command]
pub(crate) fn elegir_carpeta_proyecto() -> Result<String, String> {
    let carpeta = rfd::FileDialog::new()
        .set_title("Seleccionar carpeta del proyecto")
        .pick_folder();
    match carpeta {
        Some(dir) => Ok(dir.to_string_lossy().into_owned()),
        None => Ok(String::new()),
    }
}

/// [069A-Proyectos] Lista las áreas del usuario + cuál es la activa (la de la
/// carpeta actual; `null` si no es un proyecto registrado).
#[tauri::command]
pub(crate) fn workspaces_listar(estado: State<'_, Estado>) -> Result<ValorWorkspaces, String> {
    let sesion = sesion_actual(&estado)?;
    let workspaces = sesion
        .persistencia
        .workspaces_listar(sesion.user_id)
        .map_err(|e| e.to_string())?;
    let activa = area_activa(&sesion)?;
    Ok(ValorWorkspaces { workspaces, activa })
}

#[derive(serde::Serialize)]
pub(super) struct ValorWorkspaces {
    workspaces: Vec<Workspace>,
    activa: Option<Workspace>,
}

/// [069A-Proyectos] Registra (o reutiliza renombrando) un proyecto para una
/// carpeta y la activa como workspace de la sesión. Si es la PRIMERA área del
/// usuario, adopta las conversaciones sin área (legacy) para que no
/// desaparezcan del sidebar. Devuelve la sesión resultante (mismo contrato
/// que `elegir_workspace`). 409 con turno en curso.
#[tauri::command]
pub(crate) fn workspace_crear_o_activar(
    estado: State<'_, Estado>,
    app: AppHandle,
    nombre: String,
    ruta: String,
) -> Result<InfoSesion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("el nombre es obligatorio".into());
    }
    if nombre.chars().count() > 200 {
        return Err("nombre demasiado largo".into());
    }
    let ruta = std::path::PathBuf::from(ruta.trim());
    if !ruta.is_absolute() {
        return Err("la ruta debe ser absoluta".into());
    }
    if !ruta.is_dir() {
        return Err("la ruta no existe o no es un directorio".into());
    }
    let ruta_s = ruta.to_string_lossy().into_owned();
    let comun = sesion
        .comun
        .lock()
        .map_err(|_| "sesión bloqueada".to_string())?;
    let es_primera = comun
        .persistencia
        .workspaces_listar(comun.user_id)
        .map_err(|e| e.to_string())?
        .is_empty();
    match comun
        .persistencia
        .workspace_por_ruta(comun.user_id, &ruta_s)
        .map_err(|e| e.to_string())?
    {
        Some(existente) => {
            comun
                .persistencia
                .workspace_renombrar(comun.user_id, existente.id, nombre)
                .map_err(|e| e.to_string())?;
        }
        None => {
            let creada = comun
                .persistencia
                .workspace_crear(comun.user_id, nombre, &ruta_s)
                .map_err(|e| e.to_string())?;
            if es_primera {
                comun
                    .persistencia
                    .workspace_adoptar_sin_area(comun.user_id, creada.id)
                    .map_err(|e| e.to_string())?;
            }
        }
    };
    drop(comun);
    sesion
        .persistencia
        .config_guardar("workspace", &ruta_s)
        .map_err(|e| e.to_string())?;
    abrir_sesion_interna(
        &estado,
        &app,
        OpcionesApertura {
            dir: Some(ruta_s),
            nueva_conversacion: true,
            ..Default::default()
        },
    )
}

/// [069A-Proyectos] Activa un área EXISTENTE del usuario por su ruta (para
/// cambiar de proyecto desde el menú del header, sin crear ni renombrar).
#[tauri::command]
pub(crate) fn workspace_activar_por_ruta(
    estado: State<'_, Estado>,
    app: AppHandle,
    ruta: String,
) -> Result<InfoSesion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let ruta_s = ruta.trim().to_string();
    if ruta_s.is_empty() {
        return Err("la ruta es obligatoria".into());
    }
    let existe = sesion
        .persistencia
        .workspace_por_ruta(sesion.user_id, &ruta_s)
        .map_err(|e| e.to_string())?;
    if existe.is_none() {
        return Err("no existe un proyecto para esa carpeta".into());
    }
    sesion
        .persistencia
        .config_guardar("workspace", &ruta_s)
        .map_err(|e| e.to_string())?;
    abrir_sesion_interna(
        &estado,
        &app,
        OpcionesApertura {
            dir: Some(ruta_s),
            nueva_conversacion: true,
            ..Default::default()
        },
    )
}

/// [069A-Proyectos] Renombra un área del usuario. `false` = no existe.
#[tauri::command]
pub(crate) fn workspace_renombrar(
    estado: State<'_, Estado>,
    id: String,
    nombre: String,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let nombre = nombre.trim();
    if nombre.is_empty() {
        return Err("el nombre es obligatorio".into());
    }
    if nombre.chars().count() > 200 {
        return Err("nombre demasiado largo".into());
    }
    sesion
        .persistencia
        .workspace_renombrar(sesion.user_id, id, nombre)
        .map_err(|e| e.to_string())
}

/// [069A-Proyectos] Elimina un área del usuario. Sus conversaciones quedan
/// sin área (`workspace_id = NULL`) y reaparecen en la carpeta sin proyecto.
/// `false` = no existía.
#[tauri::command]
pub(crate) fn workspace_eliminar(estado: State<'_, Estado>, id: String) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .workspace_eliminar(sesion.user_id, id)
        .map_err(|e| e.to_string())
}

/// Fija o cambia la meta ([109A-5 F1]).
///
/// `conversacion_id` es opcional: sin él la meta vive en memoria como
/// BORRADOR, que es el caso del panel global mientras el panel no identifique
/// su conversación (F3 lo cablea); con él, la transición es durable y por
/// conversación, y cualquier comando inválido se rechaza sin mutar nada.
/// `accion` (`fijar|limpiar|pausar|reanudar|lograr`) es opcional: sin ella se
/// conserva el contrato anterior del panel (texto = fijar, vacío = limpiar).
#[tauri::command]
pub(crate) fn actualizar_meta(
    estado: State<'_, Estado>,
    meta: Option<String>,
    accion: Option<String>,
    turno_id: Option<String>,
    conversacion_id: Option<String>,
) -> Result<Option<String>, String> {
    let sesion = sesion_actual(&estado)?;
    let comando = comando_desde_payload(meta, accion.as_deref(), turno_id.as_deref())
        .map_err(|e| e.to_string())?;
    match conversacion_id {
        Some(id) => {
            let conv = Uuid::parse_str(id.trim()).map_err(|_| "conversación inválida".to_string())?;
            let mut comun = sesion
                .comun
                .lock()
                .map_err(|_| "sesión bloqueada".to_string())?;
            let resultado = comun.meta_aplicar(conv, comando).map_err(|e| e.to_string())?;
            Ok(resultado.estado.texto_activo().map(str::to_owned))
        }
        None => {
            let mut borrador = sesion
                .meta
                .lock()
                .map_err(|_| "sesión bloqueada".to_string())?;
            *borrador = aplicar_en_borrador(comando).map_err(|e| e.to_string())?;
            Ok(borrador.clone())
        }
    }
}
