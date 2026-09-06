//! [069A-2 F3] Conversaciones, configuración, proveedores y workspace.
//!
//! Replica la semántica de Tauri (`desktop/src-tauri/src/main.rs`:
//! `conversacion_nueva`, `listar/cargar/renombrar/archivar/eliminar`):
//! crear/cargar/eliminar exigen turno inactivo (409); la conversación
//! actual es única por sesión (sin paneles en web). Sin vault: la limpieza
//! de índices del vault es responsabilidad exclusiva del desktop.

use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    Json,
};
use glory_harness_core::llm::LlavesProveedor;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use super::web::{autorizar_sesion, error, ApiError, AppState, SesionWeb};
use crate::servicio::SesionComun;
use crate::VENTANA_MINIMA;

/// Catálogo real de proveedores (los que `LlavesProveedor` conoce).
const PROVEEDORES: [&str; 5] = ["cerebras", "groq", "deepseek", "glory", "commandcode"];
/// Modos de turno (`OpcionesRun::modo`).
const MODOS: [&str; 4] = ["predeterminado", "meta", "autonomo", "plan"];
/// Niveles de razonamiento (`TurnoConfig::nivel_razonamiento`).
const RAZONAMIENTOS: [&str; 3] = ["low", "medium", "high"];
/// Default de `contexto_max_ventana` (igual que el servicio).
const VENTANA_DEFAULT: u32 = 150_000;
/// Título de conversación: 1..=200 caracteres.
const MAX_TITULO_CHARS: usize = 200;

#[derive(Debug, Deserialize)]
pub(crate) struct CrearConversacion {
    pub(crate) titulo: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParcheConversacion {
    pub(crate) titulo: Option<String>,
    pub(crate) archivada: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParcheConfig {
    pub(crate) provider: Option<String>,
    pub(crate) modelo: Option<String>,
    pub(crate) modo: Option<String>,
    pub(crate) razonamiento: Option<String>,
    pub(crate) max_ventana: Option<u32>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct CambiarWorkspace {
    pub(crate) ruta: String,
}

// ── Helpers ──────────────────────────────────────────────────────────────

/// `true` si la sesión tiene un turno en curso.
async fn turno_en_curso(sesion: &Arc<SesionWeb>) -> bool {
    sesion.turno.lock().await.is_some()
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

async fn sesion_y_comun(
    headers: &HeaderMap,
    metodo: &Method,
    state: &AppState,
    id: &str,
) -> Result<(Arc<SesionWeb>, SesionComun), ApiError> {
    let (sesion, _) = autorizar_sesion(headers, metodo, state, id).await?;
    let comun = sesion.comun.lock().await.clone();
    Ok((sesion, comun))
}

// ── Conversaciones ───────────────────────────────────────────────────────

/// `GET /api/v1/conversations` — recientes primero, incluye archivo.
pub(crate) async fn listar_conversaciones(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let lista = comun
        .persistencia
        .conversaciones_listar(comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "conversaciones": lista }),
    ))
}

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
    let nuevo_id = comun
        .persistencia
        .conversacion_crear(comun.user_id, &titulo)
        .map_err(|e| error("sesion", e.to_string()))?;
    *sesion.conversacion_id.lock().await = nuevo_id;
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
    use glory_harness_core::AgentPersistence as _;
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
    *sesion.conversacion_id.lock().await = conv.id;
    Ok(Json(serde_json::json!({
        "ok": true,
        "id": conv.id,
        "titulo": conv.titulo,
        "mensajes": mensajes,
        "acciones": acciones,
        "ultimo_uso": ultimo_uso,
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

/// `DELETE /api/v1/conversations/:cid` — elimina; si era la actual, crea
/// una vacía (409 con turno). Sin vault en web (solo desktop).
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
    let actual = *sesion.conversacion_id.lock().await;
    if actual == conv.id {
        let titulo = "Nueva conversación".to_string();
        let nuevo_id = comun
            .persistencia
            .conversacion_crear(comun.user_id, &titulo)
            .map_err(|e| error("sesion", e.to_string()))?;
        *sesion.conversacion_id.lock().await = nuevo_id;
    }
    let actual_id = *sesion.conversacion_id.lock().await;
    let actual_conv = comun
        .persistencia
        .conversaciones_listar(comun.user_id)
        .map_err(|e| error("sesion", e.to_string()))?
        .into_iter()
        .find(|c| c.id == actual_id)
        .ok_or_else(|| error("sesion", "conversación actual no encontrada"))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "actual": actual_conv }),
    ))
}

