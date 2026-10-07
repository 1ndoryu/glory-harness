//! [Partición limite-lineas] Ciclo de vida de sesiones del modo web local:
//! creación (`POST /api/v1/session`), reanudación por cookie
//! (`GET /api/v1/session/actual`) y cierre (`DELETE /api/v1/session/:id`).
//! Extraído de `mod.rs` (superaba 500 líneas efectivas). El router,
//! la autorización (`autorizar_sesion`), los errores (`ApiError`) y el
//! estado (`AppState`, `SesionWeb`) siguen en el padre.

use std::sync::Arc;
use std::time::SystemTime;

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::servicio::{OpcionesSesion, SesionComun};
use glory_harness_core::llm::LlavesProveedor;

use super::sse::DifusionSse;
use super::{
    antiguedad_sesion, autorizar_sesion, credencial, error, ApiError, AppState, Credencial,
    SesionWeb, COOKIE_SESION, MAX_SESIONES, SESION_TTL_SECS,
};

/// Token maestro opcional: solo crea sesiones. Nunca autoriza nada más.
/// Sin token configurado, `web` funciona en modo local tokenless; `run` lo
/// limita a loopback para que esa comodidad no exponga una API sin auth.
pub(crate) fn token_desde_env() -> Option<String> {
    match std::env::var("GLORY_HARNESS_WEB_TOKEN") {
        Ok(t) if !t.trim().is_empty() => Some(t),
        _ => None,
    }
}

/// `POST /api/v1/session` — solo token maestro; fija cookie `gh_sesion`.
pub(crate) async fn crear_sesion(
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
        creada: SystemTime::now(),
    });
    {
        let mut sesiones = state.sesiones.lock().await;
        // [069A-2 F6] Purga perezosa de expiradas + tope de vivas: el modo
        // web es single-user loopback, no un multitenant.
        sesiones.retain(|_, s| antiguedad_sesion(s.creada) <= SESION_TTL_SECS);
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
    let cookie = format!(
        "{COOKIE_SESION}={}; HttpOnly; SameSite=Lax; Path=/; Max-Age=86400",
        // [139A-8 K9] La cookie viaja firmada con el secreto de arranque.
        state.secreto.empaquetar(&session_id)
    );
    Ok((StatusCode::OK, [(header::SET_COOKIE, cookie)], cuerpo).into_response())
}

/// `GET /api/v1/session/actual` — reanuda la sesión viva de la cookie
/// `gh_sesion` sin crear una nueva (misma forma que `POST /api/v1/session`;
/// `conversacion` es `null` si la sesión está en borrador). Sin cookie de
/// sesión válida → 401 y el cliente crea una con `POST`. Cada recarga de
/// página creaba una sesión y agotaba el tope (16 con TTL 24 h); reanudar
/// evita la fuga. Solo cookie: el token maestro no reanuda (crea).
pub(crate) async fn sesion_actual(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    let sid = match credencial(&headers, &state).await {
        Some(Credencial::SesionCookie(s)) => s,
        _ => return Err(error("no_autorizado", "sin sesión activa en la cookie")),
    };
    // Reutiliza la validación completa (UUID, coincidencia, origen GET y
    // expiración/TTL con purga).
    let (sesion, _) = autorizar_sesion(&headers, &Method::GET, &state, &sid).await?;
    let comun = sesion.comun.lock().await.clone();
    let llaves = LlavesProveedor::from_env();
    let proveedores = serde_json::json!([
        { "nombre": "cerebras", "claves": llaves.cerebras.len() },
        { "nombre": "groq", "claves": llaves.groq.len() },
        { "nombre": "deepseek", "claves": llaves.deepseek.len() },
        { "nombre": "glory", "claves": llaves.glory.len() },
        { "nombre": "commandcode", "claves": llaves.commandcode.len() },
        { "nombre": "opencode-go", "claves": llaves.opencode_go.len() },
    ]);
    let actual = *sesion.conversacion_id.lock().await;
    let conversacion = match actual {
        Some(cid) => comun
            .persistencia
            .conversaciones_listar(comun.user_id)
            .map_err(|e| error("sesion", e.to_string()))?
            .into_iter()
            .find(|c| c.id == cid),
        None => None,
    };
    Ok(Json(serde_json::json!({
        "ok": true,
        "session_id": sid,
        "modelo": comun.modelo,
        "workspace": comun.workspace,
        "proveedores": proveedores,
        "conversacion": conversacion,
    }))
    .into_response())
}

/// `DELETE /api/v1/session/:id` — cancela el turno activo y cierra.
pub(crate) async fn cerrar_sesion(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = autorizar_sesion(&headers, &Method::DELETE, &state, &id).await?;
    super::turnos::abortar_turno_activo(&sesion, "sesión cerrada").await;
    /* [209A-1 F4-resto] Reap global de la sesión que se cierra: ninguna
     * consola viva debe sobrevivir a su sesión. */
    {
        let comun = sesion.comun.lock().await;
        if let Some(ejecutor) = comun.ejecutor.as_ref() {
            let matadas = ejecutor.matar_todas().await;
            if matadas > 0 {
                tracing::info!(sesion = %id, matadas, "reap de consolas al cerrar sesión");
            }
        }
    }
    state.sesiones.lock().await.remove(&id);
    Ok(Json(serde_json::json!({ "ok": true, "session_id": id })))
}
