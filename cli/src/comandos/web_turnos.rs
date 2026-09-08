//! [069A-2 F2] Turnos, cancelación y aprobaciones del modo web.
//!
//! Replica el ciclo de vida de Tauri (`desktop/src-tauri/src/main.rs`,
//! `enviar_turno`): `preparar_turno` → `ejecutar_turno` con canal mpsc →
//! reenvío a difusión SSE → `turn.finished` obligatorio. Un turno activo
//! por sesión (409 ante segundo inicio); cancelar aborta la tarea y marca
//! `cancelado` en BD; aprobar es idempotente (duplicada no ejecuta dos
//! veces). En modo `--fixture` el turno es sintético (sin proveedor).

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    Json,
};
use glory_harness_core::aprobacion::RespuestaAprobacion;
use glory_harness_core::evento::AgenteEvento;
use serde::Deserialize;
use serde_json::Value;
use tokio::sync::mpsc;
use uuid::Uuid;

use super::web::{autorizar_sesion, cable, error, ApiError, AppState, SesionWeb, TurnoActivo};

/// Límite de mensaje (F6 fijará rate limits; el tamaño se valida desde F2).
const MAX_MENSAJE_CHARS: usize = 32_000;

#[derive(Debug, Deserialize)]
pub(crate) struct CrearTurno {
    pub(crate) message: String,
    pub(crate) conversacion_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ResponderAprobacion {
    pub(crate) approved: bool,
    pub(crate) siempre: Option<bool>,
}

/// Aborta el turno activo (si lo hay) y lo marca `cancelado` en BD para que
/// no quede como pendiente eternamente. Emite `turn.finished` salvo que se
/// indique lo contrario (el cierre de sesión ya responde al cliente).
pub(crate) async fn abortar_turno_activo(sesion: &Arc<SesionWeb>, motivo: &str) {
    let activo = sesion.turno.lock().await.take();
    if let Some(t) = activo {
        t.handle.abort();
        let comun = sesion.comun.lock().await.clone();
        let _ = comun.cancelar_turno(t.id).await;
        sesion
            .emitir(cable(
                "turn.finished",
                serde_json::json!({ "turn_id": t.id, "ok": false, "error": motivo }),
            ))
            .await;
    }
}

/// `POST /api/v1/session/:id/turns` → `{ok, turn_id}`.
pub(crate) async fn iniciar_turno(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<CrearTurno>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::POST, &state, &id).await?;
    let mensaje = peticion.message.trim().to_string();
    if mensaje.is_empty() {
        return Err(error("peticion_invalida", "mensaje vacío"));
    }
    if mensaje.chars().count() > MAX_MENSAJE_CHARS {
        return Err(error(
            "mensaje_largo",
            "mensaje demasiado largo (máx. 32000 caracteres)",
        ));
    }

    // Conversación: la indicada (con ownership) o la actual de la sesión.
    // `SesionComun` es `Clone`: se clona bajo el lock y se usa sin retenerlo.
    let comun = sesion.comun.lock().await.clone();
    let conv_id = match peticion.conversacion_id {
        Some(c) => {
            let propias = comun
                .persistencia
                .conversaciones_listar(comun.user_id)
                .map_err(|e| error("sesion", e.to_string()))?;
            if propias.iter().any(|c0| c0.id == c) {
                c
            } else {
                return Err(error("no_encontrado", "conversación no encontrada"));
            }
        }
        /* [069A-7] Create-on-write: el front SIEMPRE crea la conversación
         * (POST /conversations) antes de enviar el primer mensaje. Si no hay
         * conversación anclada en la sesión y no se indica una, es un error
         * de contrato claro, no una auto-creación implícita. */
        None => match *sesion.conversacion_id.lock().await {
            Some(actual) => actual,
            None => {
                return Err(error(
                    "sin_conversacion",
                    "no hay conversación: crea una antes de enviar",
                ))
            }
        },
    };

    {
        let turno = sesion.turno.lock().await;
        if turno.is_some() {
            return Err(error("turno_activo", "ya hay un turno en curso"));
        }
    }

