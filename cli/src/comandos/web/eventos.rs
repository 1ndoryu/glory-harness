//! [Partición limite-lineas] SSE y liveness del modo web local:
//! `GET /healthz` y `GET /api/v1/session/:id/events` (snapshot `ready` +
//! difusión), más el desempaquetado del cable SSE. Extraído de `mod.rs`
//! (superaba 500 líneas efectivas). El constructor del cable (`cable`) y la
//! autorización siguen en el padre porque `turnos.rs`/`meta.rs` los usan.

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde_json::Value;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;

use super::{autorizar_sesion, cable, ApiError, AppState};

/// `GET /healthz`
pub(crate) async fn healthz() -> Json<Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// `GET /api/v1/session/:id/events` → SSE con snapshot `ready` + difusión.
pub(crate) async fn eventos_sse(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<
    Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>> + Send + 'static>,
    ApiError,
> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::GET, &state, &id).await?;

    // Snapshot mínimo al suscribir (el `ready` de F1 se perdía si el SSE
    // llegaba tarde: reconexión recibe estado actual, sin replay completo).
    let turno_activo = sesion.turno.lock().await.as_ref().map(|t| t.id);
    let conversacion_id = *sesion.conversacion_id.lock().await;
    let ready = cable(
        "ready",
        serde_json::json!({
            "session_id": id,
            "conversacion_id": conversacion_id,
            "turno_activo": turno_activo,
        }),
    );

    let rx = sesion.sse.lock().await.suscribir().1;
    let stream = tokio_stream::once(Ok(Event::default()
        .event("ready")
        .data(ready_json_data(&ready))))
    // [079A-1 F1] Sin `Lagged`: el mpsc acotado descarta ante lector lento
    // en vez de avisar (ver `sse.rs`); el lector ve eventos contiguos.
    .chain(ReceiverStream::new(rx).map(|cable| {
        let (tipo, data) = partir_cable(&cable);
        Ok(Event::default().event(tipo).data(data))
    }));

    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("heartbeat"),
    ))
}

/// Extrae `data` del cable (el `event:` viaja en el campo SSE, no en data).
fn ready_json_data(cable: &str) -> String {
    partir_cable(cable).1
}

fn partir_cable(cable: &str) -> (String, String) {
    match serde_json::from_str::<Value>(cable) {
        Ok(v) => (
            v.get("event")
                .and_then(|e| e.as_str())
                .unwrap_or("message")
                .to_string(),
            v.get("data")
                .map(|d| serde_json::to_string(d).unwrap_or_default())
                .unwrap_or_default(),
        ),
        Err(_) => ("error".into(), r#"{"code":"cable"}"#.into()),
    }
}
