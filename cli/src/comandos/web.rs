//! [069A-2 F1/F2] Servidor HTTP del modo web local (`glory-harness web`):
//! axum loopback con SSE, health, sesión con cookie, turnos reales,
//! cancelación y aprobaciones.
//!
//! # Contrato
//!
//! | Método y ruta                                   | Propósito              |
//! |-------------------------------------------------|------------------------
//! | `GET /healthz`                                  | Liveness               |
//! | `POST /api/v1/session` (Bearer master)          | Crear sesión + cookie  |
//! | `DELETE /api/v1/session/:id`                    | Cerrar sesión          |
//! | `GET /api/v1/session/:id/events`                | SSE (cookie o Bearer)  |
//! | `POST /api/v1/session/:id/turns`                | Iniciar turno (F2)     |
//! | `POST /api/v1/session/:id/turns/:tid/cancel`    | Cancelar turno (F2)    |
//! | `POST /api/v1/session/:id/approvals/:aid`       | Responder aprobación   |
//!
//! Errores: `{ "ok": false, "code": "...", "message": "..." }`.
//! Eventos SSE: tipo en el campo `event:` (`ready`, `turn.started`,
//! `agent.event`, `turn.finished`, `error`); `data` es JSON compacto
//! (sin `\n` literales: seguro para axum 0.8 sin sanitizar).

use std::collections::HashMap;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use serde_json::Value;
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use crate::servicio::{OpcionesSesion, SesionComun};

/// Token maestro: solo crea sesiones. Nunca autoriza nada más.
fn token_desde_env() -> String {
    match std::env::var("GLORY_HARNESS_WEB_TOKEN") {
        Ok(t) if !t.trim().is_empty() => t,
        _ => {
            let generado = Uuid::new_v4().to_string();
            /* [069A-2 v3 §7] Excepción única: impresión a stderr del token
             * temporal en first-run loopback (el usuario local lo necesita);
             * prohibido en logs persistentes. */
            eprintln!(
                "[glory-harness web] AVISO: GLORY_HARNESS_WEB_TOKEN no definido;\
                 se ha generado un token temporal: {generado}\n\
                 Configúralo en el entorno para sesiones persistentes."
            );
            generado
        }
    }
}

/// Nombre de la cookie de sesión (`HttpOnly`, `SameSite=Lax`).
pub(crate) const COOKIE_SESION: &str = "gh_sesion";

/// Turno en curso: id + tarea para abortar al cancelar/cerrar.
pub(crate) struct TurnoActivo {
    pub(crate) id: Uuid,
    pub(crate) handle: JoinHandle<()>,
}

pub(crate) struct SesionWeb {
    pub(crate) comun: SesionComun,
    pub(crate) conversacion_id: Uuid,
    /// Canal broadcast con el JSON de cable `{"event":..,"data":..}` compacto.
    pub(crate) tx: broadcast::Sender<String>,
    pub(crate) turno: Mutex<Option<TurnoActivo>>,
}

pub(crate) struct AppState {
    pub(crate) token: String,
    pub(crate) sesiones: Mutex<HashMap<String, Arc<SesionWeb>>>,
    pub(crate) fixture: bool,
}

// ── Errores API ──────────────────────────────────────────────────────────

#[derive(Debug, serde::Serialize)]
pub(crate) struct ApiError {
    ok: bool,
    code: String,
    message: String,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match self.code.as_str() {
            "no_autorizado" => StatusCode::UNAUTHORIZED,
            "origen" => StatusCode::FORBIDDEN,
            "no_encontrado" | "sin_turno" => StatusCode::NOT_FOUND,
            "turno_activo" => StatusCode::CONFLICT,
            _ => StatusCode::BAD_REQUEST,
        };
        (status, Json(self)).into_response()
    }
}

pub(crate) fn error(code: &str, msg: impl Into<String>) -> ApiError {
    ApiError {
        ok: false,
        code: code.into(),
        message: msg.into(),
    }
}

// ── Autorización ─────────────────────────────────────────────────────────
// [069A-2 F2 §5.2] `EventSource` nativo no envía `Authorization`: la sesión
// se autoriza por cookie `gh_sesion` o por `Bearer <session_id>` (la sesión
// opaca entregada al crearla). El token maestro SOLO crea sesiones.

pub(crate) enum Credencial {
    Maestra,
    Sesion(String),
}

fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::trim)
        .filter(|t| !t.is_empty())
}

fn cookie_sesion(headers: &HeaderMap) -> Option<String> {
    let cookies = headers.get(header::COOKIE)?.to_str().ok()?;
    cookies.split(';').find_map(|parte| {
        let (k, v) = parte.split_once('=')?;
        if k.trim() == COOKIE_SESION {
            let v = v.trim().trim_matches('"');
            if v.is_empty() {
                None
            } else {
                Some(v.to_string())
            }
        } else {
            None
        }
    })
}

