//! [089A-10] Estado Git local del workspace activo por HTTP (modo web).
//!
//! [139A-8 F5n/S8] Pasarela fina: el estado lo calcula `ServicioGit` del
//! core (única orquestación); aquí solo resolución de sesión/raíz HTTP y
//! mapeo del error a `ApiError` (se conservan los códigos que el front
//! distingue). Espejo web del comando Tauri `workspace_git_estado`: solo
//! consulta `git` (sin commit/push/pull/merge/rebase); el cwd se resuelve en
//! el backend desde `comun.workspace` y la salida está acotada.
//!
//! Nota: el JSON incluye `rama` (la comparte el DTO del core con desktop);
//! el front web la ignora: es aditiva, no rompe el contrato.

use std::sync::Arc;

use axum::{
    extract::{Path as Ruta, State},
    http::{HeaderMap, Method},
    Json,
};

use super::{area_activa, error, sesion_y_comun, ApiError, AppState};
use crate::servicio::SesionComun;
use glory_harness_core::{ErrorGit, EstadoGit, ServicioGit};

/// `GET /api/v1/session/{id}/git/estado` — estado Git del workspace activo.
pub(crate) async fn git_estado(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
) -> Result<Json<EstadoGit>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    ServicioGit::nuevo(raiz)
        .estado()
        .await
        .map(Json)
        .map_err(|fallo: ErrorGit| error(&fallo.codigo, fallo.mensaje))
}

fn raiz_activa(comun: &SesionComun) -> Result<std::path::PathBuf, ApiError> {
    let workspace = area_activa(comun)?.ok_or_else(|| {
        error(
            "workspace_no_configurado",
            "elige un workspace antes de consultar Git",
        )
    })?;
    let raiz = std::path::PathBuf::from(workspace.ruta);
    let canon = raiz
        .canonicalize()
        .map_err(|e| error("workspace_invalido", e.to_string()))?;
    if !canon.is_dir() {
        return Err(error(
            "workspace_invalido",
            "el workspace activo no es una carpeta",
        ));
    }
    Ok(canon)
}
