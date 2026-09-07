//! [079A-1 F3] Handlers web de conversaciones (partido de web_datos.rs).

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
use crate::servicio::SesionComun;

#[derive(Debug, Deserialize)]
pub(crate) struct CrearConversacion {
    pub(crate) titulo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParcheConversacion {
    pub(crate) titulo: Option<String>,
    pub(crate) archivada: Option<bool>,
}

/// Conversación con ownership (existe en la lista del usuario).
async fn conv_propia(comun: &SesionComun, cid: &str) -> Result<crate::InfoConversacion, ApiError> {
    let id = Uuid::parse_str(cid.trim())
        .map_err(|_| error("peticion_invalida", "id de conversación malformado"))?;
    comun
        .persistencia
        .conversaciones_listar(comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| error("no_encontrado", "conversación no encontrada"))
}

fn titulo_validado(titulo: Option<String>) -> Result<String, ApiError> {
    let t = titulo
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Nueva conversación".into());
    if t.chars().count() > MAX_TITULO_CHARS {
        return Err(error("peticion_invalida", "título demasiado largo"));
    }
    Ok(t)
}

// ── Conversaciones ───────────────────────────────────────────────────────

/// [069A-Proyectos] Filtra la lista visible por el área activa de la sesión
/// (la carpeta con fila en `workspaces`): `Some(ws)` → solo sus
/// conversaciones; `None` (carpeta sin proyecto) → solo las sin área. La
/// respuesta incluye el proyecto activo para que el front pinte el header.
/// `GET /api/v1/conversations` — recientes primero, incluye archivo.
pub(crate) async fn listar_conversaciones(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let area = area_activa(&comun)?;
    let lista = comun
        .persistencia
        .conversaciones_listar_con_proyecto(comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "conversaciones": lista,
        "proyecto": area,
    })))
}

/// [069A-Proyectos] Crea la conversación dentro del área activa (si la
/// carpeta actual es un proyecto registrado); si no, sin área. La sesión no
/// guarda `workspace_id` duplicado: el backend lo resuelve por ruta.
/// `POST /api/v1/conversations` — crea y la deja como actual (409 con turno).
pub(crate) async fn crear_conversacion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<CrearConversacion>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, comun) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let titulo = titulo_validado(peticion.titulo)?;
    let area = area_activa(&comun)?;
    let nuevo_id = comun
        .persistencia
        .conversacion_crear_en(comun.user_id, &titulo, area.as_ref().map(|a| a.id))
        .map_err(|e| error("sesion", e.to_string()))?;
    *sesion.conversacion_id.lock().await = Some(nuevo_id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "conversacion": comun
            .persistencia
            .conversaciones_listar(comun.user_id)
            .map_err(|e| error("sesion", e.to_string()))?
            .into_iter()
            .find(|c| c.id == nuevo_id)
            .ok_or_else(|| error("sesion", "conversación recién creada no encontrada"))?,
    })))
}

/// `GET /api/v1/conversations/:cid/messages` — historial + acciones +
/// último uso; la deja como actual (409 con turno).
pub(crate) async fn cargar_conversacion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    use glory_harness_core::AgentPersistence as _;
    let (sesion, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let conv = conv_propia(&comun, &cid).await?;
    let mensajes = comun
        .persistencia
        .listar_mensajes(conv.id)
        .await
        .map_err(|e| error("sesion", e.to_string()))?;
    let acciones = comun
        .persistencia
        .acciones_por_conversacion(conv.id)
        .map_err(|e| error("sesion", e.to_string()))?;
    let ultimo_uso = comun
        .persistencia
        .turno_ultimo_uso_por_conversacion(conv.id)
        .map_err(|e| error("sesion", e.to_string()))?
        .map(|(provider, modelo, tokens_prompt, tokens_complecion)| {
            serde_json::json!({
                "provider": provider,
                "modelo": modelo,
                "tokens_prompt": tokens_prompt,
                "tokens_complecion": tokens_complecion,
            })
        });
    *sesion.conversacion_id.lock().await = Some(conv.id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": conv.id,
        "titulo": conv.titulo,
        "mensajes": mensajes,
        "acciones": acciones,
        "ultimo_uso": ultimo_uso,
    })))
}

/// `PATCH /api/v1/conversations/:cid` — renombrar y/o archivar.
pub(crate) async fn parchear_conversacion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
    Json(peticion): Json<ParcheConversacion>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::PATCH, &state, &id).await?;
    if peticion.titulo.is_none() && peticion.archivada.is_none() {
        return Err(error("peticion_invalida", "nada que actualizar"));
    }
    let conv = conv_propia(&comun, &cid).await?;
    if let Some(titulo) = peticion.titulo {
        let t = titulo_validado(Some(titulo))?;
        comun
            .persistencia
            .conversacion_renombrar(conv.id, comun.user_id, &t)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    if let Some(archivada) = peticion.archivada {
        comun
            .persistencia
            .conversacion_archivar(conv.id, comun.user_id, archivada)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    let actualizada = conv_propia(&comun, &cid.to_string()).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "conversacion": actualizada }),
    ))
}

/// `DELETE /api/v1/conversations/:cid` — elimina; si era la actual de la
/// sesión, ancla la más reciente restante o deja `None` si no queda ninguna
/// (borrador, sin fila fantasma). [069A-7] Create-on-write: NO se auto-crea
/// una vacía al borrar (409 con turno). Sin vault en web (solo desktop).
pub(crate) async fn eliminar_conversacion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, cid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, comun) = sesion_y_comun(&headers, &Method::DELETE, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let conv = conv_propia(&comun, &cid).await?;
    comun
        .persistencia
        .conversacion_eliminar(conv.id, comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?;
    /* [069A-7] Si borramos la conversación que la sesión tenía como actual:
     * anclar la más reciente no-archivada restante (ORDER BY actualizada_en
     * DESC) o dejar `None` (borrador) si ya no queda ninguna. Nunca se crea
     * una fila vacía de reemplazo. */
    let actual = *sesion.conversacion_id.lock().await;
    if actual == Some(conv.id) {
        let restante = comun
            .persistencia
            .conversaciones_listar(comun.user_id)
            .map_err(|e| error("sesion", e.to_string()))?
            .into_iter()
            .find(|c| !c.archivada);
        *sesion.conversacion_id.lock().await = restante.as_ref().map(|c| c.id);
    }
    /* [069A-7] `actual` puede ser `null` = sin conversación (borrador): el
     * contrato de respuesta lo permite; el front limpia el panel a borrador. */
    let actual_id = *sesion.conversacion_id.lock().await;
    let actual_conv: Option<crate::InfoConversacion> = match actual_id {
        Some(aid) => Some(
            comun
                .persistencia
                .conversaciones_listar(comun.user_id)
                .map_err(|e| error("sesion", e.to_string()))?
                .into_iter()
                .find(|c| c.id == aid)
                .ok_or_else(|| error("sesion", "conversación actual no encontrada"))?,
        ),
        None => None,
    };
    Ok(Json(
        serde_json::json!({ "ok": true, "actual": actual_conv }),
    ))
}
