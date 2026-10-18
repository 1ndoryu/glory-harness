//! Pruebas de consolas web: matar consola y su idempotencia.

use super::*;
use axum::{body::Body, http::{header, Method, Request, StatusCode}};
use tower::ServiceExt;

/// [209A-1 F4-resto] Matar una consola inexistente es idempotente:
/// 200 con `matada: false` (sin error: la vista ya la marca como
/// terminada al llegar `consola_fin`).
#[tokio::test]
async fn matar_consola_desconocida_devuelve_matada_false() {
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;
    sesion.comun.lock().await.ejecutor =
        Some(Arc::new(crate::ejecutor::EjecutorCliente::nuevo()));
    let app = router(Arc::clone(&state));
    let res = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/session/{sid}/consolas/no-existe/matar"))
                .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let cuerpo: Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
            .unwrap();
    assert_eq!(cuerpo, serde_json::json!({ "ok": true, "matada": false }));
}

/// [209A-1 F4-resto] La × de la tab sobre una viva la mata de verdad:
/// 200 con `matada: true` y el proceso deja de estar vivo (el pump lo
/// reapea al salir).
#[tokio::test]
async fn matar_consola_viva_la_mata() {
    use glory_harness_core::EjecutorComando;
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;
    let ejecutor = Arc::new(crate::ejecutor::EjecutorCliente::nuevo());
    let id = format!("test-matar-{}", Uuid::new_v4());
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let comando = if cfg!(windows) {
        "ping -n 61 127.0.0.1".to_string()
    } else {
        "sleep 60".to_string()
    };
    ejecutor
        .ejecutar_en_vivo(&id, &comando, Uuid::nil(), true, tx)
        .await
        .expect("arranca la viva");
    sesion.comun.lock().await.ejecutor = Some(Arc::clone(&ejecutor));
    let app = router(Arc::clone(&state));
    let res = app
        .oneshot(
            Request::builder()
                .method(Method::POST)
                .uri(format!("/api/v1/session/{sid}/consolas/{id}/matar"))
                .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let cuerpo: Value =
        serde_json::from_slice(&axum::body::to_bytes(res.into_body(), 1024).await.unwrap())
            .unwrap();
    assert_eq!(cuerpo, serde_json::json!({ "ok": true, "matada": true }));
    let mut viva = true;
    for _ in 0..20 {
        let infos = ejecutor.lista().await.expect("lista");
        if infos.iter().all(|i| i.id_ejecucion != id || !i.viva) {
            viva = false;
            break;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    assert!(!viva, "la consola debió morir tras matarla");
}