// ── Proveedores y configuración ──────────────────────────────────────────

/// `GET /api/v1/providers` — allowlist sin credenciales.
pub(crate) async fn leer_proveedores(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _ = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let llaves = LlavesProveedor::from_env();
    let conteos = [
        ("cerebras", llaves.cerebras.len()),
        ("groq", llaves.groq.len()),
        ("deepseek", llaves.deepseek.len()),
        ("glory", llaves.glory.len()),
        ("commandcode", llaves.commandcode.len()),
    ];
    let proveedores: Vec<Value> = conteos
        .into_iter()
        .map(|(nombre, claves)| serde_json::json!({ "nombre": nombre, "disponible": claves > 0 }))
        .collect();
    Ok(Json(
        serde_json::json!({ "ok": true, "proveedores": proveedores }),
    ))
}

/// `GET /api/v1/config` — solo opciones soportadas por el núcleo.
pub(crate) async fn leer_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "config": config_efectiva(&comun)? }),
    ))
}

/// `PATCH /api/v1/config` — valida contra el catálogo real y reconfigura.
pub(crate) async fn guardar_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<ParcheConfig>,
) -> Result<Json<Value>, ApiError> {
    if let Some(p) = &peticion.provider {
        if !PROVEEDORES.contains(&p.as_str()) {
            return Err(error("peticion_invalida", "proveedor no soportado"));
        }
    }
    if let Some(m) = &peticion.modelo {
        if m.trim().is_empty() || m.chars().count() > MAX_TITULO_CHARS {
            return Err(error("peticion_invalida", "modelo inválido"));
        }
    }
    if let Some(m) = &peticion.modo {
        if !MODOS.contains(&m.as_str()) {
            return Err(error("peticion_invalida", "modo no soportado"));
        }
    }
    if let Some(r) = &peticion.razonamiento {
        if !RAZONAMIENTOS.contains(&r.as_str()) {
            return Err(error("peticion_invalida", "razonamiento no soportado"));
        }
    }
    if let Some(v) = peticion.max_ventana {
        if v < VENTANA_MINIMA {
            return Err(error("peticion_invalida", "max_ventana bajo el mínimo"));
        }
    }

    let (sesion, _) = sesion_y_comun(&headers, &Method::PATCH, &state, &id).await?;
    let mut comun = sesion.comun.lock().await;
    if let Some(v) = peticion.max_ventana {
        comun
            .persistencia
            .config_guardar("contexto_max_ventana", &v.to_string())
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    comun
        .reconfigurar(
            peticion.provider,
            peticion.modelo,
            peticion.modo,
            peticion.razonamiento,
        )
        .map_err(|e| error("sesion", e.to_string()))?;
    let vista = config_efectiva(&comun)?;
    Ok(Json(serde_json::json!({ "ok": true, "config": vista })))
}

fn config_efectiva(comun: &SesionComun) -> Result<Value, ApiError> {
    let cfg = &comun.runtime.turno_config;
    let max_ventana = comun
        .persistencia
        .config_leer("contexto_max_ventana")
        .map_err(|e| error("sesion", e.to_string()))?
        .and_then(|t| t.trim().parse::<u32>().ok())
        .unwrap_or(VENTANA_DEFAULT);
    Ok(serde_json::json!({
        "provider": cfg.provider,
        "modelo": cfg.modelo,
        "modo": cfg.modo,
        "razonamiento": cfg.nivel_razonamiento,
        "max_ventana": max_ventana,
        "workspace": comun.workspace,
    }))
}

// ── Workspace ────────────────────────────────────────────────────────────

/// `GET /api/v1/workspace` — workspace efectivo (loopback; sin redacción
/// porque el único cliente es el usuario local).
pub(crate) async fn leer_workspace(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "workspace": comun.workspace }),
    ))
}

