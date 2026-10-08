//! [Partición limite-lineas] CRUD de conversaciones del desktop
//! (crear/listar/cargar/renombrar/archivar/eliminar, single + proyecto).
//! Extraído de `conversaciones.rs` (god-object de 639 líneas efectivas).
//! `CargaConversacion` + `usos_turno` viven aquí y los usa `rewind.rs`.

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

use super::conversaciones::conv_id_de_panel;

/// [039A-3 P5] Abre un panel con una conversación dada (la deja como la que
/// muestra ese panel). No crea duplicados: si el panel ya existe, solo cambia
/// su conversación. El tramo pendiente se limpia (pertenece a la conversación
/// anterior del panel).
/// [069A-7] `Option<Uuid>`: `None` deja el panel en borrador (sin fila).
fn panel_poner_conversacion(
    sesion: &Sesion,
    panel_id: &str,
    conv_id: Option<Uuid>,
) -> Result<(), String> {
    sesion
        .paneles
        .lock()
        .map(|mut p| {
            let d = p.entry(panel_id.to_string()).or_insert(PanelDatos {
                conversacion_id: conv_id,
                turno_id: None,
                tramo_rewind: None,
            });
            d.conversacion_id = conv_id;
            d.tramo_rewind = None;
            Ok(())
        })
        .map_err(|_| "sesión bloqueada por otro turno".to_string())?
}

// --- F4: conversaciones (CRUD) ---

/// Crea una conversación y la deja como actual del panel (falla si hay turno
/// en curso). [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) fn conversacion_nueva(
    estado: State<'_, Estado>,
    titulo: Option<String>,
    panel_id: Option<String>,
) -> Result<InfoConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let titulo = titulo
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Nueva conversación".into());
    /* [069A-Proyectos] La conversación nueva nace dentro del área activa de
     * la carpeta actual (si es un proyecto registrado); si no, sin área. El
     * workspace_id se resuelve por ruta (nunca se duplica en `SesionComun`). */
    let area = area_activa(&sesion)?;
    let id = sesion
        .persistencia
        .conversacion_crear_en(sesion.user_id, &titulo, area.as_ref().map(|a| a.id))
        .map_err(|e| e.to_string())?;
    /* [039A-3 P5] La nueva conversación queda como actual SOLO de este panel.
     * [039A-3 P3] Conversación nueva = contexto nuevo: no hay tramo previo
     * que restaurar desde aquí (se limpia el del panel). */
    panel_poner_conversacion(&sesion, &panel_id, Some(id))?;
    Ok(InfoConversacion {
        id,
        titulo,
        archivada: false,
        creada_en: chrono::Utc::now(),
        actualizada_en: chrono::Utc::now(),
        workspace_id: None,
        workspace_nombre: None,
    })
}

/// [069A-Proyectos] Lista todas las conversaciones agrupables por proyecto.
/// Las conversaciones legacy conservan `workspace_id: null`.
#[tauri::command]
pub(crate) fn listar_conversaciones(
    estado: State<'_, Estado>,
) -> Result<Vec<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .conversaciones_listar_con_proyecto(sesion.user_id)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub(crate) struct CargaConversacion {
    pub(crate) id: Uuid,
    pub(crate) titulo: String,
    pub(crate) mensajes: Vec<MensajePersistido>,
    /// [039A-1 04-09 H6] Acciones (tools) de la conversación en orden de
    /// ejecución, para repintar los bloques `.herramienta` al recargar.
    pub(crate) acciones: Vec<glory_harness::AccionRecuperada>,
    /// [039A-3 P1] Uso/modelo real del último turno (para repintar el pie de
    /// turno al recargar). `None` si no hay turno con uso registrado.
    /// Se conserva por compatibilidad; el front prefiere `usos_turno`.
    pub(crate) ultimo_uso: Option<UsoTurnoPersistido>,
    /// [20-09-2026] Uso/modelo real de TODOS los turnos (para repintar CADA
    /// pie de turno al recargar, no solo el último).
    pub(crate) usos_turno: Vec<UsoTurnoPorTurno>,
    /// [039A-3 P3] Archivos que tocó el último tramo rebobinado ("volver a
    /// punto"), listos para la acción EXPLÍCITA "restaurar archivos de este
    /// tramo". Vacío cuando la carga no viene de un rewind (no hay nada que
    /// restaurar). El front lo ofrece solo cuando no está vacío.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) archivos_tramo: Vec<String>,
}

