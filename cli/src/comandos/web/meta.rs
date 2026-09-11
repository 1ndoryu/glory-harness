/* [109A-5 F3] Ciclo de vida de la meta por HTTP.
 *
 * Se separó de `web.rs` —que quedó por encima del límite de 500 líneas para
 * un archivo de servicio— sin cambiar rutas ni contrato: cuando hay
 * conversación la meta vive en su fila durable (reloj neto de pausas e
 * historial de logros) y, mientras la conversación no exista
 * (create-on-write), en el borrador en memoria de la sesión.
 */

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    Json,
};
use glory_harness_core::evento::AgenteEvento;
use serde_json::Value;

use crate::servicio::{aplicar_en_borrador, comando_desde_payload};

use super::{autorizar_sesion, cable, error, AppState, ApiError};

/// `PATCH /api/v1/session/:id/meta` — ciclo de vida de la meta ([109A-5 F1]).
///
/// `meta` sin `accion` conserva el contrato del panel (texto = fijar, ausente
/// o vacío = limpiar); `accion` explícita permite pausar/reanudar/lograr.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ActualizarMeta {
    pub(crate) meta: Option<String>,
    pub(crate) accion: Option<String>,
    pub(crate) turno_id: Option<String>,
}

pub(crate) async fn actualizar_meta(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<ActualizarMeta>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::PATCH, &state, &id).await?;
    let comando = comando_desde_payload(
        peticion.meta,
        peticion.accion.as_deref(),
        peticion.turno_id.as_deref(),
    )
    .map_err(|e| error(e.codigo(), e.to_string()))?;
    let conversacion = *sesion.conversacion_id.lock().await;
    let meta = match conversacion {
        Some(conversacion) => {
            /* `SesionComun` es `Clone` y no guarda estado mutable propio: se
             * clona bajo el lock para no dejarlo tomado durante el `await` del
             * evento SSE. */
            let mut comun = sesion.comun.lock().await.clone();
            /* [109A-5 F3] La respuesta lleva el estado COMPLETO (meta activa +
             * historial) y el logro recién creado cuando el comando fue
             * `lograr`: el panel repinta persecución e historial con la misma
             * respuesta y pinta el badge del pie sin una segunda consulta. */
            let resultado = comun
                .meta_aplicar(conversacion, comando)
                .map_err(|e| error(e.codigo(), e.to_string()))?;
            /* [109A-5 F3] El badge del pie se pinta con un evento, igual que en
             * la ventana Tauri: el navegador lo recibe como `agent.event` por el
             * SSE de la sesión y lo aplica al pie del turno que respalda el
             * logro. Se emite antes de responder para que el badge no llegue
             * después del repintado del panel. */
            if let Some(logro) = &resultado.logro {
                sesion
                    .emitir(cable(
                        "agent.event",
                        serde_json::to_value(AgenteEvento::MetaLograda {
                            meta: logro.meta.clone(),
                            lograda_en: logro.lograda_en.to_rfc3339(),
                            elapsed_ms: logro.elapsed_ms,
                            turno_id: logro.turno_id,
                        })
                        .unwrap_or(Value::Null),
                    ))
                    .await;
            }
            return Ok(Json(serde_json::json!({
                "ok": true,
                "meta": resultado.estado.texto_activo().map(str::to_owned),
                "estado": resultado.estado,
                "logro": resultado.logro,
            })));
        }
        None => {
            let mut borrador = sesion.meta.lock().await;
            *borrador =
                aplicar_en_borrador(comando).map_err(|e| error(e.codigo(), e.to_string()))?;
            borrador.clone()
        }
    };
    // Sin conversación solo existe el borrador en memoria: no hay reloj
    // durable, así que `estado`/`logro` son nulos (no un estado inventado).
    Ok(Json(serde_json::json!({
        "ok": true,
        "meta": meta,
        "estado": Value::Null,
        "logro": Value::Null,
    })))
}

/// `GET /api/v1/session/:id/meta` — estado durable de la meta ([109A-5 F3]).
///
/// El panel necesita leer antes de pintar: la meta activa, su reloj neto de
/// pausas y el historial de logros viven en la fila de la conversación, no en
/// el DOM. Sin conversación devuelve el borrador en memoria y `estado: null`,
/// que es un estado legítimo (panel recién abierto, sin primer mensaje).
pub(crate) async fn leer_meta(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::GET, &state, &id).await?;
    let conversacion = *sesion.conversacion_id.lock().await;
    match conversacion {
        Some(conversacion) => {
            let comun = sesion.comun.lock().await;
            let estado = comun
                .meta_leer(conversacion)
                .map_err(|e| error(e.codigo(), e.to_string()))?;
            Ok(Json(serde_json::json!({
                "ok": true,
                "meta": estado.texto_activo().map(str::to_owned),
                "estado": estado,
            })))
        }
        None => {
            let borrador = sesion.meta.lock().await.clone();
            Ok(Json(serde_json::json!({
                "ok": true,
                "meta": borrador,
                "estado": Value::Null,
            })))
        }
    }
}
