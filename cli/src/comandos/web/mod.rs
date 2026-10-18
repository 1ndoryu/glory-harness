//! [069A-2 F1/F2] Servidor HTTP del modo web local (`glory-harness web`):
//! axum loopback con SSE, health, sesión con cookie, turnos reales,
//! cancelación y aprobaciones.
//!
//! [069A-2 F6] Límites: cuerpo HTTP 256 KiB, mensaje 32k chars
//! (`turnos.rs`), 16 sesiones vivas con TTL 24 h (igual que la cookie).
//!
//! # Contrato
//!
//! | Verbo y ruta                                    | Propósito              |
//! |-------------------------------------------------|------------------------
//! | `GET /healthz`                                  | Liveness               |
//! | `POST /api/v1/session` (Bearer master)          | Crear sesión + cookie  |
//! | `GET /api/v1/session/actual` (cookie)            | Reanudar sesión viva   |
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

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use axum::{
    routing::{delete, get, patch, post},
    Router,
};
use serde_json::Value;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use tower_http::limit::RequestBodyLimitLayer;
use uuid::Uuid;

use crate::servicio::SesionComun;

// Dominios del servidor web, en subdirectorio para no abarrotar `comandos/`.
pub mod eventos;
pub mod meta;
pub mod seguridad;
pub mod sesiones;
pub mod sse;
pub mod turnos;
mod autorizacion;
mod errores;
pub(crate) use autorizacion::{autorizar_sesion, credencial, Credencial};
pub(crate) use errores::{error, ApiError};


use self::meta::{actualizar_meta, leer_meta};
use self::seguridad::SecretoSesion;
use self::sse::DifusionSse;

/// Nombre de la cookie de sesión (`HttpOnly`, `SameSite=Lax`).
pub(crate) const COOKIE_SESION: &str = "gh_sesion";

/// [069A-2 F6] Límites del modo web local (single-user, loopback).
/// Cuerpo HTTP máximo por petición (mensajes, config, workspace).
pub(crate) const BODY_MAX_BYTES: usize = 256 * 1024;
/// Sesiones vivas máximas (cada una retiene runtime + difusión SSE).
pub(crate) const MAX_SESIONES: usize = 16;
/// TTL de sesión en segundos (24 h, igual que `Max-Age` de la cookie).
pub(crate) const SESION_TTL_SECS: u64 = 86_400;

/// Antigüedad de una sesión en segundos. `SystemTime` (no `Instant`): el TTL
/// es tiempo de pared y debe funcionar aunque el uptime < TTL (el test de
/// expiración restaba `TTL+60s` a `Instant::now()` y desbordaba en máquinas
/// con menos de un día encendidas). Reloj hacia atrás = fresca (fail-closed:
/// nunca se purga por skew).
pub(crate) fn antiguedad_sesion(creada: SystemTime) -> u64 {
    creada.elapsed().unwrap_or(Duration::ZERO).as_secs()
}
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
    /// ver `sse.rs`): el JSON de cable `{"event":..,"data":..}` compacto.
    pub(crate) sse: Mutex<DifusionSse>,
    /// Objetivo vigente del modo `meta`, equivalente al estado IPC de Tauri.
    pub(crate) meta: Mutex<Option<String>>,
    pub(crate) turno: Mutex<Option<TurnoActivo>>,
    /// [069A-2 F6] Creación para TTL (las sesiones no son eternas aunque el
    /// proceso viva días; el cierre explícito sigue siendo `DELETE`).
    pub(crate) creada: SystemTime,
}

impl SesionWeb {
    /// [079A-1 F1] Emite un cable a los suscriptores SSE (best-effort,
    /// lock-free; ver `sse.rs`). Nunca bloquea ni falla.
    pub(crate) async fn emitir(&self, cable: String) {
        self.sse.lock().await.emitir(cable);
    }
}

pub(crate) struct AppState {
    /// `None` = modo local tokenless; el servidor solo debe bindear loopback.
    pub(crate) token: Option<String>,
    pub(crate) sesiones: Mutex<HashMap<String, Arc<SesionWeb>>>,
    pub(crate) fixture: bool,
    /// [139A-8 K9] Secreto de firma de la cookie `gh_sesion` (aleatorio por
    /// arranque salvo `--secreto-sesion` / env para fijarlo).
    pub(crate) secreto: SecretoSesion,
    /// [139A-8 K12] rate_limit por ventana deslizante de inicios de turno
    /// (más tope de sesiones y `RequestBodyLimitLayer` en el router; el
    /// servidor solo bindea loopback). Responde 429 ante exceso.
    pub(crate) inicios_turno: Mutex<VecDeque<Instant>>,
}