/// [039A-3 P1] Uso real de un turno persistido (serializable al front).
#[derive(serde::Serialize)]
pub(crate) struct UsoTurnoPersistido {
    pub(crate) provider: String,
    pub(crate) modelo: String,
    pub(crate) tokens_prompt: u32,
    pub(crate) tokens_complecion: u32,
}

/// [20-09-2026] Uso real de un turno con su ancla temporal (para que el front
/// lo ancle a los mensajes de ESE turno en vez de pintar solo el último pie).
#[derive(serde::Serialize)]
pub(crate) struct UsoTurnoPorTurno {
    pub(crate) turno_en: String,
    pub(crate) provider: String,
    pub(crate) modelo: String,
    pub(crate) tokens_prompt: u32,
    pub(crate) tokens_complecion: u32,
}

/// [20-09-2026] Usos de todos los turnos de una conversación (ordenados por
/// `creado_en` desde la query). Auxiliar común de `cargar_conversacion` y del
/// rewind: la carga reconstruida tras "volver a punto" también repinta pies.
pub(crate) fn usos_turno(
    sesion: &Sesion,
    conv_id: Uuid,
) -> Result<Vec<UsoTurnoPorTurno>, String> {
    sesion
        .persistencia
        .usos_turno_por_conversacion(conv_id)
        .map_err(|e| e.to_string())
        .map(|usos| {
            usos.into_iter()
                .map(|u| UsoTurnoPorTurno {
                    turno_en: u.turno_en,
                    provider: u.provider,
                    modelo: u.modelo,
                    tokens_prompt: u.tokens_prompt,
                    tokens_complecion: u.tokens_complecion,
                })
                .collect()
        })
}

/// Carga una conversación como actual del panel con su historial (falla con
/// turno vivo). [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) async fn cargar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<CargaConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.titulo)
        .ok_or_else(|| "conversación no encontrada".to_string())?;
    let mensajes = sesion
        .persistencia
        .listar_mensajes(id)
        .await
        .map_err(|e| e.to_string())?;
    let acciones = sesion
        .persistencia
        .acciones_por_conversacion(id)
        .map_err(|e| e.to_string())?;
    let ultimo_uso = sesion
        .persistencia
        .turno_ultimo_uso_por_conversacion(id)
        .map_err(|e| e.to_string())?
        .map(
            |(provider, modelo, tokens_prompt, tokens_complecion)| UsoTurnoPersistido {
                provider,
                modelo,
                tokens_prompt,
                tokens_complecion,
            },
        );
    /* [039A-3 P5] La conversación cargada queda como actual de este panel.
     * [039A-3 P3] Al cambiar de conversación se limpia el tramo pendiente de
     * restaurar (pertenece a la conversación anterior): su restauración ya no
     * es accesible desde aquí. */
    panel_poner_conversacion(&sesion, &panel_id, Some(id))?;
    Ok(CargaConversacion {
        id,
        titulo,
        mensajes,
        acciones,
        ultimo_uso,
        usos_turno: usos_turno(&sesion, id)?,
        archivos_tramo: Vec::new(),
    })
}

/// Renombra una conversación (`false` = no existe o no es del usuario).
#[tauri::command]
pub(crate) fn renombrar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    titulo: String,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = titulo.trim();
    if titulo.is_empty() {
        return Err("título vacío".into());
    }
    sesion
        .persistencia
        .conversacion_renombrar(id, sesion.user_id, titulo)
        .map_err(|e| e.to_string())
}

/// Archiva/desarchiva (`false` = no existe o no es del usuario).
#[tauri::command]
pub(crate) fn archivar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    archivada: bool,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_archivar(id, sesion.user_id, archivada)
        .map_err(|e| e.to_string())
}

