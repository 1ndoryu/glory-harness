//! [Partición limite-lineas] Consolas del desktop (tab Consola):
//! matar/listar/salida/escribir/crear. Extraído de `conversaciones.rs`
//! (god-object de 639 líneas efectivas).

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

/// [209A-1 F4-resto] Mata UNA consola viva por su `id_ejecucion` (la × de
/// la tab Consola sobre una entrada viva). Devuelve `true` si estaba viva
/// y se mató; `false` si no existe o ya terminó (idempotente, sin error:
/// la vista ya la marca como terminada al llegar `consola_fin`).
#[tauri::command]
pub(crate) async fn consola_matar(
    estado: State<'_, Estado>,
    id_ejecucion: String,
) -> Result<bool, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let viva = ejecutor
        .lista()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .any(|c| c.id_ejecucion == id && c.viva);
    if !viva {
        return Ok(false);
    }
    ejecutor.matar(id).await.map_err(|e| e.to_string())?;
    Ok(true)
}

/// [219A-3] Vista de consola para la sub-barra de la tab Consola (vivas +
/// recientes). Se serializa con los mismos nombres que el endpoint web
/// (`id_ejecucion`, `comando`, `viva`, `codigo_salida`, `origen`).
#[derive(serde::Serialize)]
pub(crate) struct InfoConsolaTauri {
    id_ejecucion: String,
    comando: String,
    viva: bool,
    codigo_salida: Option<i32>,
    /// [219A-4] Dueño (`agente`/`usuario` por el contrato de `OrigenConsola`).
    origen: glory_harness_core::ports::OrigenConsola,
}

/// [219A-3] Transcript retenido para el backfill de la tab (mismos nombres
/// que el endpoint web; `flujo` serializa `stdout`/`stderr` por el contrato
/// de `FlujoConsola`).
#[derive(serde::Serialize)]
pub(crate) struct LineaConsolaTauri {
    flujo: glory_harness_core::evento::FlujoConsola,
    linea: String,
}

#[derive(serde::Serialize)]
pub(crate) struct TranscriptConsolaTauri {
    id_ejecucion: String,
    comando: String,
    viva: bool,
    codigo_salida: Option<i32>,
    /// [219A-4] Dueño (coherencia con la lista).
    origen: glory_harness_core::ports::OrigenConsola,
    lineas: Vec<LineaConsolaTauri>,
}

/// [219A-3] `consolas_listar` — vivas primero + recientes archivadas (el
/// runner ordena por inicio). Backfill de la sub-barra al abrir la tab.
#[tauri::command]
pub(crate) async fn consolas_listar(
    estado: State<'_, Estado>,
) -> Result<Vec<InfoConsolaTauri>, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let lista = ejecutor.lista().await.map_err(|e| e.to_string())?;
    Ok(lista
        .into_iter()
        .map(|c| InfoConsolaTauri {
            id_ejecucion: c.id_ejecucion,
            comando: c.comando,
            viva: c.viva,
            codigo_salida: c.codigo_salida,
            origen: c.origen,
        })
        .collect())
}

/// [219A-3] `consola_salida` — transcript retenido por `id_ejecucion`.
/// Error claro si el runner ya no retiene ese id.
#[tauri::command]
pub(crate) async fn consola_salida(
    estado: State<'_, Estado>,
    id_ejecucion: String,
) -> Result<TranscriptConsolaTauri, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let t = ejecutor.salida(id).await.map_err(|e| e.to_string())?;
    Ok(TranscriptConsolaTauri {
        id_ejecucion: t.id_ejecucion,
        comando: t.comando,
        viva: t.viva,
        codigo_salida: t.codigo_salida,
        origen: t.origen,
        lineas: t
            .lineas
            .into_iter()
            .map(|l| LineaConsolaTauri {
                flujo: l.flujo,
                linea: l.linea,
            })
            .collect(),
    })
}

/// [219A-3] `consola_escribir` — bytes crudos al stdin de una viva.
/// Devuelve los bytes aceptados. Vacío o >64 KB se rechaza aquí (mismo tope
/// que el runner); terminada o desconocida → error claro del runner.
#[tauri::command]
pub(crate) async fn consola_escribir(
    estado: State<'_, Estado>,
    id_ejecucion: String,
    texto: String,
) -> Result<usize, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    if texto.is_empty() {
        return Err("texto vacío: nada que escribir".into());
    }
    if texto.len() > 64 * 1024 {
        return Err("texto mayor de 64 KB: trocéalo en varias escrituras".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    ejecutor
        .escribir(id, texto.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

/// [219A-4] `consola_crear` — abre una consola PROPIA del operador ([+
/// Nueva] de la tab). Sin `comando` = shell por defecto del SO. Devuelve
/// el id + la etiqueta + el dueño (siempre `usuario`).
#[derive(serde::Serialize)]
pub(crate) struct NuevaConsolaTauri {
    id_ejecucion: String,
    comando: String,
    origen: glory_harness_core::ports::OrigenConsola,
}

#[tauri::command]
pub(crate) async fn consola_crear(
    estado: State<'_, Estado>,
    comando: Option<String>,
) -> Result<NuevaConsolaTauri, String> {
    use glory_harness_core::ports::{EjecutorComando, OrigenConsola};
    let sesion = sesion_actual(&estado)?;
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let id = ejecutor
        .ejecutar_propia(comando.as_deref())
        .await
        .map_err(|e| e.to_string())?;
    let etiqueta = ejecutor
        .lista()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id_ejecucion == id)
        .map(|c| c.comando)
        .unwrap_or_else(|| comando.unwrap_or_default());
    Ok(NuevaConsolaTauri {
        id_ejecucion: id,
        comando: etiqueta,
        origen: OrigenConsola::Usuario,
    })
}