    let meta = sesion.meta.lock().await.clone();
    let preparacion = comun
        .preparar_turno(conv_id, mensaje.clone(), meta)
        .await
        .map_err(|e| error("turno", e.to_string()))?;
    let turno_id = preparacion.turno_id;
    let fixture = state.fixture;

    sesion
        .emitir(cable(
            "turn.started",
            serde_json::json!({ "turn_id": turno_id }),
        ))
        .await;

    let sesion2 = Arc::clone(&sesion);
    let user_id = comun.user_id;
    let persistencia = Arc::clone(&comun.persistencia);
    let mensaje_fixture = preparacion.mensaje_efectivo.clone();
    let handle = tokio::spawn(async move {
        if fixture {
            turno_fixture(&sesion2, turno_id, &mensaje_fixture).await;
        } else {
            turno_real(&sesion2, preparacion, user_id, persistencia).await;
        }
        // Liberar el guard solo si seguimos siendo el turno activo
        // (un cancel/cierre posterior ya lo limpió y emitió su finished).
        let mut g = sesion2.turno.lock().await;
        if g.as_ref().is_some_and(|t| t.id == turno_id) {
            *g = None;
        }
    });

    sesion.turno.lock().await.replace(TurnoActivo {
        id: turno_id,
        handle,
    });

    Ok(Json(serde_json::json!({ "ok": true, "turn_id": turno_id })))
}

/// `POST /api/v1/session/:id/turns/:turn_id/cancel` — idempotente.
pub(crate) async fn cancelar_turno(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, turn_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::POST, &state, &id).await?;
    let tid =
        Uuid::parse_str(&turn_id).map_err(|_| error("peticion_invalida", "turn_id malformado"))?;

    let activo = sesion.turno.lock().await.as_ref().map(|t| t.id);
    match activo {
        None => Ok(Json(serde_json::json!({ "ok": true, "cancelado": false }))),
        Some(a) if a != tid => Err(error(
            "peticion_invalida",
            "el turno indicado no está activo",
        )),
        Some(_) => {
            abortar_turno_activo(&sesion, "cancelado por el usuario").await;
            Ok(Json(serde_json::json!({ "ok": true, "cancelado": true })))
        }
    }
}

/// `POST /api/v1/session/:id/approvals/:approval_id` — idempotente: una
/// aprobación tardía o duplicada responde `duplicada:true` sin ejecutar
/// la tool dos veces.
pub(crate) async fn responder_aprobacion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path((id, approval_id)): Path<(String, String)>,
    Json(peticion): Json<ResponderAprobacion>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::POST, &state, &id).await?;

    let runtime = sesion.comun.lock().await.runtime.clone();
    let pendiente = runtime
        .peticiones_aprobacion_pendientes()
        .iter()
        .any(|p| p.id == approval_id);
    if !pendiente {
        return Ok(Json(serde_json::json!({ "ok": true, "duplicada": true })));
    }

    let respuesta = match (peticion.approved, peticion.siempre) {
        (true, Some(true)) => RespuestaAprobacion::Siempre,
        (true, _) => RespuestaAprobacion::Aprobar,
        (false, _) => RespuestaAprobacion::Rechazar,
    };
    runtime
        .responder_aprobacion(&approval_id, respuesta)
        .map_err(|e| error("turno", e))?;
    Ok(Json(serde_json::json!({ "ok": true })))
}

// ── Ejecución ────────────────────────────────────────────────────────────

/// Turno sintético (`--fixture`): eco + `Done` sin proveedor. Comprueba el
/// ciclo de vida (started → eventos → finished) y los caminos de error.
async fn turno_fixture(sesion: &Arc<SesionWeb>, turno_id: Uuid, mensaje: &str) {
    let eventos = vec![
        AgenteEvento::Token {
            texto: format!("[fixture] eco: {mensaje}"),
        },
        AgenteEvento::Usage {
            tokens_prompt: 0,
            tokens_complecion: 0,
            ocupacion_pct: None,
            provider: None,
            modelo: None,
        },
        AgenteEvento::Done { turno_id },
    ];
    for ev in eventos {
        if matches!(ev, AgenteEvento::Done { .. }) {
            break;
        }
        sesion
            .emitir(cable(
                "agent.event",
                serde_json::to_value(&ev).unwrap_or(Value::Null),
            ))
            .await;
    }
    sesion
        .emitir(cable(
            "turn.finished",
            serde_json::json!({ "turn_id": turno_id, "ok": true, "error": null }),
        ))
        .await;
}

