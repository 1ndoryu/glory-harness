//! Pruebas de sesiones web: creación, autorización y expiración.

use super::*;
use axum::{body::Body, http::{header, Method, Request, StatusCode}};
use tower::ServiceExt;

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
    let vieja = SystemTime::now() - Duration::from_secs(SESION_TTL_SECS + 60);
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
                // [139A-8 K9] La cookie viaja firmada.
                .header(header::COOKIE, format!("{COOKIE_SESION}={}", state.secreto.empaquetar(&sid)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::GONE);
    assert!(!state.sesiones.lock().await.contains_key(&sid));
}

/// Reanudar por cookie devuelve la misma sesión (sin crear otra).
#[tokio::test]
async fn sesion_actual_reanuda_por_cookie() {
    let state = state_test();
    let (sid, _) = sesion_memoria(&state).await;
    let app = router(Arc::clone(&state));
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/session/actual")
                .header(header::COOKIE, format!("{COOKIE_SESION}={}", state.secreto.empaquetar(&sid)))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let cuerpo: Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 4096).await.unwrap())
            .unwrap();
    assert_eq!(cuerpo["session_id"], serde_json::json!(sid));
    assert_eq!(state.sesiones.lock().await.len(), 1);
}

/// Sin cookie de sesión, reanudar → 401 (el cliente creará con POST).
#[tokio::test]
async fn sesion_actual_sin_cookie_devuelve_401() {
    let app = router(state_test());
    let res = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/session/actual")
                .header(header::AUTHORIZATION, bearer_master())
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
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
