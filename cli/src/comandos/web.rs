//! [069A-2 F1] Servidor HTTP mínimo del modo web local (`glory-harness web`):
//! axum loopback con SSE, health, sesión simple y servido de la UI compilada.
//!
//! Sin proveedor externo, un fixture local permite probar el ciclo de vida:
//! la sesión se abre, emite `ready`, recibe heartbeat y se cierra.
//!
//! # Contrato
//!
//! | Método y ruta                 | Propósito                                    |
//! |-------------------------------|---------------------------------------------|
//! | `GET /healthz`                | Liveness                                     |
//! | `POST /api/v1/session`        | Crear sesión (Bearer token)                  |
//! | `DELETE /api/v1/session/:id`  | Cerrar sesión                                |
//! | `GET /api/v1/session/:id/events` | SSE: ready, heartbeat, turn.*, error    |
//!
//! Errores: `{ "ok": false, "code": "...", "message": "..." }`.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::{header, HeaderMap, Method, StatusCode, Uri},
    response::sse::{Event, Sse},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;

use crate::servicio::{OpcionesSesion, SesionComun};

/// Token de autorización para el servidor web.
fn token_desde_env() -> String {
    match std::env::var("GLORY_HARNESS_WEB_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            let generado = Uuid::new_v4().to_string();
            eprintln!(
                "[glory-harness web] AVISO: GLORY_HARNESS_WEB_TOKEN no definido;\
                 se ha generado un token temporal: {generado}\n\
                 Configúralo en el entorno para sesiones persistentes."
            );
            generado
        }
    }
}

// ── Estado compartido del servidor ────────────────────────────────────────

struct SesionWeb {
    comun: SesionComun,
    /// Canal broadcast para SSE: el servidor envía eventos y todos los
    /// clientes conectados a una sesión los reciben.
    tx: broadcast::Sender<String>,
}

struct AppState {
    token: String,
    sesiones: Mutex<Vec<(String, Arc<SesionWeb>)>>,
}

// ── Errores API ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ApiError {
    ok: bool,
    code: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "no_autorizado" => StatusCode::UNAUTHORIZED,
            "no_encontrado" => StatusCode::NOT_FOUND,
            "sesion_activa" => StatusCode::CONFLICT,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, Json(self)).into_response()
    }
}

fn error(code: &str, msg: impl Into<String>) -> ApiError {
    ApiError {
        ok: false,
        code: code.into(),
        message: msg.into(),
    }
}

// ── Extractor de autorización ────────────────────────────────────────────

fn extraer_bearer(headers: &HeaderMap) -> Option<&str> {
    let auth = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    auth.strip_prefix("Bearer ")
}

fn autorizar(headers: &HeaderMap, token: &str) -> Result<(), ApiError> {
    match extraer_bearer(headers) {
        Some(t) if t == token => Ok(()),
        _ => Err(error("no_autorizado", "token inválido o ausente")),
    }
}

// ── Handlers ─────────────────────────────────────────────────────────────

/// `GET /healthz`
async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// `POST /api/v1/session`
async fn crear_sesion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    autorizar(&headers, &state.token)?;

    let (comun, apertura) = SesionComun::abrir(OpcionesSesion::default())
        .map_err(|e| error("sesion", e.to_string()))?;

    let (tx, _rx) = broadcast::channel(128);

    // Emitir `ready` en el canal SSE
    let session_id = Uuid::new_v4().to_string();
    let ready = serde_json::json!({
        "event": "ready",
        "data": {
            "session_id": session_id,
            "modelo": apertura.modelo,
            "workspace": apertura.workspace,
            "proveedores": apertura.proveedores,
            "conversacion": apertura.conversacion,
        }
    });
    let _ = tx.send(serde_json::to_string(&ready).unwrap_or_default());

    let sesion = Arc::new(SesionWeb { comun, tx });
    let mut sesiones = state.sesiones.lock().await;
    sesiones.push((session_id.clone(), sesion));

    Ok(Json(serde_json::json!({
        "ok": true,
        "session_id": session_id,
        "modelo": apertura.modelo,
        "workspace": apertura.workspace,
        "proveedores": apertura.proveedores,
        "conversacion": apertura.conversacion,
    })))
}