/// `POST /api/v1/workspace` — ruta absoluta validada; bloqueado con turno.
pub(crate) async fn cambiar_workspace_ep(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<CambiarWorkspace>,
) -> Result<Json<Value>, ApiError> {
    let (sesion, _) = sesion_y_comun(&headers, &Method::POST, &state, &id).await?;
    if turno_en_curso(&sesion).await {
        return Err(error("turno_activo", "hay un turno en curso"));
    }
    let ruta = PathBuf::from(peticion.ruta.trim());
    if !ruta.is_absolute() {
        return Err(error("peticion_invalida", "la ruta debe ser absoluta"));
    }
    if !ruta.is_dir() {
        return Err(error(
            "no_encontrado",
            "la ruta no existe o no es un directorio",
        ));
    }
    let mut comun = sesion.comun.lock().await;
    comun
        .cambiar_workspace(ruta)
        .map_err(|e| error("sesion", e.to_string()))?;
    Ok(Json(
        serde_json::json!({ "ok": true, "workspace": comun.workspace }),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::web::tests::{sesion_memoria, state_test};
    use super::*;
    use axum::{
        body::Body,
        http::{header, Request},
    };
    use tower::ServiceExt;

    const COOKIE: &str = super::super::web::COOKIE_SESION;

    fn peticion(metodo: Method, uri: String, sid: &str, cuerpo: Option<String>) -> Request<Body> {
        let mut b = Request::builder()
            .method(metodo)
            .uri(uri)
            .header(header::COOKIE, format!("{COOKIE}={sid}"));
        if let Some(c) = cuerpo {
            b = b.header(header::CONTENT_TYPE, "application/json");
            b.body(Body::from(c)).unwrap()
        } else {
            b.body(Body::empty()).unwrap()
        }
    }

    async fn cuerpo(res: axum::response::Response) -> Value {
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 65536).await.unwrap())
            .unwrap()
    }

    /// Ciclo CRUD: crear → listar → cargar → renombrar → archivar → eliminar.
    #[tokio::test]
    async fn conversaciones_ciclo_completo() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let base = format!("/api/v1/session/{sid}");

        // Crear (pasa a ser la actual).
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::POST,
                format!("{base}/conversations"),
                &sid,
                Some(r#"{"titulo":"Prueba web"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let creada = cuerpo(res).await;
        assert_eq!(creada["conversacion"]["titulo"], "Prueba web");
        let cid = creada["conversacion"]["id"].as_str().unwrap().to_string();

        // Listar la contiene.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::GET,
                format!("{base}/conversations"),
                &sid,
                None,
            ))
            .await
            .unwrap();
        let lista = cuerpo(res).await;
        assert!(lista["conversaciones"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["id"] == cid));

        // Cargar: historial vacío + acciones vacías.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::GET,
                format!("{base}/conversations/{cid}/messages"),
                &sid,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let carga = cuerpo(res).await;
        assert_eq!(carga["mensajes"], serde_json::json!([]));
        assert_eq!(carga["ultimo_uso"], Value::Null);

        // Renombrar + archivar en un PATCH.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::PATCH,
                format!("{base}/conversations/{cid}"),
                &sid,
                Some(r#"{"titulo":"Renombrada","archivada":true}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let parche = cuerpo(res).await;
        assert_eq!(parche["conversacion"]["titulo"], "Renombrada");
        assert_eq!(parche["conversacion"]["archivada"], true);

        // Eliminar la actual → crea una vacía y la devuelve.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::DELETE,
                format!("{base}/conversations/{cid}"),
                &sid,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let borrada = cuerpo(res).await;
        assert_ne!(borrada["actual"]["id"], cid);
    }

    #[tokio::test]
    async fn conversacion_ajena_devuelve_404() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let res = app
            .oneshot(peticion(
                Method::GET,
                format!("/api/v1/session/{sid}/conversations/00000000-0000-0000-0000-000000000000/messages"),
                &sid,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn config_get_y_patch_validado() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let base = format!("/api/v1/session/{sid}");

        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(Method::GET, format!("{base}/config"), &sid, None))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let cfg = cuerpo(res).await;
        assert_eq!(cfg["config"]["modo"], "predeterminado");

        // Proveedor inexistente → 400.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::PATCH,
                format!("{base}/config"),
                &sid,
                Some(r#"{"provider":"inexistente"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);

        // Modo + razonamiento + ventana válidos → 200 y efectivos.
        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::PATCH,
                format!("{base}/config"),
                &sid,
                Some(r#"{"modo":"meta","razonamiento":"low","max_ventana":20000}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let cfg2 = cuerpo(res).await;
        assert_eq!(cfg2["config"]["modo"], "meta");
        assert_eq!(cfg2["config"]["razonamiento"], "low");
        assert_eq!(cfg2["config"]["max_ventana"], 20000);

        // Ventana bajo el mínimo → 400.
        let app = super::super::web::router(state);
        let res = app
            .oneshot(peticion(
                Method::PATCH,
                format!("{base}/config"),
                &sid,
                Some(r#"{"max_ventana":100}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn providers_sin_credenciales() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let app = super::super::web::router(state);
        let res = app
            .oneshot(peticion(
                Method::GET,
                format!("/api/v1/session/{sid}/providers"),
                &sid,
                None,
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body = cuerpo(res).await;
        let nombres: Vec<&str> = body["proveedores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["nombre"].as_str().unwrap())
            .collect();
        assert_eq!(
            nombres,
            vec!["cerebras", "groq", "deepseek", "glory", "commandcode"]
        );
        assert!(body.to_string().find("sk-").is_none());
    }

    #[tokio::test]
    async fn workspace_relativo_e_inexistente_fallan() {
        let state = state_test();
        let (sid, _) = sesion_memoria(&state).await;
        let base = format!("/api/v1/session/{sid}");

        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::POST,
                format!("{base}/workspace"),
                &sid,
                Some(r#"{"ruta":"relativa/no"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::BAD_REQUEST);

        let app = super::super::web::router(Arc::clone(&state));
        let res = app
            .oneshot(peticion(
                Method::POST,
                format!("{base}/workspace"),
                &sid,
                Some(r#"{"ruta":"C:\\no-existe-gh-test\\x"}"#.into()),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::NOT_FOUND);

        // Ruta real (temp del sistema): cambia y persiste.
        let real = std::env::temp_dir();
        let app = super::super::web::router(state);
        let res = app
            .oneshot(peticion(
                Method::POST,
                format!("{base}/workspace"),
                &sid,
                Some(format!(
                    r#"{{"ruta":{}}}"#,
                    serde_json::to_string(&real).unwrap()
                )),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), axum::http::StatusCode::OK);
        let body = cuerpo(res).await;
        // El harness puede normalizar con separador final: comparar sin él.
        let devuelto = body["workspace"]
            .as_str()
            .unwrap()
            .trim_end_matches(['\\', '/']);
        let esperado = real
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_string();
        assert_eq!(devuelto, esperado);
    }

    /// Dos handles sobre el mismo fichero (WAL + busy_timeout): el modo web
    /// y Tauri/CLI comparten la SQLite sin "database is locked".
    #[tokio::test]
    async fn sqlite_compartida_entre_dos_handles() {
        let ruta = std::env::temp_dir().join(format!("gh-web-test-{}.db", Uuid::new_v4()));
        let a = crate::PersistenciaSqlite::abrir(&ruta).expect("abrir A");
        let b = crate::PersistenciaSqlite::abrir(&ruta).expect("abrir B");
        let uid = Uuid::new_v4();
        let id_a = a.conversacion_crear(uid, "desde A").expect("crear A");
        let lista_b = b.conversaciones_listar(uid).expect("listar B");
        assert!(lista_b.iter().any(|c| c.id == id_a));
        let id_b = b.conversacion_crear(uid, "desde B").expect("crear B");
        let lista_a = a.conversaciones_listar(uid).expect("listar A");
        assert!(lista_a.iter().any(|c| c.id == id_b));
        drop(a);
        drop(b);
        let _ = std::fs::remove_file(&ruta);
        let _ = std::fs::remove_file(ruta.with_extension("db-wal"));
        let _ = std::fs::remove_file(ruta.with_extension("db-shm"));
    }
}