/// Elimina con mensajes y turnos; si era la actual del panel, ancla la más
/// reciente no-archivada restante o deja el panel en borrador (`None`) si no
/// queda ninguna. [069A-7] Create-on-write: NO se crea una vacía de reemplazo.
/// [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) async fn eliminar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<Option<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_eliminar(id, sesion.user_id)
        .map_err(|e| e.to_string())?;
    /* [039A-3 P3] Al eliminar la conversación se limpia su índice del vault y
     * se hace GC de los hashes que quedaron huérfanos. */
    sesion.vault.eliminar_conversacion(id);
    /* [209A-1 F4-resto] Reap: las consolas vivas de la conversación
     * borrada se matan (su pump archiva el transcript acotado). */
    if let Some(ejecutor) = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
    {
        let matadas = ejecutor.matar_por_conversacion(id).await;
        if matadas > 0 {
            eprintln!(
                "[glory-harness-desktop] reap al eliminar conversación {id}: {matadas} matada(s)"
            );
        }
    }
    /* [039A-3 P5] "Era la actual" se decide POR PANEL: si este panel tenía esa
     * conversación cargada, hay que re-anclar el panel. */
    let actual = conv_id_de_panel(&sesion, &panel_id)?;
    if actual == Some(id) {
        /* El tramo pendiente pertenecía a la conversación borrada: se limpia.
         * [069A-7] Se ancla la más reciente no-archivada restante (lista en
         * orden `actualizada_en DESC`) o se deja el panel en borrador. */
        let restante = sesion
            .persistencia
            .conversaciones_listar(sesion.user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| !c.archivada);
        if let Ok(mut g) = sesion.paneles.lock() {
            if let Some(d) = g.get_mut(&panel_id) {
                d.conversacion_id = restante.as_ref().map(|c| c.id);
                d.tramo_rewind = None;
            }
        }
        return Ok(restante);
    }
    info_de_panel(&sesion, &panel_id).map(|i| i.conversacion)
}

/// [119A-2 F4] Archiva/desarchiva TODAS las conversaciones de un proyecto.
/// Devuelve cuántas cambió. Reversible hilo a hilo (igual que el single,
/// sin guard de turno).
#[tauri::command]
pub(crate) fn archivar_conversaciones_proyecto(
    estado: State<'_, Estado>,
    id: String,
    archivada: bool,
) -> Result<usize, String> {
    let sesion = sesion_actual(&estado)?;
    let ws = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversaciones_archivar_por_workspace(sesion.user_id, ws, archivada)
        .map_err(|e| e.to_string())
}

/// [119A-2 F4] Elimina TODAS las conversaciones de un proyecto con sus
/// mensajes y turnos; limpia el vault por hilo y re-ancla el panel si
/// mostraba una de las borradas (la más reciente no-archivada restante o
/// borrador si no queda ninguna, espejo de `eliminar_conversacion`).
#[tauri::command]
pub(crate) async fn eliminar_conversaciones_proyecto(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<Option<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let ws = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let borradas = sesion
        .persistencia
        .conversaciones_eliminar_por_workspace(sesion.user_id, ws)
        .map_err(|e| e.to_string())?;
    for b in &borradas {
        sesion.vault.eliminar_conversacion(*b);
    }
    /* [209A-1 F4-resto] Reap por conversación borrada (espejo del single):
     * las vivas de cada hilo se matan; el resto sigue. */
    if let Some(ejecutor) = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
    {
        let mut matadas = 0;
        for b in &borradas {
            matadas += ejecutor.matar_por_conversacion(*b).await;
        }
        if matadas > 0 {
            eprintln!(
                "[glory-harness-desktop] reap al eliminar proyecto {ws}: {matadas} matada(s)"
            );
        }
    }
    let actual = conv_id_de_panel(&sesion, &panel_id)?;
    if actual.is_some_and(|a| borradas.contains(&a)) {
        let restante = sesion
            .persistencia
            .conversaciones_listar(sesion.user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| !c.archivada);
        if let Ok(mut g) = sesion.paneles.lock() {
            if let Some(d) = g.get_mut(&panel_id) {
                d.conversacion_id = restante.as_ref().map(|c| c.id);
                d.tramo_rewind = None;
            }
        }
        return Ok(restante);
    }
    info_de_panel(&sesion, &panel_id).map(|i| i.conversacion)
}
