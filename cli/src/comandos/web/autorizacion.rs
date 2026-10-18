//! Autorización de las rutas web: credencial por cookie o Bearer y control de origen.

use axum::http::{header, HeaderMap, Method};
use std::sync::Arc;
use uuid::Uuid;

use super::errores::{error, ApiError};
use super::{antiguedad_sesion, SESION_TTL_SECS};
use super::{AppState, SesionWeb, COOKIE_SESION};

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

pub(crate) async fn credencial(headers: &HeaderMap, state: &AppState) -> Option<Credencial> {
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
    if let Some(valor) = cookie_sesion(headers) {
        // [139A-8 K9] La cookie viaja firmada (`{sid}.{firma}`); sin firma
        // válida no hay sesión, aunque el `sid` exista en el mapa.
        if let Some(sid) = state.secreto.desempaquetar(&valor) {
            if state.sesiones.lock().await.contains_key(&sid) {
                return Some(Credencial::SesionCookie(sid));
            }
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
    if antiguedad_sesion(sesion.creada) > SESION_TTL_SECS {
        sesiones.remove(&sid);
        drop(sesiones);
        super::turnos::abortar_turno_activo(&sesion, "sesión expirada").await;
        return Err(error(
            "sesion_expirada",
            "sesión expirada (24 h): crea otra",
        ));
    }
    Ok((sesion, por_cookie))
}
