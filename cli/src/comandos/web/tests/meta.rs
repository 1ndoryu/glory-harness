//! Pruebas de las rutas de meta web (PATCH/GET de meta y logros).

use super::*;
use axum::{body::Body, http::{header, Method, Request, StatusCode}};
use tower::ServiceExt;

#[tokio::test]
async fn meta_web_se_fija_y_se_normaliza() {
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;
    let (status, cuerpo) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "meta": "  objetivo claro  " }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], "objetivo claro");
    /* [109A-5 F1] La meta ya no vive solo en memoria: queda en la fila de la
     * conversación y es la fuente que lee el turno. */
    let conv = sesion.conversacion_id.lock().await.expect("conversación");
    let estado = sesion
        .comun
        .lock()
        .await
        .meta_leer(conv)
        .expect("lee meta");
    assert_eq!(estado.texto_activo(), Some("objetivo claro"));
    assert!(sesion.meta.lock().await.is_none());
}

#[tokio::test]
async fn meta_web_ciclo_de_vida_falla_cerrado() {
    let state = state_test();
    let (sid, _) = sesion_memoria(&state).await;

    // Pausar/reanudar/lograr sin meta activa son errores explícitos.
    for accion in ["pausar", "reanudar"] {
        let (status, cuerpo) =
            parchear_meta(&state, &sid, serde_json::json!({ "accion": accion })).await;
        assert_eq!(status, StatusCode::CONFLICT, "{accion}");
        assert_eq!(cuerpo["code"], "sin_meta_activa");
    }
    let (status, cuerpo) =
        parchear_meta(&state, &sid, serde_json::json!({ "accion": "lograr" })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(cuerpo["code"], "turno_requerido");

    // Fijar + pausar dos veces: la segunda no muta nada.
    let (status, _) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "accion": "fijar", "meta": "meta web" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) =
        parchear_meta(&state, &sid, serde_json::json!({ "accion": "pausar" })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, cuerpo) =
        parchear_meta(&state, &sid, serde_json::json!({ "accion": "pausar" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(cuerpo["code"], "meta_ya_pausada");

    // Lograr con turno válido cierra la meta y deja historial.
    let turno = Uuid::new_v4().to_string();
    let (status, cuerpo) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "accion": "lograr", "turno_id": turno }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], Value::Null);

    // Combinaciones ambiguas se rechazan en vez de interpretarse.
    let (status, cuerpo) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "accion": "pausar", "meta": "texto" }),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(cuerpo["code"], "peticion_invalida");
}

#[tokio::test]
async fn meta_web_en_borrador_no_inventa_conversacion() {
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;
    *sesion.conversacion_id.lock().await = None;

    let (status, cuerpo) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "meta": "borrador" }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], "borrador");
    assert_eq!(sesion.meta.lock().await.as_deref(), Some("borrador"));

    // Sin fila no hay reloj durable que pausar: error explícito.
    let (status, cuerpo) =
        parchear_meta(&state, &sid, serde_json::json!({ "accion": "pausar" })).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(cuerpo["code"], "sin_conversacion");
}

#[tokio::test]
async fn meta_web_rechaza_exceso_y_sesion_ajena() {
    let state = state_test();
    let (sid, _) = sesion_memoria(&state).await;
    let larga = "x".repeat(crate::servicio::meta::MAX_META_CHARS + 1);
    let (status, cuerpo) =
        parchear_meta(&state, &sid, serde_json::json!({ "meta": larga })).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(cuerpo["code"], "meta_larga");

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

/* [109A-5 F3] El panel pinta historial y badge con la respuesta del PATCH:
 * si `logro` no viajara, la UI tendría que releer y podría perder el logro
 * entre ambas llamadas. El turno lo ancla al pie del turno que lo respalda. */
#[tokio::test]
async fn meta_web_lograr_devuelve_logro_y_estado_completo() {
    let state = state_test();
    let (sid, _) = sesion_memoria(&state).await;
    let (status, _) =
        parchear_meta(&state, &sid, serde_json::json!({ "meta": "cerrar fase 3" })).await;
    assert_eq!(status, StatusCode::OK);

    let turno = Uuid::new_v4().to_string();
    let (status, cuerpo) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "accion": "lograr", "turno_id": turno }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], Value::Null);
    assert_eq!(cuerpo["logro"]["meta"], "cerrar fase 3");
    assert_eq!(cuerpo["logro"]["turno_id"], turno);
    assert!(cuerpo["logro"]["elapsed_ms"].is_u64());
    assert_eq!(cuerpo["estado"]["activa"], Value::Null);
    assert_eq!(cuerpo["estado"]["logros"][0]["turno_id"], turno);
}

