//! [109A-3] Memorias del proyecto ACTIVO para el panel "Memorias" del modal
//! de configuración.
//!
//! El escritorio no añade reglas de memoria propias: reutiliza el puerto del
//! núcleo (`AgentPersistence`) y el formato de archivos del CLI
//! (`glory_harness::memoria_io`), que es el mismo que usa el subcomando
//! `memoria exportar|importar`. Lo único que decide aquí es el ÁMBITO.
//!
//! El ámbito lo resuelve SIEMPRE el backend a partir del área activa de la
//! sesión ([`crate::area_activa`]): el front no envía ni puede enviar un
//! proyecto, así que el panel no puede listar por error los recuerdos de otro
//! proyecto. Sin área registrada el ámbito real del turno es el global (el
//! núcleo degrada igual con `ambito_de_ruta`) y la vista lo marca como tal.

use std::path::PathBuf;
use std::sync::Arc;

use glory_harness::memoria_io::{self, CARPETA_PROYECTO};
use glory_harness::Workspace;
use glory_harness_core::memoria::{ejecutar_curador, PoliticaCurador};
use glory_harness_core::ports::{AgentPersistence, AmbitoMemoria, MemoriaEntrada};
use tauri::State;

use crate::{area_activa, sesion_actual, Estado, Sesion};

/// Un recuerdo tal como lo pinta el panel (DTO plano: el front no conoce la
/// entidad de persistencia ni sus detalles internos).
#[derive(serde::Serialize)]
pub(crate) struct RecuerdoVista {
    clave: String,
    contenido: String,
    origen: String,
    usos: u32,
    ultimo_uso: Option<String>,
    actualizada_en: String,
    /// Archivado por el curador: se conserva para auditar, no se inyecta.
    archivada: bool,
}

impl RecuerdoVista {
    fn de(entrada: &MemoriaEntrada) -> Self {
        Self {
            clave: entrada.clave.clone(),
            contenido: entrada.contenido.clone(),
            origen: entrada.origen.clone(),
            usos: entrada.usos,
            ultimo_uso: entrada.ultimo_uso.map(|d| d.to_rfc3339()),
            actualizada_en: entrada.actualizada_en.to_rfc3339(),
            archivada: entrada.archivada(),
        }
    }
}

/// Estado del ámbito activo: de qué proyecto son los recuerdos que se ven y
/// dónde vive su carpeta de export/import.
#[derive(serde::Serialize)]
pub(crate) struct ListadoMemoria {
    /// `true` = el ámbito activo es el global (la carpeta activa no es un área
    /// registrada). El panel lo distingue para no atribuir al proyecto
    /// recuerdos que en realidad comparte con todo el usuario.
    global: bool,
    /// Tipo de ámbito activo tal como lo nombra el núcleo (`global` o
    /// `proyecto`); el nombre legible del área va en `proyecto`.
    ambito: String,
    proyecto: Option<String>,
    ruta: Option<String>,
    /// Carpeta `.glory/memorias` del área activa; `None` si no hay área (el
    /// export/import de proyecto exige carpeta real, no se inventa una).
    carpeta: Option<String>,
    recuerdos: Vec<RecuerdoVista>,
}

/// Resultado de exportar o importar la carpeta del proyecto.
#[derive(serde::Serialize)]
pub(crate) struct ResultadoCarpetaMemoria {
    carpeta: String,
    recuerdos: usize,
    /// Motivos por los que un archivo no se importó (vacío al exportar).
    omitidos: Vec<String>,
}

/// Ámbito del panel: el proyecto activo, o global si la carpeta activa no es
/// un área registrada. Devuelve además el área para derivar su carpeta.
fn ambito_activo(sesion: &Sesion) -> Result<(AmbitoMemoria, Option<Workspace>), String> {
    match area_activa(sesion)? {
        Some(area) => Ok((AmbitoMemoria::Proyecto(area.id), Some(area))),
        None => Ok((AmbitoMemoria::Global, None)),
    }
}