/// Turno real: mismo patrón que Tauri (`enviar_turno`).
async fn turno_real(
    sesion: &Arc<SesionWeb>,
    preparacion: crate::servicio::PreparacionTurno,
    user_id: Uuid,
    persistencia: Arc<crate::PersistenciaSqlite>,
) {
    let turno_id = preparacion.turno_id;
    let (tx_ev, mut rx_ev) = mpsc::channel::<AgenteEvento>(64);

    let sesion_fw = Arc::clone(sesion);
    let reenvio = tokio::spawn(async move {
        let mut uso_p = 0u32;
        let mut uso_c = 0u32;
        let mut prov: Option<String> = None;
        let mut mod_: Option<String> = None;
        while let Some(ev) = rx_ev.recv().await {
            let es_done = matches!(ev, AgenteEvento::Done { .. });
            if let AgenteEvento::Usage {
                tokens_prompt,
                tokens_complecion,
                provider,
                modelo,
                ..
            } = &ev
            {
                uso_p = uso_p.saturating_add(*tokens_prompt);
                uso_c = uso_c.saturating_add(*tokens_complecion);
                if prov.is_none() {
                    prov = provider.clone();
                }
                if mod_.is_none() {
                    mod_ = modelo.clone();
                }
            }
            sesion_fw
                .emitir(cable(
                    "agent.event",
                    serde_json::to_value(&ev).unwrap_or(Value::Null),
                ))
                .await;
            if es_done {
                break;
            }
        }
        (uso_p, uso_c, prov, mod_)
    });

    let resultado = preparacion
        .runtime
        .ejecutar_turno(
            user_id,
            turno_id,
            preparacion.conversacion_id,
            preparacion.historial,
            preparacion.mensaje_efectivo,
            &tx_ev,
        )
        .await;
    drop(tx_ev);
    let (uso_p, uso_c, prov, mod_) = reenvio.await.unwrap_or((0, 0, None, None));

    match resultado {
        Ok(()) => {
            // Persistir el uso REAL (best-effort, como Tauri).
            if uso_p > 0 || uso_c > 0 || prov.is_some() {
                let _ = persistencia.turno_actualizar_uso(
                    turno_id,
                    uso_p,
                    uso_c,
                    prov.as_deref(),
                    mod_.as_deref(),
                );
            }
            sesion
                .emitir(cable(
                    "turn.finished",
                    serde_json::json!({ "turn_id": turno_id, "ok": true, "error": null }),
                ))
                .await;
        }
        Err(e) => {
            sesion
                .emitir(cable(
                    "agent.event",
                    serde_json::to_value(&AgenteEvento::Error {
                        mensaje: e.to_string(),
                        retryable: true,
                    })
                    .unwrap_or(Value::Null),
                ))
                .await;
            sesion
                .emitir(cable(
                    "turn.finished",
                    serde_json::json!({ "turn_id": turno_id, "ok": false, "error": e.to_string() }),
                ))
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::web::tests::{sesion_memoria, state_test};
    use super::super::web::COOKIE_SESION;
    use super::*;
    use axum::{
        body::Body,
        http::{header, Request},
    };
    use tokio::time::{timeout, Duration};
    use tower::ServiceExt;

    fn post_turno(sid: &str, cookie: bool, cuerpo: &str) -> Request<Body> {
        let mut b = Request::builder()
            .method(Method::POST)
            .uri(format!("/api/v1/session/{sid}/turns"))
            .header(header::CONTENT_TYPE, "application/json");
        if cookie {
            b = b.header(header::COOKIE, format!("{}={sid}", COOKIE_SESION));
        } else {
            b = b.header(header::AUTHORIZATION, format!("Bearer {sid}"));
        }
        b.body(Body::from(cuerpo.to_string())).unwrap()
    }

    /// Espera el `turn.finished` en la difusión SSE (falla si tarda >10 s).
    async fn esperar_finished(rx: &mut tokio::sync::mpsc::Receiver<String>) -> Value {
        timeout(Duration::from_secs(10), async {
            loop {
                let en_cable = rx.recv().await.expect("canal abierto");
                let v: Value = serde_json::from_str(&en_cable).expect("cable json");
                if v["event"] == "turn.finished" {
                    return v["data"].clone();
                }
            }
        })
        .await
        .expect("turn.finished a tiempo")
    }

    #[tokio::test]
    async fn turno_fixture_completa_ciclo() {
        let state = state_test();
        let (sid, sesion) = sesion_memoria(&state).await;
        let mut rx = sesion.sse.lock().await.suscribir().1;
        let app = super::super::web::router(Arc::clone(&state));

        let res = app
            .oneshot(post_turno(&sid, true, r#"{"message":"hola"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 4096).await.unwrap())
                .unwrap();
        assert_eq!(body["ok"], true);

        let fin = esperar_finished(&mut rx).await;
        assert_eq!(fin["ok"], true);
        assert_eq!(fin["turn_id"], body["turn_id"]);

        // El guard se liberó: un segundo turno arranca sin 409.
        let app2 = super::super::web::router(state);
        let res2 = app2
            .oneshot(post_turno(&sid, true, r#"{"message":"otro"}"#))
            .await
            .unwrap();
        assert_eq!(res2.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn segundo_turno_devuelve_409() {
        let state = state_test();
        let (sid, sesion) = sesion_memoria(&state).await;
        sesion.turno.lock().await.replace(TurnoActivo {
            id: Uuid::new_v4(),
            handle: tokio::spawn(std::future::pending::<()>()),
        });
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(post_turno(&sid, false, r#"{"message":"hola"}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::CONFLICT);
        // Limpieza: abortar el pendiente insertado.
        let pendiente = {
            let mut g = sesion.turno.lock().await;
            g.take()
        };
        if let Some(t) = pendiente {
            t.handle.abort();
        }
    }

    #[tokio::test]
    async fn mensaje_vacio_devuelve_400() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let res = app
            .oneshot(post_turno(&sid, true, r#"{"message":"   "}"#))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn cancelar_sin_turno_es_idempotente() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let tid = Uuid::new_v4();
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/v1/session/{sid}/turns/{tid}/cancel"))
                    .header(header::COOKIE, format!("{}={sid}", COOKIE_SESION))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body, serde_json::json!({ "ok": true, "cancelado": false }));
    }

    #[tokio::test]
    async fn aprobacion_desconocida_es_duplicada() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/v1/session/{sid}/approvals/no-pendiente"))
                    .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"approved":true}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body: Value =
            serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
                .unwrap();
        assert_eq!(body, serde_json::json!({ "ok": true, "duplicada": true }));
    }

    #[tokio::test]
    async fn sse_con_cookie_devuelve_200() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let res = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/v1/session/{sid}/events"))
                    .header(header::COOKIE, format!("{}={sid}", COOKIE_SESION))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn mutacion_con_cookie_y_origen_ajeno_devuelve_403() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let mut req = post_turno(&sid, true, r#"{"message":"hola"}"#);
        req.headers_mut().insert(
            header::HOST,
            header::HeaderValue::from_static("127.0.0.1:8799"),
        );
        req.headers_mut().insert(
            header::ORIGIN,
            header::HeaderValue::from_static("https://evil.example"),
        );
        let res = app.oneshot(req).await.unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::FORBIDDEN);
    }

    /// Paridad §13 a nivel socket: servidor real en puerto efímero, cliente
    /// HTTP con cookie (como `EventSource` de navegador): health → turns →
    /// SSE con campo `event:` → `turn.finished`.
    #[tokio::test]
    async fn paridad_socket_turno_fixture() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind efímero");
        let addr = listener.local_addr().expect("addr local");
        let server = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        let base = format!("http://{addr}");
        let cliente = reqwest::Client::new();
        let cookie = format!("{}={sid}", COOKIE_SESION);

        let health: Value = cliente
            .get(format!("{base}/healthz"))
            .send()
            .await
            .expect("healthz")
            .json()
            .await
            .expect("json healthz");
        assert_eq!(health["ok"], true);

        // Sin cookie ni Bearer → 401 (el navegador sin sesión no entra).
        let sin_auth = cliente
            .post(format!("{base}/api/v1/session/{sid}/turns"))
            .header("Content-Type", "application/json")
            .body(r#"{"message":"hola"}"#)
            .send()
            .await
            .expect("turns sin auth");
        assert_eq!(sin_auth.status(), reqwest::StatusCode::UNAUTHORIZED);

        // Con cookie: el turno arranca (se publica DESPUÉS de abrir el SSE,
        // porque la difusión no hace replay para suscriptores tardíos).
        let mut resp = cliente
            .get(format!("{base}/api/v1/session/{sid}/events"))
            .header("Cookie", &cookie)
            .send()
            .await
            .expect("sse");
        assert_eq!(resp.status(), reqwest::StatusCode::OK);
        let mut buf = Vec::<u8>::new();
        // Primer frame: snapshot `ready` con el campo `event:`.
        let (tipo_ready, data_ready) = timeout(Duration::from_secs(10), async {
            loop {
                let chunk = resp.chunk().await.expect("chunk sse").expect("abierto");
                buf.extend(chunk.iter().filter(|b| **b != b'\r'));
                if let Some(pos) = doble_salto(&buf) {
                    let frame = String::from_utf8_lossy(&buf[..pos]).to_string();
                    buf.drain(..pos + 2);
                    return frame_sse(&frame);
                }
            }
        })
        .await
        .expect("ready a tiempo");
        assert_eq!(tipo_ready, "ready");
        let estado: Value = serde_json::from_str(&data_ready).expect("ready json");
        assert_eq!(estado["turno_activo"], Value::Null);

        let turno: Value = cliente
            .post(format!("{base}/api/v1/session/{sid}/turns"))
            .header("Cookie", &cookie)
            .header("Content-Type", "application/json")
            .body(r#"{"message":"hola socket"}"#)
            .send()
            .await
            .expect("turns con cookie")
            .json()
            .await
            .expect("json turno");
        assert_eq!(turno["ok"], true);

        // Frames hasta `turn.finished` con su `turn_id`.
        timeout(Duration::from_secs(15), async {
            loop {
                let chunk = resp.chunk().await.expect("chunk sse").expect("abierto");
                buf.extend(chunk.iter().filter(|b| **b != b'\r'));
                while let Some(pos) = doble_salto(&buf) {
                    let frame = String::from_utf8_lossy(&buf[..pos]).to_string();
                    buf.drain(..pos + 2);
                    let (tipo, data) = frame_sse(&frame);
                    if tipo == "turn.finished" {
                        let d: Value = serde_json::from_str(&data).expect("data json");
                        assert_eq!(d["ok"], true);
                        assert_eq!(d["turn_id"], turno["turn_id"]);
                        return;
                    }
                }
            }
        })
        .await
        .expect("finished a tiempo");
        server.abort();
    }

    /// Posición del `\n\n` que cierra un frame SSE (el `\r` se filtra al
    /// acumular, así los índices son exactos).
    fn doble_salto(buf: &[u8]) -> Option<usize> {
        buf.windows(2).position(|w| w == [b'\n', b'\n'])
    }

    /// `(event, data)` de un frame SSE.
    fn frame_sse(frame: &str) -> (String, String) {
        let mut tipo = "message".to_string();
        let mut datos = Vec::new();
        for linea in frame.lines() {
            if let Some(e) = linea.strip_prefix("event:") {
                tipo = e.trim().to_string();
            } else if let Some(d) = linea.strip_prefix("data:") {
                datos.push(d.trim().to_string());
            }
        }
        (tipo, datos.join("\n"))
    }
}
