//! [069A-2 F1/F2] Servidor HTTP del modo web local (`glory-harness web`):
//! axum loopback con SSE, health, sesión con cookie, turnos reales,
//! cancelación y aprobaciones.
//!
//! [069A-2 F6] Límites: cuerpo HTTP 256 KiB, mensaje 32k chars
//! (`web_turnos.rs`), 16 sesiones vivas con TTL 24 h (igual que la cookie).
//!
//! # Contrato
//!
//! | Método y ruta                                   | Propósito              |
//! |-------------------------------------------------|------------------------
//! | `GET /healthz`                                  | Liveness               |
//! | `POST /api/v1/session` (Bearer master)          | Crear sesión + cookie  |
//! | `DELETE /api/v1/session/:id`                    | Cerrar sesión          |
//! | `GET /api/v1/session/:id/events`                | SSE (cookie o Bearer)  |
//! | `PATCH /api/v1/session/:id/meta`                | Fijar/limpiar meta     |
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
use std::time::{Duration, Instant};

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
    response::{IntoResponse, Response},
    routing::{delete, get, patch, post},
    Json, Router,
};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::StreamExt;
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

use crate::servicio::{OpcionesSesion, SesionComun};

use super::web_sse::DifusionSse;

/// Token maestro opcional: solo crea sesiones. Nunca autoriza nada más.
/// Sin token configurado, `web` funciona en modo local tokenless; `run` lo
/// limita a loopback para que esa comodidad no exponga una API sin auth.
fn token_desde_env() -> Option<String> {
    match std::env::var("GLORY_HARNESS_WEB_TOKEN") {
        Ok(t) if !t.trim().is_empty() => Some(t),
        _ => None,
    }
}

/// Nombre de la cookie de sesión (`HttpOnly`, `SameSite=Lax`).
pub(crate) const COOKIE_SESION: &str = "gh_sesion";

/// [069A-2 F6] Límites del modo web local (single-user, loopback).
/// Cuerpo HTTP máximo por petición (mensajes, config, workspace).
pub(crate) const BODY_MAX_BYTES: usize = 256 * 1024;
/// Sesiones vivas máximas (cada una retiene runtime + difusión SSE).
pub(crate) const MAX_SESIONES: usize = 16;
/// TTL de sesión en segundos (24 h, igual que `Max-Age` de la cookie).
pub(crate) const SESION_TTL_SECS: u64 = 86_400;
/// Límite de la meta para evitar payloads grandes y mantener el contrato del UI.
pub(crate) const MAX_META_CHARS: usize = 8_000;
/// Turno en curso: id + tarea para abortar al cancelar/cerrar.
pub(crate) struct TurnoActivo {
    pub(crate) id: Uuid,
    pub(crate) handle: JoinHandle<()>,
}

pub(crate) struct SesionWeb {
    /// Mutex: `PATCH config` y `POST workspace` reconstruyen el runtime.
    pub(crate) comun: Mutex<SesionComun>,
    /// [069A-7] Conversación actual de la sesión (sin paneles en web).
    /// `Option`: `None` = sin conversación todavía (borrador). La fila se crea
    /// SOLO al escribir el primer mensaje (create-on-write); `None` es el
    /// estado legítimo tras abrir con lista vacía o borrar la última.
    pub(crate) conversacion_id: Mutex<Option<Uuid>>,
    /// [079A-1 F1] Difusión SSE lock-free (un mpsc acotado por suscriptor,
    /// ver `web_sse.rs`): el JSON de cable `{"event":..,"data":..}` compacto.
    pub(crate) sse: Mutex<DifusionSse>,
    /// Objetivo vigente del modo `meta`, equivalente al estado IPC de Tauri.
    pub(crate) meta: Mutex<Option<String>>,
    pub(crate) turno: Mutex<Option<TurnoActivo>>,
    /// [069A-2 F6] Creación para TTL (las sesiones no son eternas aunque el
    /// proceso viva días; el cierre explícito sigue siendo `DELETE`).
    pub(crate) creada: Instant,
}