/// Persistencia del núcleo detrás del puerto compartido (una sola ruta para
/// lecturas, borrados y curador).
fn puerto(sesion: &Arc<Sesion>) -> Arc<dyn AgentPersistence> {
    sesion.persistencia.clone()
}

fn a_listado(
    ambito: AmbitoMemoria,
    area: Option<Workspace>,
    mut entradas: Vec<MemoriaEntrada>,
) -> ListadoMemoria {
    // Orden estable por clave: el panel no depende del orden de la BD.
    entradas.sort_by(|a, b| a.clave.cmp(&b.clave));
    let carpeta = area
        .as_ref()
        .map(|a| PathBuf::from(&a.ruta).join(CARPETA_PROYECTO))
        .map(|p| p.display().to_string());
    ListadoMemoria {
        global: ambito.proyecto_id().is_none(),
        ambito: ambito.etiqueta().to_string(),
        proyecto: area.as_ref().map(|a| a.nombre.clone()),
        ruta: area.as_ref().map(|a| a.ruta.clone()),
        carpeta,
        recuerdos: entradas.iter().map(RecuerdoVista::de).collect(),
    }
}

/// Lista los recuerdos del proyecto activo (nunca de otro proyecto).
#[tauri::command]
pub(crate) async fn memoria_listar_proyecto(
    estado: State<'_, Estado>,
) -> Result<ListadoMemoria, String> {
    let sesion = sesion_actual(&estado)?;
    let (ambito, area) = ambito_activo(&sesion)?;
    let entradas = puerto(&sesion)
        .memoria_listar(sesion.user_id, ambito)
        .await
        .map_err(|e| e.to_string())?;
    Ok(a_listado(ambito, area, entradas))
}

/// Borra un recuerdo del ámbito activo y devuelve el listado ya actualizado
/// (una sola ida y vuelta para el panel). Borrar una clave inexistente es
/// idempotente: no es un error, el listado devuelto lo refleja.
#[tauri::command]
pub(crate) async fn memoria_borrar(
    estado: State<'_, Estado>,
    clave: String,
) -> Result<ListadoMemoria, String> {
    let clave = clave.trim().to_string();
    if clave.is_empty() {
        return Err("la clave del recuerdo es obligatoria".into());
    }
    let sesion = sesion_actual(&estado)?;
    let (ambito, area) = ambito_activo(&sesion)?;
    let persistencia = puerto(&sesion);
    persistencia
        .memoria_borrar(sesion.user_id, ambito, &clave)
        .await
        .map_err(|e| e.to_string())?;
    let entradas = persistencia
        .memoria_listar(sesion.user_id, ambito)
        .await
        .map_err(|e| e.to_string())?;
    Ok(a_listado(ambito, area, entradas))
}

