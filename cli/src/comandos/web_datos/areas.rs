//! [079A-1 F3] Handlers web de áreas de trabajo (partido de web_datos.rs).

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    Json,
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::{
    area_activa, error, sesion_y_comun, turno_en_curso, ApiError, AppState, MAX_TITULO_CHARS,
};

#[derive(Debug, Deserialize)]
pub(crate) struct CambiarWorkspace {
    pub(crate) ruta: String,
}

/// [069A-Proyectos] Cuerpo de `POST /workspaces`: registrar (o reutilizar
/// renombrando) un área de trabajo con esa carpeta y activarla.
#[derive(Debug, Deserialize)]
pub(crate) struct CrearWorkspace {
    pub(crate) nombre: String,
    pub(crate) ruta: String,
}

/// [069A-Proyectos] Cuerpo de `PATCH /workspaces/:wid`: renombrar.
#[derive(Debug, Deserialize)]
pub(crate) struct ParcheWorkspace {
    pub(crate) nombre: Option<String>,
}

/// [069A-Proyectos] Nombre de área validado: no vacío, ≤200 caracteres.
fn nombre_area_validado(nombre: Option<String>) -> Result<String, ApiError> {
    let n = nombre
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty())
        .ok_or_else(|| error("peticion_invalida", "el nombre es obligatorio"))?;
    if n.chars().count() > MAX_TITULO_CHARS {
        return Err(error("peticion_invalida", "nombre demasiado largo"));
    }
    Ok(n)
}

// ── Workspace ────────────────────────────────────────────────────────────

/// `GET /api/v1/workspace` — workspace efectivo (loopback; sin redacción
/// porque el único cliente es el usuario local).
pub(crate) async fn leer_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "workspace": comun.workspace }),
    ))
}

/// `POST /api/v1/workspace` — ruta absoluta validada; bloqueado con turno.
pub(crate) async fn cambiar_workspace_ep(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<CambiarWorkspace>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let ruta = PathBuf::from(peticion.ruta.trim());
    if !ruta.is_absolute() {
        return Err(error("peticion_invalida", "la ruta debe ser absoluta"));
    }
    if !ruta.is_dir() {
        return Err(error(
            "no_encontrado",
            "la ruta no existe o no es un directorio",
        ));
    }
    let mut comun = sesion.comun.lock().await;
    comun
        .cambiar_workspace(ruta)
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "workspace": comun.workspace }),
    ))
}

// ── [069A-Proyectos] Áreas de trabajo (workspaces) ───────────────────────

/// `GET /api/v1/workspaces` — áreas del usuario (recientes primero) + cuál es
/// la activa (resuelta por la ruta de la sesión; `null` si la carpeta actual
/// no tiene proyecto registrado).
pub(crate) async fn listar_workspaces(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let workspaces = comun
        .persistencia
        .workspaces_listar(comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?;
    let activa = area_activa(&comun)?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "workspaces": workspaces,
        "activa": activa,
    })))
}

/// [069A-Proyectos] Crea/activa un proyecto sobre la carpeta actual:
/// 1) valida la ruta absoluta + directorio; 2) si ya hay un área con esa
///    carpeta, la reutiliza renombrándola (idempotente); si no, crea la fila
///    y, al ser la PRIMERA área del usuario, adopta las conversaciones sin
///    área (legacy) para que no desaparezcan del sidebar; 3) activa la
///    carpeta como workspace de la sesión (config + runtime) y la deja como
///    actual.
///
/// `POST /api/v1/workspaces` — 409 con turno en curso.
pub(crate) async fn crear_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<CrearWorkspace>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let nombre = nombre_area_validado(Some(peticion.nombre))?;
    let ruta = PathBuf::from(peticion.ruta.trim());
    if !ruta.is_absolute() {
        return Err(error("peticion_invalida", "la ruta debe ser absoluta"));
    }
    if !ruta.is_dir() {
        return Err(error(
            "no_encontrado",
            "la ruta no existe o no es un directorio",
        ));
    }

    let mut comun = sesion.comun.lock().await;
    let ruta_s = ruta.to_string_lossy().into_owned();
    let area = match comun
        .persistencia
        .workspace_por_ruta(comun.user_id, &ruta_s)
        .map_err(|e| error("sesion", e.to_string()))?
    {
        // Ya hay un proyecto para esta carpeta → reutilizarlo con el nombre
        // pedido (idempotente; el usuario lo está "creando" de nuevo).
        Some(existente) => {
            comun
                .persistencia
                .workspace_renombrar(comun.user_id, existente.id, &nombre)
                .map_err(|e| error("sesion", e.to_string()))?;
            existente
        }
        None => {
            // Primera área del usuario → adoptar las conversaciones sin área
            // (legacy) para que el sidebar filtrado no las oculte.
            let es_primera = comun
                .persistencia
                .workspaces_listar(comun.user_id)
                .map_err(|e| error("sesion", e.to_string()))?
                .is_empty();
            let creada = comun
                .persistencia
                .workspace_crear(comun.user_id, &nombre, &ruta_s)
                .map_err(|e| error("sesion", e.to_string()))?;
            if es_primera {
                comun
                    .persistencia
                    .workspace_adoptar_sin_area(comun.user_id, creada.id)
                    .map_err(|e| error("sesion", e.to_string()))?;
            }
            creada
        }
    };

    // Activar la carpeta como workspace de la sesión (persiste config +
    // reconstruye runtime sobre ella). Valida de nuevo la ruta (ya hecha).
    comun
        .cambiar_workspace(ruta)
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "activa": area_activa(&comun)?,
        "creada": area,
        "workspace": comun.workspace,
    })))
}

/// `PATCH /api/v1/workspaces/:wid` — renombra (no adopta ni reasigna).
pub(crate) async fn renombrar_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, wid)): Path<(String, String)>,
    Json(peticion): Json<ParcheWorkspace>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::PATCH, &state, &id).await?;
    let nombre = match peticion.nombre {
        Some(n) => nombre_area_validado(Some(n))?,
        None => return Err(error("peticion_invalida", "nombre obligatorio")),
    };
    let wid = Uuid::parse_str(wid.trim())
        .map_err(|_| error("peticion_invalida", "id de área malformado"))?;
    let ok = comun
        .persistencia
        .workspace_renombrar(comun.user_id, wid, &nombre)
        .map_err(|e| error("sesion", e.to_string()))?;
    if !ok {
        return Err(error("no_encontrado", "área de trabajo no encontrada"));
    }
    Ok(Json(serde_json::json!({ "ok": true, "renombrada": true })))
}

/// `DELETE /api/v1/workspaces/:wid` — elimina; sus conversaciones quedan sin
/// área (`workspace_id = NULL`) y reaparecen en la carpeta sin proyecto.
pub(crate) async fn eliminar_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, wid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::DELETE, &state, &id).await?;
    let wid = Uuid::parse_str(wid.trim())
        .map_err(|_| error("peticion_invalida", "id de área malformado"))?;
    let ok = comun
        .persistencia
        .workspace_eliminar(comun.user_id, wid)
        .map_err(|e| error("sesion", e.to_string()))?;
    if !ok {
        return Err(error("no_encontrado", "área de trabajo no encontrada"));
    }
    Ok(Json(serde_json::json!({ "ok": true, "eliminada": true })))
}