/// Cable SSE: JSON compacto (sin `\n` literales → axum 0.8 no hace panic
/// y no hace falta sanitizar como el parche 069A-1).
pub(crate) fn cable(tipo: &str, data: Value) -> String {
    serde_json::to_string(&serde_json::json!({ "event": tipo, "data": data }))
        .unwrap_or_else(|_| r#"{"event":"error","data":{"code":"interno"}}"#.into())
}

// ── Handlers ─────────────────────────────────────────────────────────────
// [Partición limite-lineas] `healthz` + SSE viven en `eventos.rs`; el ciclo
// de vida de sesiones (`crear_sesion`, `sesion_actual`, `cerrar_sesion`) en
// `sesiones.rs`. Aquí quedan errores, autorización, router y arranque.

/* [109A-5 F3] El ciclo de vida de la meta por HTTP (PATCH/GET `/meta`) vive
 * en `meta.rs`: este archivo roza el límite de 500 líneas y ese par de
 * handlers, con su carga y su emisión de `agent.event`, es autocontenido. */

// ── Enrutador y arranque ─────────────────────────────────────────────────

pub(crate) fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/healthz", get(self::eventos::healthz))
        .route("/api/v1/session", post(self::sesiones::crear_sesion))
        .route("/api/v1/session/actual", get(self::sesiones::sesion_actual))
        .route("/api/v1/session/{id}", delete(self::sesiones::cerrar_sesion))
        .route("/api/v1/session/{id}/events", get(self::eventos::eventos_sse))
        .route("/api/v1/session/{id}/meta", patch(actualizar_meta).get(leer_meta))
        .route(
            "/api/v1/session/{id}/turns",
            post(self::turnos::iniciar_turno),
        )
        .route(
            "/api/v1/session/{id}/turns/{turn_id}/cancel",
            post(self::turnos::cancelar_turno),
        )
        .route(
            "/api/v1/session/{id}/approvals/{approval_id}",
            post(self::turnos::responder_aprobacion),
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
        /* [209A-1 F4-resto] × de la tab Consola sobre una entrada viva. */
        .route(
            "/api/v1/session/{id}/consolas/{eid}/matar",
            post(super::web_datos::matar_consola),
        )
        /* [219A-3] Sub-barra de la tab Consola: lista + backfill + stdin. */
        /* [219A-4] POST crea consola propia del operador ([+ Nueva]). */
        .route(
            "/api/v1/session/{id}/consolas",
            get(super::web_datos::listar_consolas).post(super::web_datos::crear_consola),
        )
        .route(
            "/api/v1/session/{id}/consolas/{eid}/salida",
            get(super::web_datos::salida_consola),
        )
        .route(
            "/api/v1/session/{id}/consolas/{eid}/escribir",
            post(super::web_datos::escribir_consola),
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
        .route(
            "/api/v1/session/{id}/workspaces/{wid}/archivar",
            post(super::web_datos::archivar_conversaciones_proyecto),
        )
        .route(
            "/api/v1/session/{id}/workspaces/{wid}/conversaciones",
            delete(super::web_datos::eliminar_conversaciones_proyecto),
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
/// `--fixture` (turnos sintéticos sin proveedor), `--secreto-sesion <texto>`
/// (fija el secreto de firma de la cookie; por defecto aleatorio por
/// arranque; también vale `GLORY_HARNESS_SESION_SECRETO`, el flag prevalece).
pub async fn run(
    puerto: u16,
    ui_dir: Option<String>,
    fixture: bool,
    secreto_fijo: Option<String>,
) -> std::process::ExitCode {
    let token = self::sesiones::token_desde_env();
    let tokenless = token.is_none();
    // [139A-8 K9] Secreto de firma: flag > env > aleatorio por arranque.
    let secreto = secreto_fijo
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::env::var(self::seguridad::ENV_SECRETO_SESION)
                .ok()
                .filter(|s| !s.trim().is_empty())
        })
        .map(|s| SecretoSesion::derivar(s.trim()))
        .unwrap_or_else(SecretoSesion::aleatorio);

    let state = Arc::new(AppState {
        token,
        sesiones: Mutex::new(HashMap::new()),
        fixture,
        secreto,
        inicios_turno: Mutex::new(VecDeque::new()),
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
    // [139A-8 K3] La decisión vive en `seguridad::direccion_escucha` (con
    // tests); aquí solo se usa.
    let addr = self::seguridad::direccion_escucha(!tokenless, puerto);
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

    match axum::serve(listener, app)
        .with_graceful_shutdown(apagado_ordenado(Arc::clone(&state)))
        .await
    {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("[glory-harness web] error del servidor: {e}");
            std::process::ExitCode::from(1)
        }
    }
}

/// [209A-1 F4-resto] Apagado ordenado: ante Ctrl+C se matan las consolas
/// vivas de TODAS las sesiones antes de soltar el listener (ningún hijo
/// debe sobrevivir al proceso).
async fn apagado_ordenado(state: Arc<AppState>) {
    if tokio::signal::ctrl_c().await.is_err() {
        return;
    }
    eprintln!("[glory-harness web] cerrando: reap de consolas…");
    let sesiones = state.sesiones.lock().await;
    for (id, sesion) in sesiones.iter() {
        let comun = sesion.comun.lock().await;
        if let Some(ejecutor) = comun.ejecutor.as_ref() {
            let matadas = ejecutor.matar_todas().await;
            if matadas > 0 {
                eprintln!("[glory-harness web] sesión {id}: {matadas} consola(s) matada(s)");
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests;