/// `DELETE /api/v1/session/:id`
async fn cerrar_sesion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    autorizar(&headers, &state.token)?;

    let mut sesiones = state.sesiones.lock().await;
    let pos = sesiones.iter().position(|(sid, _)| sid == &id);
    match pos {
        Some(i) => {
            sesiones.swap_remove(i);
            Ok(Json(serde_json::json!({ "ok": true, "session_id": id })))
        }
        None => Err(error("no_encontrado", "sesión no encontrada")),
    }
}

/// `GET /api/v1/session/:id/events` → SSE
async fn eventos_sse(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>> + Send + 'static>, ApiError> {
    autorizar(&headers, &state.token)?;

    let sesion = {
        let sesiones = state.sesiones.lock().await;
        sesiones
            .iter()
            .find(|(sid, _)| sid == &id)
            .map(|(_, s)| Arc::clone(s))
            .ok_or_else(|| error("no_encontrado", "sesión no encontrada"))?
    };

    let rx = sesion.tx.subscribe();
    let (tx, rx_stream) = mpsc::unbounded_channel();

    // Reenviar del broadcast al canal unbounded del SSE
    tokio::spawn(async move {
        let mut rx = rx;
        loop {
            match rx.recv().await {
                Ok(msg) => {
                    if tx.send(msg).is_err() {
                        break; // cliente cerró
                    }
                }
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    let warn = serde_json::json!({
                        "event": "error",
                        "data": { "code": "lagged", "message": format!("omitidos {n} eventos") }
                    });
                    let _ = tx.send(serde_json::to_string(&warn).unwrap_or_default());
                }
            }
        }
    });

    // Mapear String → SSE Event y mantener keep-alive
    let stream = UnboundedReceiverStream::new(rx_stream)
        .map(|msg| Ok(Event::default().data(msg)));

    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("data: {\"event\":\"heartbeat\",\"data\":{\"at\":\"...\"}}\n\n"),
    ))
}

// ── Enrutador completo ───────────────────────────────────────────────────

fn router(state: Arc<AppState>, ui_dir: Option<String>) -> Router {
    let mut app = Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/session", post(crear_sesion))
        .route("/api/v1/session/{id}", delete(cerrar_sesion))
        .route("/api/v1/session/{id}/events", get(eventos_sse))
        .with_state(state);

    // Servir la UI compilada si se proporciona el directorio
    if let Some(static_dir) = ui_dir {
        let serve_dir = tower_http::services::ServeDir::new(&static_dir)
            .append_index_html_on_directories(true);
        app = app.fallback_service(serve_dir);
    }

    app
}

// ── Arranque ─────────────────────────────────────────────────────────────

/// Punto de entrada del subcomando `web`. Args esperados:
/// `--puerto <N>` (default 8799), `--dir-ui <ruta>` (default
/// `../desktop/ui/dist` relativo al binario), `--fixture` (sin proveedor).
pub async fn run(puerto: u16, ui_dir: Option<String>, _fixture: bool) -> std::process::ExitCode {
    let token = token_desde_env();

    let state = Arc::new(AppState {
        token,
        sesiones: Mutex::new(Vec::new()),
    });

    let app = router(Arc::clone(&state), ui_dir);

    let addr = SocketAddr::from(([127, 0, 0, 1], puerto));
    eprintln!("[glory-harness web] escuchando en http://{addr}");
    eprintln!(
        "[glory-harness web] autenticación: Bearer token en GLORY_HARNESS_WEB_TOKEN"
    );

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[glory-harness web] no se pudo escuchar en {addr}: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    axum::serve(listener, app)
        .await
        .unwrap_or_else(|e| {
            eprintln!("[glory-harness web] error del servidor: {e}");
        });

    std::process::ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    /// Crea un estado de prueba con un token conocido.
    fn state_test() -> Arc<AppState> {
        Arc::new(AppState {
            token: "test-token".into(),
            sesiones: Mutex::new(Vec::new()),
        })
    }

    #[tokio::test]
    async fn healthz_devuelve_ok() {
        let app = router(state_test(), None);
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        let body: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), 1024).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["ok"], true);
    }

    #[tokio::test]
    async fn crear_sesion_sin_token_devuelve_401() {
        let app = router(state_test(), None);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/session")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn crear_sesion_con_token_falso_devuelve_401() {
        let app = router(state_test(), None);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/session")
                    .header(header::AUTHORIZATION, "Bearer fake-token")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn cerrar_sesion_inexistente_devuelve_404() {
        let app = router(state_test(), None);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/v1/session/no-existe")
                    .header(header::AUTHORIZATION, "Bearer test-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::NOT_FOUND);
    }
}