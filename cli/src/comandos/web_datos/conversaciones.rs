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
use glory_harness_core::error::Error as ErrorNucleo;
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::EjecutorComando;

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
    use glory_harness_core::PersistenciaTurnos as _;
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
    // [20-09-2026] Usos de TODOS los turnos (cada pie de turno al recargar,
    // no solo el último). Ordenados por `creado_en` desde la query.
    let usos_turno = comun
        .persistencia
        .usos_turno_por_conversacion(conv.id)
        .map_err(|e| error("sesion", e.to_string()))?
        .into_iter()
        .map(|u| {
            serde_json::json!({
                "turno_en": u.turno_en,
                "provider": u.provider,
                "modelo": u.modelo,
                "tokens_prompt": u.tokens_prompt,
                "tokens_complecion": u.tokens_complecion,
            })
        })
        .collect::<Vec<_>>();
    *sesion.conversacion_id.lock().await = Some(conv.id);
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": conv.id,
        "titulo": conv.titulo,
        "mensajes": mensajes,
        "acciones": acciones,
        "ultimo_uso": ultimo_uso,
        "usos_turno": usos_turno,
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
    /* [209A-1 F4-resto] Reap: las consolas vivas de la conversación
     * borrada se matan (cada pump retira su viva y archiva el transcript
     * acotado). Sin vivas devuelve 0, sin error. */
    if let Some(ejecutor) = comun.ejecutor.as_ref() {
        let matadas = ejecutor.matar_por_conversacion(conv.id).await;
        if matadas > 0 {
            tracing::info!(conversacion = %conv.id, matadas, "reap de consolas al eliminar conversación");
        }
    }
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

/// [209A-1 F4-resto] `POST /api/v1/session/:id/consolas/:eid/matar` — mata
/// UNA consola viva por su `id_ejecucion` (la × de la tab Consola sobre una
/// entrada viva). `matada: false` = no existe o ya terminó (idempotente,
/// sin error: la vista ya la marca como terminada al llegar `consola_fin`).
/// No toca turnos: una consola viva no implica turno en curso.
pub(crate) async fn matar_consola(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, eid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let (_sesion, comun) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    let eid = eid.trim();
    if eid.is_empty() {
        return Err(error("peticion_invalida", "id de ejecución vacío"));
    }
    let ejecutor = comun
        .ejecutor
        .as_ref()
        .ok_or_else(|| error("sesion", "sesión sin ejecutor"))?;
    let viva = ejecutor
        .lista()
        .await
        .map_err(|e| error("sesion", e.to_string()))?
        .into_iter()
        .any(|c| c.id_ejecucion == eid && c.viva);
    if !viva {
        return Ok(Json(serde_json::json!({ "ok": true, "matada": false })));
    }
    ejecutor
        .matar(eid)
        .await
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(serde_json::json!({ "ok": true, "matada": true })))
}

/// [219A-3] `GET /api/v1/session/:id/consolas` — vivas primero + recientes
/// archivadas (el runner ordena por inicio; las archivadas van al final).
/// Backfill de la sub-barra al abrir la tab a mitad de turno.
pub(crate) async fn listar_consolas(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_sesion, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let ejecutor = comun
        .ejecutor
        .as_ref()
        .ok_or_else(|| error("sesion", "sesión sin ejecutor"))?;
    let lista = ejecutor
        .lista()
        .await
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "consolas": lista
            .into_iter()
            .map(|c| serde_json::json!({
                "id_ejecucion": c.id_ejecucion,
                "comando": c.comando,
                "viva": c.viva,
                "codigo_salida": c.codigo_salida,
            }))
            .collect::<Vec<_>>(),
    })))
}

/// [219A-3] `GET /api/v1/session/:id/consolas/:eid/salida` — transcript
/// retenido para backfill (viva = anillo con flujo; archivada = resultado
/// guardado como stdout). `no_encontrado` si el runner ya no retiene ese id.
pub(crate) async fn salida_consola(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, eid)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let (_sesion, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let eid = eid.trim();
    if eid.is_empty() {
        return Err(error("peticion_invalida", "id de ejecución vacío"));
    }
    let ejecutor = comun
        .ejecutor
        .as_ref()
        .ok_or_else(|| error("sesion", "sesión sin ejecutor"))?;
    let t = ejecutor.salida(eid).await.map_err(|e| match e {
        ErrorNucleo::NoEncontrado(m) => error("no_encontrado", m),
        otro => error("sesion", otro.to_string()),
    })?;
    Ok(Json(serde_json::json!({
        "ok": true,
        "id_ejecucion": t.id_ejecucion,
        "comando": t.comando,
        "viva": t.viva,
        "codigo_salida": t.codigo_salida,
        "lineas": t
            .lineas
            .into_iter()
            .map(|l| serde_json::json!({
                "flujo": match l.flujo {
                    FlujoConsola::Stdout => "stdout",
                    FlujoConsola::Stderr => "stderr",
                },
                "linea": l.linea,
            }))
            .collect::<Vec<_>>(),
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct EscribirConsola {
    pub(crate) texto: Option<String>,
}

/// [219A-3] `POST /api/v1/session/:id/consolas/:eid/escribir` — bytes crudos
/// al stdin de una viva (`{ok, escritos}`). Vacío o >64 KB →
// `peticion_invalida` (el runner aplica el mismo tope); terminada o
/// desconocida → `no_encontrado`. La UI envía lo tecleado + `\n` al Enter.
pub(crate) async fn escribir_consola(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, eid)): Path<(String, String)>,
    Json(cuerpo): Json<EscribirConsola>,
) -> Result<Json<Value>, ApiError> {
    let (_sesion, comun) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    let eid = eid.trim();
    if eid.is_empty() {
        return Err(error("peticion_invalida", "id de ejecución vacío"));
    }
    let texto = cuerpo.texto.unwrap_or_default();
    if texto.is_empty() {
        return Err(error("peticion_invalida", "texto vacío: nada que escribir"));
    }
    if texto.len() > 64 * 1024 {
        return Err(error(
            "peticion_invalida",
            "texto mayor de 64 KB: trocéalo en varias escrituras",
        ));
    }
    let ejecutor = comun
        .ejecutor
        .as_ref()
        .ok_or_else(|| error("sesion", "sesión sin ejecutor"))?;
    let escritos = ejecutor.escribir(eid, texto.as_bytes()).await.map_err(|e| match e {
        ErrorNucleo::NoEncontrado(m) => error("no_encontrado", m),
        otro => error("sesion", otro.to_string()),
    })?;
    Ok(Json(serde_json::json!({ "ok": true, "escritos": escritos })))
}