impl SesionWeb {
    /// [079A-1 F1] Emite un cable a los suscriptores SSE (best-effort,
    /// lock-free; ver `web_sse.rs`). Nunca bloquea ni falla.
    pub(crate) async fn emitir(&self, cable: String) {
        self.sse.lock().await.emitir(cable);
    }
}

pub(crate) struct AppState {
    /// `None` = modo local tokenless; el servidor solo debe bindear loopback.
    pub(crate) token: Option<String>,
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
            "mensaje_largo" | "meta_larga" => StatusCode::PAYLOAD_TOO_LARGE,
            "demasiadas_sesiones" => StatusCode::TOO_MANY_REQUESTS,
            "sesion_expirada" => StatusCode::GONE,
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
    /// Indica si la credencial vino por cookie (para origen_valido).
    SesionCookie(String),
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
        if state.token.as_deref() == Some(t) {
            return Some(Credencial::Maestra);
        }
        if Uuid::parse_str(t).is_ok() && state.sesiones.lock().await.contains_key(t) {
            // Autenticado por Bearer → no es cookie, mutaciones permitidas.
            return Some(Credencial::Sesion(t.to_string()));
        }
        return None;
    }
    if let Some(sid) = cookie_sesion(headers) {
        if Uuid::parse_str(&sid).is_ok() && state.sesiones.lock().await.contains_key(&sid) {
            return Some(Credencial::SesionCookie(sid));
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
    Uuid::parse_str(id_ruta).map_err(|_| error("peticion_invalida", "id de sesión malformado"))?;
    let cred = credencial(headers, state)
        .await
        .ok_or_else(|| error("no_autorizado", "token inválido o ausente"))?;
    let (sid, por_cookie) = match cred {
        Credencial::Maestra => {
            return Err(error("no_autorizado", "el token maestro no abre sesiones"));
        }
        Credencial::Sesion(s) => (s, false),
        Credencial::SesionCookie(s) => (s, true),
    };
    if sid != id_ruta {
        return Err(error("no_autorizado", "sesión no autorizada"));
    }
    if !origen_valido(headers, por_cookie, metodo) {
        return Err(error("origen", "origen no permitido para esta mutación"));
    }
    let mut sesiones = state.sesiones.lock().await;
    let sesion = sesiones
        .get(&sid)
        .cloned()
        .ok_or_else(|| error("no_encontrado", "sesión no encontrada"))?;
    // [069A-2 F6] TTL: la sesión expira aunque el proceso siga vivo (el
    // cliente debe crear otra; el turno activo se aborta para no dejar un
    // turno eternamente "ejecutando" sin dueño).
    if sesion.creada.elapsed().as_secs() > SESION_TTL_SECS {
        sesiones.remove(&sid);
        drop(sesiones);
        super::web_turnos::abortar_turno_activo(&sesion, "sesión expirada").await;
        return Err(error(
            "sesion_expirada",
            "sesión expirada (24 h): crea otra",
        ));
    }
    Ok((sesion, por_cookie))
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
    let autorizado = state.token.is_none()
        || matches!(
            credencial(&headers, &state).await,
            Some(Credencial::Maestra)
        );
    if !autorizado {
        return Err(error("no_autorizado", "token inválido o ausente"));
    }

    let (comun, apertura) = SesionComun::abrir(OpcionesSesion::default())
        .map_err(|e| error("sesion", e.to_string()))?;

    /* [069A-7] Create-on-write: si el servicio auto-creó una "Nueva
     * conversación" vacía porque no había ninguna, se DESCARTA aquí (se
     * elimina la fila) y la sesión arranca en borrador (`conversacion:
     * null`). La primera escritura del usuario creará la fila real. Si había
     * una conversación previa, se conserva como la actual (decisión A). */
    let conv_autocreada = apertura.conv_autocreada;
    let conversacion = if conv_autocreada {
        comun
            .persistencia
            .conversacion_eliminar(apertura.conversacion.id, comun.user_id)
            .map_err(|e| error("sesion", e.to_string()))?;
        None
    } else {
        Some(apertura.conversacion)
    };

    let session_id = Uuid::new_v4().to_string();
    let sesion = Arc::new(SesionWeb {
        comun: Mutex::new(comun),
        conversacion_id: Mutex::new(conversacion.as_ref().map(|c| c.id)),
        sse: Mutex::new(DifusionSse::nueva()),
        meta: Mutex::new(None),
        turno: Mutex::new(None),
        creada: Instant::now(),
    });
    {
        let mut sesiones = state.sesiones.lock().await;
        // [069A-2 F6] Purga perezosa de expiradas + tope de vivas: el modo
        // web es single-user loopback, no un multitenant.
        sesiones.retain(|_, s| s.creada.elapsed().as_secs() <= SESION_TTL_SECS);
        if sesiones.len() >= MAX_SESIONES {
            return Err(error(
                "demasiadas_sesiones",
                "demasiadas sesiones abiertas: cierra alguna con DELETE",
            ));
        }
        sesiones.insert(session_id.clone(), Arc::clone(&sesion));
    }

    let cuerpo = Json(serde_json::json!({
        "ok": true,
        "session_id": session_id,
        "modelo": apertura.modelo,
        "workspace": apertura.workspace,
        "proveedores": apertura.proveedores,
        /* [069A-7] `null` cuando no hay conversación (borrador); objeto cuando
         * la apertura ancló una existente. */
        "conversacion": conversacion,
    }));
    let cookie =
        format!("{COOKIE_SESION}={session_id}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400");
    Ok((StatusCode::OK, [(header::SET_COOKIE, cookie)], cuerpo).into_response())
}

/// `PATCH /api/v1/session/:id/meta` — fija o limpia la meta de la sesión.
#[derive(Debug, serde::Deserialize)]
pub(crate) struct ActualizarMeta {
    pub(crate) meta: Option<String>,
}

pub(crate) async fn actualizar_meta(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<ActualizarMeta>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::PATCH, &state, &id).await?;
    let normalizada = peticion
        .meta
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    if normalizada
        .as_ref()
        .is_some_and(|m| m.chars().count() > MAX_META_CHARS)
    {
        return Err(error(
            "meta_larga",
            "meta demasiado larga (máx. 8000 caracteres)",
        ));
    }
    *sesion.meta.lock().await = normalizada.clone();
    Ok(Json(serde_json::json!({ "ok": true, "meta": normalizada })))
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

/// `GET /api/v1/session/:id/events` → SSE con snapshot `ready` + difusión.
async fn eventos_sse(
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
    // en vez de avisar (ver `web_sse.rs`); el lector ve eventos contiguos.
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

// ── Enrutador y arranque ─────────────────────────────────────────────────

pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/v1/session", post(crear_sesion))
        .route("/api/v1/session/{id}", delete(cerrar_sesion))
        .route("/api/v1/session/{id}/events", get(eventos_sse))
        .route("/api/v1/session/{id}/meta", patch(actualizar_meta))
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
        .route(
            "/api/v1/session/{id}/conversations",
            get(super::web_datos::listar_conversaciones).post(super::web_datos::crear_conversacion),
        )
        .route(
            "/api/v1/session/{id}/conversations/{cid}/messages",
            get(super::web_datos::cargar_conversacion),
        )
        .route(
            "/api/v1/session/{id}/conversations/{cid}",
            patch(super::web_datos::parchear_conversacion)
                .delete(super::web_datos::eliminar_conversacion),
        )
        .route(
            "/api/v1/session/{id}/providers",
            get(super::web_datos::leer_proveedores),
        )
        .route(
            "/api/v1/session/{id}/config",
            get(super::web_datos::leer_config).patch(super::web_datos::guardar_config),
        )
        .route(
            "/api/v1/session/{id}/workspace",
            get(super::web_datos::leer_workspace).post(super::web_datos::cambiar_workspace_ep),
        )
        .route(
            "/api/v1/session/{id}/workspaces",
            get(super::web_datos::listar_workspaces).post(super::web_datos::crear_workspace),
        )
        .route(
            "/api/v1/session/{id}/workspaces/{wid}",
            patch(super::web_datos::renombrar_workspace)
                .delete(super::web_datos::eliminar_workspace),
        )
        // [089A-10] Files y Git del workspace activo (modo web): GET de solo
        // lectura sobre la raíz de la sesión; sin watcher (ponytail).
        .route(
            "/api/v1/session/{id}/files/info",
            get(super::web_datos::files_info),
        )
        .route(
            "/api/v1/session/{id}/files/listar",
            get(super::web_datos::files_listar),
        )
        .route(
            "/api/v1/session/{id}/files/leer",
            get(super::web_datos::files_leer),
        )
        .route(
            "/api/v1/session/{id}/files/buscar",
            get(super::web_datos::files_buscar),
        )
        .route(
            "/api/v1/session/{id}/git/estado",
            get(super::web_datos::git_estado),
        )
        // [069A-2 F6] Tope de cuerpo por petición (axum trae 2 MiB por
        // defecto; 256 KiB cubre mensaje/config/workspace de sobra).
        .layer(RequestBodyLimitLayer::new(BODY_MAX_BYTES))
        .with_state(state)
}

/// Punto de entrada del subcomando `web`: `--puerto <N>` (default 8799),
/// `--dir-ui <ruta>` (default `../desktop/ui/dist` relativo al binario),
/// `--fixture` (turnos sintéticos sin proveedor).
pub async fn run(puerto: u16, ui_dir: Option<String>, fixture: bool) -> std::process::ExitCode {
    let token = token_desde_env();
    let tokenless = token.is_none();

    let state = Arc::new(AppState {
        token,
        sesiones: Mutex::new(HashMap::new()),
        fixture,
    });

    let mut app = router(Arc::clone(&state));

    // Servir la UI compilada si se proporciona el directorio
    if let Some(static_dir) = ui_dir {
        let serve_dir =
            tower_http::services::ServeDir::new(&static_dir).append_index_html_on_directories(true);
        app = app.fallback_service(serve_dir);
    }

    // Sin token, bind loopback: el acceso tokenless es solo para el usuario
    // local. Con token explícito se puede servir en todas las interfaces.
    let addr = if tokenless {
        SocketAddr::from(([127, 0, 0, 1], puerto))
    } else {
        SocketAddr::from(([0, 0, 0, 0], puerto))
    };
    eprintln!("[glory-harness web] escuchando en http://{addr}");
    if tokenless {
        eprintln!("[glory-harness web] modo local: sin token, solo loopback");
    } else {
        eprintln!("[glory-harness web] autenticación: Bearer token en GLORY_HARNESS_WEB_TOKEN");
    }

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
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    /// Estado de prueba con token maestro conocido (sin fixture).
    pub(crate) fn state_test() -> Arc<AppState> {
        Arc::new(AppState {
            token: Some("test-token".into()),
            sesiones: Mutex::new(HashMap::new()),
            fixture: true,
        })
    }

    fn bearer_master() -> String {
        "Bearer test-token".into()
    }

    /// Sesión en BD memoria (sin tocar la SQLite real del usuario).
    /// [069A-7] Representa una sesión que YA tiene una conversación anclada
    /// (la que el servicio auto-creó al abrir, conservada como actual):
    /// los tests de turnos/CRUD existentes envían sin `conversacion_id` y
    /// dependen de que la sesión tenga una. El estado "borrador sin
    /// conversación" (create-on-write) se cubre descartando la conversación
    /// auto-creada tras abrir, igual que hace el arranque real con lista
    /// vacía (antes en `sesion_memoria_borrador`, eliminada por no usarse).
    pub(crate) async fn sesion_memoria(state: &Arc<AppState>) -> (String, Arc<SesionWeb>) {
        let persist = crate::PersistenciaSqlite::en_memoria().expect("bd memoria");
        let (comun, apertura) = crate::servicio::SesionComun::abrir_con_persistencia(
            crate::servicio::OpcionesSesion::default(),
            persist,
            None,
        )
        .expect("abrir sesión memoria");
        let sid = Uuid::new_v4().to_string();
        let sesion = Arc::new(SesionWeb {
            conversacion_id: Mutex::new(Some(apertura.conversacion.id)),
            comun: Mutex::new(comun),
            sse: Mutex::new(DifusionSse::nueva()),
            meta: Mutex::new(None),
            turno: Mutex::new(None),
            creada: Instant::now(),
        });
        state
            .sesiones
            .lock()
            .await
            .insert(sid.clone(), Arc::clone(&sesion));
        (sid, sesion)
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
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body["ok"], true);
    }

    fn state_local() -> Arc<AppState> {
        Arc::new(AppState {
            token: None,
            sesiones: Mutex::new(HashMap::new()),
            fixture: true,
        })
    }

    #[tokio::test]
    async fn crear_sesion_sin_token_en_modo_local_devuelve_200() {
        let app = router(state_local());
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
        assert_eq!(res.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn meta_web_se_fija_y_se_normaliza() {
        let state = state_test();
        let (sid, sesion) = sesion_memoria(&state).await;
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/v1/session/{sid}/meta"))
                    .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"meta":"  objetivo claro  "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(sesion.meta.lock().await.as_deref(), Some("objetivo claro"));
    }

    #[tokio::test]
    async fn meta_web_rechaza_exceso_y_sesion_ajena() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = router(Arc::clone(&state));
        let larga = "x".repeat(MAX_META_CHARS + 1);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/v1/session/{sid}/meta"))
                    .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(serde_json::json!({ "meta": larga }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let app2 = router(Arc::clone(&state));
        let res2 = app2
            .oneshot(
                Request::builder()
                    .method(Method::PATCH)
                    .uri(format!("/api/v1/session/{sid}/meta"))
                    .header(
                        header::AUTHORIZATION,
                        "Bearer 00000000-0000-0000-0000-000000000000",
                    )
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"meta":"ajena"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res2.status(), StatusCode::UNAUTHORIZED);
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

    /// [069A-2 F6] Sesión con `creada` más vieja que el TTL → 410 y purga.
    #[tokio::test]
    async fn sesion_expirada_devuelve_410_y_purga() {
        let state = state_test();
        let persist = crate::PersistenciaSqlite::en_memoria().expect("bd memoria");
        let (comun, apertura) = crate::servicio::SesionComun::abrir_con_persistencia(
            crate::servicio::OpcionesSesion::default(),
            persist,
            None,
        )
        .expect("abrir sesión memoria");
        let sid = Uuid::new_v4().to_string();
        let vieja = Instant::now() - Duration::from_secs(SESION_TTL_SECS + 60);
        state.sesiones.lock().await.insert(
            sid.clone(),
            Arc::new(SesionWeb {
                conversacion_id: Mutex::new(Some(apertura.conversacion.id)),
                comun: Mutex::new(comun),
                sse: Mutex::new(DifusionSse::nueva()),
                meta: Mutex::new(None),
                turno: Mutex::new(None),
                creada: vieja,
            }),
        );
        let app = router(Arc::clone(&state));
        let res = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/session/{sid}/events"))
                    .header(header::COOKIE, format!("{COOKIE_SESION}={sid}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::GONE);
        assert!(!state.sesiones.lock().await.contains_key(&sid));
    }

    /// [069A-2 F6] Con el tope de sesiones vivas, crear otra → 429.
    #[tokio::test]
    async fn crear_sesion_con_tope_devuelve_429() {
        let state = state_test();
        for _ in 0..MAX_SESIONES {
            sesion_memoria(&state).await;
        }
        let app = router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/v1/session")
                    .header(header::AUTHORIZATION, bearer_master())
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    }
}