async fn credencial(headers: &HeaderMap, state: &AppState) -> Option<Credencial> {
    if let Some(t) = bearer(headers) {
        if t == state.token {
            return Some(Credencial::Maestra);
        }
        if Uuid::parse_str(t).is_ok() && state.sesiones.lock().await.contains_key(t) {
            return Some(Credencial::Sesion(t.to_string()));
        }
        return None;
    }
    if let Some(sid) = cookie_sesion(headers) {
        if Uuid::parse_str(&sid).is_ok() && state.sesiones.lock().await.contains_key(&sid) {
            return Some(Credencial::Sesion(sid));
        }
    }
    None
}

/// Mutaciones con auth por cookie exigen mismo origen (defensa CSRF en
/// profundidad; con `SameSite=Lax` el navegador ya bloquea el envío
/// cross-site, pero `curl`/clientes no-b navegador deben usar Bearer).
fn origen_valido(headers: &HeaderMap, por_cookie: bool, metodo: &Method) -> bool {
    if !por_cookie || *metodo == Method::GET || *metodo == Method::HEAD {
        return true;
    }
    let host = headers
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or_default();
    let mismo = |url: &str| {
        url.rsplit("://")
            .next()
            .unwrap_or(url)
            .split('/')
            .next()
            .unwrap_or_default()
            == host
    };
    let origin_ok = headers
        .get(header::ORIGIN)
        .and_then(|h| h.to_str().ok())
        .is_none_or(mismo);
    let referer_ok = headers
        .get(header::REFERER)
        .and_then(|h| h.to_str().ok())
        .is_none_or(mismo);
    // Sin Origin ni Referer (curl, scripts): se permite; la cookie Lax
    // no viaja cross-site en navegador, así que no hay CSRF que temer.
    origin_ok && referer_ok
}

/// Autoriza una sesión contra el `:id` de la ruta. Devuelve la sesión y si
/// la credencial vino por cookie (para el check de origen).
pub(crate) async fn autorizar_sesion(
    headers: &HeaderMap,
    metodo: &Method,
    state: &AppState,
    id_ruta: &str,
) -> Result<(Arc<SesionWeb>, bool), ApiError> {
    Uuid::parse_str(id_ruta)
        .map_err(|_| error("peticion_invalida", "id de sesión malformado"))?;
    let cred = credencial(headers, state)
        .await
        .ok_or_else(|| error("no_autorizado", "token inválido o ausente"))?;
    let sid = match cred {
        Credencial::Maestra => {
            return Err(error("no_autorizado", "el token maestro no abre sesiones"));
        }
        Credencial::Sesion(s) => s,
    };
    if sid != id_ruta {
        return Err(error("no_autorizado", "la sesión no es tuya"));
    }
    let por_cookie = cookie_sesion(headers).as_deref() == Some(id_ruta);
    if !origen_valido(headers, por_cookie, metodo) {
        return Err(error("origen", "origen no permitido para esta mutación"));
    }
    let sesiones = state.sesiones.lock().await;
    sesiones
        .get(&sid)
        .cloned()
        .map(|s| (s, por_cookie))
        .ok_or_else(|| error("no_encontrado", "sesión no encontrada"))
}

/// Cable SSE: JSON compacto (sin `\n` literales → axum 0.8 no hace panic
/// y no hace falta sanitizar como el parche 069A-1).
pub(crate) fn cable(tipo: &str, data: Value) -> String {
    serde_json::to_string(&serde_json::json!({ "event": tipo, "data": data }))
        .unwrap_or_else(|_| r#"{"event":"error","data":{"code":"interno"}}"#.into())
}

// ── Handlers ─────────────────────────────────────────────────────────────

/// `GET /healthz`
async fn healthz() -> Json<Value> {
    Json(serde_json::json!({ "ok": true }))
}

/// `POST /api/v1/session` — solo token maestro; fija cookie `gh_sesion`.
async fn crear_sesion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    match credencial(&headers, &state).await {
        Some(Credencial::Maestra) => {}
        _ => return Err(error("no_autorizado", "token inválido o ausente")),
    }

    let (comun, apertura) = SesionComun::abrir(OpcionesSesion::default())
        .map_err(|e| error("sesion", e.to_string()))?;

    let (tx, _rx) = broadcast::channel(256);
    let session_id = Uuid::new_v4().to_string();
    let sesion = Arc::new(SesionWeb {
        conversacion_id: apertura.conversacion.id,
        comun,
        tx,
        turno: Mutex::new(None),
    });
    state
        .sesiones
        .lock()
        .await
        .insert(session_id.clone(), Arc::clone(&sesion));

    let cuerpo = Json(serde_json::json!({
        "ok": true,
        "session_id": session_id,
        "modelo": apertura.modelo,
        "workspace": apertura.workspace,
        "proveedores": apertura.proveedores,
        "conversacion": apertura.conversacion,
    }));
    let cookie = format!(
        "{COOKIE_SESION}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400"
    );
    Ok((
        StatusCode::OK,
        [(header::SET_COOKIE, cookie)],
        cuerpo,
    )
        .into_response())
}

