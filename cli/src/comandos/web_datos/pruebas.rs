//! [079A-1 F3] Tests HTTP del hub de datos (partidos de web_datos.rs).

use super::super::web::tests::{sesion_memoria, state_test};
use axum::{
    body::Body,
    http::{header, Request},
};
use tower::ServiceExt;

use std::sync::Arc;

use axum::http::Method;
use serde_json::Value;
use uuid::Uuid;

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
    serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 65536).await.unwrap()).unwrap()
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
            format!(
                "/api/v1/session/{sid}/conversations/00000000-0000-0000-0000-000000000000/messages"
            ),
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
