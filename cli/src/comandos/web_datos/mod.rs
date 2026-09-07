//! [079A-1 F3] Hub de datos web: conversaciones, configuración y áreas.
//!
//! Partición de web_datos.rs (995 líneas) por dominio. Re-exporta los
//! handlers para que comandos::web siga usando web_datos::X sin cambios.
//! Helpers compartidos (sesion_y_comun, 	urno_en_curso, rea_activa).

mod areas;
mod configuracion;
mod conversaciones;
#[cfg(test)]
mod pruebas;

pub(crate) use areas::{
    cambiar_workspace_ep, crear_workspace, eliminar_workspace, leer_workspace, listar_workspaces,
    renombrar_workspace,
};
pub(crate) use configuracion::{guardar_config, leer_config, leer_proveedores};
pub(crate) use conversaciones::{
    cargar_conversacion, crear_conversacion, eliminar_conversacion, listar_conversaciones,
    parchear_conversacion,
};

use std::sync::Arc;

use axum::http::{HeaderMap, Method};

use super::web::{autorizar_sesion, error, ApiError, AppState, SesionWeb};
use crate::servicio::SesionComun;

/// Título/nombre: 1..=200 caracteres (compartido por los tres dominios).
pub(crate) const MAX_TITULO_CHARS: usize = 200;

/// [069A-Proyectos] Área de trabajo activa de la sesión. Si la ruta activa
/// (`comun.workspace`) es un directorio real pero no está registrada en la
/// tabla, se auto-registra (workspace implícito, como opencode/claurst/grok
/// usan `cwd` como área). Si no hay ruta activa, devuelve el primer workspace
/// registrado (o `None` si no hay ninguno).
fn area_activa(comun: &SesionComun) -> Result<Option<crate::Workspace>, ApiError> {
    if comun.workspace.is_empty() || comun.workspace == "<desconocido>" {
        // Sin ruta activa: primer workspace registrado, o None.
        return comun
            .persistencia
            .workspaces_listar(comun.user_id)
            .map(|ws| ws.into_iter().next())
            .map_err(|e| error("sesion", e.to_string()));
    }
    // La ruta activa de la sesión puede estar o no registrada en la tabla.
    match comun
        .persistencia
        .workspace_por_ruta(comun.user_id, &comun.workspace)
        .map_err(|e| error("sesion", e.to_string()))?
    {
        Some(ws) => Ok(Some(ws)),
        None => {
            // La ruta activa existe como directorio real pero no está
            // registrada: auto-registrarla.
            let path = std::path::Path::new(&comun.workspace);
            if path.is_dir() {
                let nombre = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Área de trabajo".to_string());
                comun
                    .persistencia
                    .workspace_crear(comun.user_id, &nombre, &comun.workspace)
                    .map(Some)
                    .map_err(|e| error("sesion", e.to_string()))
            } else {
                // La ruta persistida ya no existe (disco extraído, carpeta
                // borrada): primer workspace registrado o None.
                comun
                    .persistencia
                    .workspaces_listar(comun.user_id)
                    .map(|ws| ws.into_iter().next())
                    .map_err(|e| error("sesion", e.to_string()))
            }
        }
    }
}

/// `true` si la sesión tiene un turno en curso.
async fn turno_en_curso(sesion: &Arc<SesionWeb>) -> bool {
    sesion.turno.lock().await.is_some()
}

async fn sesion_y_comun(
    headers: &HeaderMap,
    metodo: &Method,
    state: &AppState,
    id: &str,
) -> Result<(Arc<SesionWeb>, SesionComun), ApiError> {
    let (sesion, _) = autorizar_sesion(headers, metodo, state, id).await?;
    let comun = sesion.comun.lock().await.clone();
    Ok((sesion, comun))
}