/// `DELETE /api/v1/session/:id` — cancela el turno activo y cierra.
async fn cerrar_sesion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::DELETE, &state, &id).await?;
    super::web_turnos::abortar_turno_activo(&sesion, "sesión cerrada").await;
    state.sesiones.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true, "session_id": id })))
}

/// `GET /api/v1/session/:id/events` → SSE con snapshot `ready` + broadcast.
async fn eventos_sse(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>> + Send + 'static>, ApiError>
{
    let (sesion, _) = autorizar_sesion(&headers, &Method::GET, &state, &id).await?;

    // Snapshot mínimo al suscribir (el `ready` de F1 se perdía si el SSE
    // llegaba tarde: reconexión recibe estado actual, sin replay completo).
    let turno_activo = sesion.turno.lock().await.as_ref().map(|t| t.id);
    let ready = cable(
        "ready",
        serde_json::json!({
            "session_id": id,
            "conversacion_id": sesion.conversacion_id,
            "turno_activo": turno_activo,
        }),
    );

    let rx = sesion.tx.subscribe();
    let stream = tokio_stream::once(Ok(Event::default()
        .event("ready")
        .data(ready_json_data(&ready))))
    .chain(BroadcastStream::new(rx).map(|msg| {
        let cable = match msg {
            Ok(c) => c,
            Err(_) => cable("error", serde_json::json!({"code": "lagged"})),
        };
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

// ── Enrutador y arranque ─────────────────────────────────────────────────

pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/session", post(crear_sesion))
        .route("/api/v1/session/{id}", delete(cerrar_sesion))
        .route(
            "/api/v1/session/{id}/events",
            get(eventos_sse),
        )
        .route(
            "/api/v1/session/{id}/turns",
            post(super::web_turnos::iniciar_turno),
        )
        .route(
            "/api/v1/session/{id}/turns/{turn_id}/cancel",
            post(super::web_turnos::cancelar_turno),
        )
        .route(
            "/api/v1/session/{id}/approvals/{approval_id}",
            post(super::web_turnos::responder_aprobacion),
        )
        .with_state(state)
}

/// Punto de entrada del subcomando `web`: `--puerto <N>` (default 8799),
/// `--dir-ui <ruta>` (default `../desktop/ui/dist` relativo al binario),
/// `--fixture` (turnos sintéticos sin proveedor).
pub async fn run(
    puerto: u16,
    ui_dir: Option<String>,
    fixture: bool,
) -> std::process::ExitCode {
    let token = token_desde_env();

    let state = Arc::new(AppState {
        token,
        sesiones: Mutex::new(HashMap::new()),
        fixture,
    });

    let mut app = router(Arc::clone(&state));

    // Servir la UI compilada si se proporciona el directorio
    if let Some(static_dir) = ui_dir {
        let serve_dir = tower_http::services::ServeDir::new(&static_dir)
            .append_index_html_on_directories(true);
        app = app.fallback_service(serve_dir);
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], puerto));
    eprintln!("[glory-harness web] escuchando en http://{addr}");
    eprintln!("[glory-harness web] autenticación: Bearer token en GLORY_HARNESS_WEB_TOKEN");

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[glory-harness web] no se pudo escuchar en {addr}: {e}");
            return std::process::ExitCode::from(1);
        }
    };

    match axum::serve(listener, app).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[glory-harness web] error del servidor: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::Request,
    };
    use tower::ServiceExt;

    /// Estado de prueba con token maestro conocido (sin fixture).
    pub(crate) fn state_test() -> Arc<AppState> {
        Arc::new(AppState {
            token: "test-token".into(),
            sesiones: Mutex::new(HashMap::new()),
            fixture: true,
        })
    }

    fn bearer_master() -> String {
        "Bearer test-token".into()
    }

    #[tokio::test]
    async fn healthz_devuelve_ok() {
        let app = router(state_test());
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
        let body: Value = serde_json::from_slice(
            &axum::body::to_bytes(res.into_body(), 1024).await.unwrap(),
        )
        .unwrap();
        assert_eq!(body["ok"], true);
    }

    #[tokio::test]
    async fn crear_sesion_sin_token_devuelve_401() {
        let app = router(state_test());
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
        let app = router(state_test());
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
    async fn cerrar_sesion_con_master_devuelve_401() {
        // v3: el token maestro solo crea sesiones, no las gestiona.
        let app = router(state_test());
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/v1/session/00000000-0000-0000-0000-000000000000")
                    .header(header::AUTHORIZATION, bearer_master())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn sesion_malformada_devuelve_400() {
        let app = router(state_test());
        let res = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/session/no-es-uuid/events")
                    .header(header::AUTHORIZATION, bearer_master())
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    }
}