/// Pasa el curador determinista sobre el ámbito ACTIVO (mismo código que el
/// cron y que `memoria curar`, sin gastar tokens) y devuelve su reporte.
/// Se cura solo el ámbito del panel: curar los demás proyectos desde aquí
/// mutaría datos que el panel no está mostrando.
#[tauri::command]
pub(crate) async fn memoria_curar(estado: State<'_, Estado>) -> Result<String, String> {
    let sesion = sesion_actual(&estado)?;
    let (ambito, _) = ambito_activo(&sesion)?;
    let resumen = ejecutar_curador(
        &puerto(&sesion),
        sesion.user_id,
        ambito,
        &PoliticaCurador::default(),
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(resumen.texto())
}

/// Exporta a `.glory/memorias` del área activa (destino versionable del
/// repo). El destino NO lo elige el front: así el comando no puede escribir
/// en una ruta arbitraria del disco.
#[tauri::command]
pub(crate) async fn memoria_exportar(
    estado: State<'_, Estado>,
) -> Result<ResultadoCarpetaMemoria, String> {
    let sesion = sesion_actual(&estado)?;
    let (ambito, area) = ambito_activo(&sesion)?;
    let area = area.ok_or_else(|| {
        "el export al proyecto necesita un área de trabajo activa (abre o crea un proyecto)".to_string()
    })?;
    let entradas = puerto(&sesion)
        .memoria_listar(sesion.user_id, ambito)
        .await
        .map_err(|e| e.to_string())?;
    let carpeta = PathBuf::from(&area.ruta).join(CARPETA_PROYECTO);
    let recuerdos = memoria_io::exportar_carpeta(&carpeta, &entradas)?;
    Ok(ResultadoCarpetaMemoria {
        carpeta: carpeta.display().to_string(),
        recuerdos,
        omitidos: Vec::new(),
    })
}

/// Importa los `.md` de `.glory/memorias` del área activa: upsert por clave,
/// con el sanitizado de memoria aplicado a cada archivo (entrada externa).
/// Un archivo rechazado se informa en `omitidos` sin abortar el resto.
#[tauri::command]
pub(crate) async fn memoria_importar(
    estado: State<'_, Estado>,
) -> Result<ResultadoCarpetaMemoria, String> {
    let sesion = sesion_actual(&estado)?;
    let (ambito, area) = ambito_activo(&sesion)?;
    let area = area.ok_or_else(|| {
        "el import del proyecto necesita un área de trabajo activa (abre o crea un proyecto)"
            .to_string()
    })?;
    let carpeta = PathBuf::from(&area.ruta).join(CARPETA_PROYECTO);
    if !carpeta.is_dir() {
        return Err(format!(
            "no hay carpeta de memorias que importar: {}",
            carpeta.display()
        ));
    }
    let resumen = memoria_io::importar_carpeta(&carpeta, &puerto(&sesion), sesion.user_id, ambito)
        .await?;
    Ok(ResultadoCarpetaMemoria {
        carpeta: carpeta.display().to_string(),
        recuerdos: resumen.importados,
        omitidos: resumen.omitidos,
    })
}

#[cfg(test)]
mod pruebas {
    //! [109A-3] El DTO no debe perder ni inventar estado: la marca de
    //! archivada y el orden estable los fija el backend, no el front.
    use super::*;
    use uuid::Uuid;

    fn entrada(clave: &str, origen: &str) -> MemoriaEntrada {
        let mut e = MemoriaEntrada::nueva(clave.to_string(), "cuerpo".to_string(), origen.to_string());
        e.usos = 4;
        e
    }

    #[test]
    fn listado_ordena_por_clave_y_marca_el_global() {
        let listado = a_listado(
            AmbitoMemoria::Global,
            None,
            vec![entrada("zeta", "turno"), entrada("alfa", "turno")],
        );
        assert!(listado.global, "sin área el ámbito es global");
        assert_eq!(listado.ambito, "global");
        assert_eq!(listado.carpeta, None, "el global no tiene carpeta de proyecto");
        let claves: Vec<&str> = listado.recuerdos.iter().map(|r| r.clave.as_str()).collect();
        assert_eq!(claves, vec!["alfa", "zeta"], "orden estable por clave");
    }

    #[test]
    fn recuerdo_vista_no_pierde_metadatos() {
        let vista = RecuerdoVista::de(&entrada("color", "archivada:2026-01-01"));
        assert_eq!(vista.usos, 4);
        assert!(vista.archivada, "`archivada:<fecha>` marca el recuerdo");
        let viva = RecuerdoVista::de(&entrada("color", "turno"));
        assert!(!viva.archivada);
        assert!(viva.ultimo_uso.is_none(), "sin uso no se inventa fecha");
    }

    #[test]
    fn etiqueta_de_proyecto_usa_el_uuid() {
        let id = Uuid::new_v4();
        let listado = a_listado(AmbitoMemoria::Proyecto(id), None, Vec::new());
        assert!(!listado.global);
        assert_eq!(listado.ambito, "proyecto");
        assert_eq!(listado.ambito, AmbitoMemoria::Proyecto(id).etiqueta());
    }
}