/* [109A-5 F3] El badge del pie no puede depender del cuerpo HTTP: en el
 * navegador el logro viaja como `agent.event` por el SSE de la sesión, igual
 * que en la ventana Tauri. Se suscribe ANTES de lograr para no perder el
 * cable (la difusión descarta si no hay lector). */
#[tokio::test]
async fn meta_web_lograr_publica_el_evento_del_badge() {
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;
    let (status, _) =
        parchear_meta(&state, &sid, serde_json::json!({ "meta": "cerrar fase 3" })).await;
    assert_eq!(status, StatusCode::OK);

    let mut rx = sesion.sse.lock().await.suscribir().1;
    let turno = Uuid::new_v4().to_string();
    let (status, _) = parchear_meta(
        &state,
        &sid,
        serde_json::json!({ "accion": "lograr", "turno_id": turno }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let cable = rx.recv().await.expect("cable del logro");
    assert!(cable.contains(r#""event":"agent.event""#), "{cable}");
    assert!(cable.contains(r#""tipo":"meta_lograda""#), "{cable}");
    assert!(cable.contains(&turno), "{cable}");
    assert!(cable.contains(r#""elapsed_ms":"#), "{cable}");

    // Limpiar no registra logro: sin meta no hay badge que emitir.
    let (status, _) =
        parchear_meta(&state, &sid, serde_json::json!({ "accion": "limpiar" })).await;
    assert_eq!(status, StatusCode::OK);
    assert!(rx.try_recv().is_err());
}

/* [109A-5 F3] Sin conversación no hay reloj durable: el panel debe recibir
 * el borrador y `estado` nulo en vez de un estado inventado que luego no
 * pueda pausar (el comando devolvería `sin_conversacion`). */
#[tokio::test]
async fn leer_meta_web_distingue_borrador_de_estado_durable() {
    let state = state_test();
    let (sid, sesion) = sesion_memoria(&state).await;

    let (status, cuerpo) = leer_meta_http(&state, &sid).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], Value::Null);
    // Con conversación el estado VACÍO es legítimo (sin meta, sin logros):
    // no es `null`, que reservamos para el borrador sin fila durable.
    assert_eq!(cuerpo["estado"]["activa"], Value::Null);
    assert!(cuerpo["estado"]["logros"]
        .as_array()
        .is_some_and(|logros| logros.is_empty()));

    let (status, _) =
        parchear_meta(&state, &sid, serde_json::json!({ "meta": "meta viva" })).await;
    assert_eq!(status, StatusCode::OK);
    let (status, cuerpo) = leer_meta_http(&state, &sid).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["meta"], "meta viva");
    assert_eq!(cuerpo["estado"]["activa"]["texto"], "meta viva");
    assert_eq!(cuerpo["estado"]["activa"]["pausada_en"], Value::Null);
    assert!(cuerpo["estado"]["logros"]
        .as_array()
        .is_some_and(|logros| logros.is_empty()));

    *sesion.conversacion_id.lock().await = None;
    let (status, cuerpo) = leer_meta_http(&state, &sid).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(cuerpo["estado"], Value::Null);
}
