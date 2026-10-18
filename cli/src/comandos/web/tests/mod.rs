//! Pruebas del módulo web. Los helpers son `pub(crate)`: también los usan `turnos.rs` y `web_datos/pruebas.rs`.

use super::*;
use axum::{body::Body, http::{header, Method, Request, StatusCode}};
use tower::ServiceExt;

mod consola;
mod meta;
mod sesiones;

/// Estado de prueba con token maestro conocido (sin fixture).
pub(crate) fn state_test() -> Arc<AppState> {
    Arc::new(AppState {
        token: Some("test-token".into()),
        sesiones: Mutex::new(HashMap::new()),
        fixture: true,
        secreto: SecretoSesion::derivar("test"),
        inicios_turno: Mutex::new(VecDeque::new()),
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
        creada: SystemTime::now(),
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
        secreto: SecretoSesion::derivar("test"),
        inicios_turno: Mutex::new(VecDeque::new()),
    })
}

/// Envía un PATCH de meta y devuelve (status, cuerpo). `pub(crate)` para
/// que los tests del cierre de turno ([109A-5 F4], en `turnos.rs`) puedan
/// fijar la meta que ese camino pausa.
pub(crate) async fn parchear_meta(
    state: &Arc<AppState>,
    sid: &str,
    cuerpo: Value,
) -> (StatusCode, Value) {
    let app = router(Arc::clone(state));
    let res = app
        .oneshot(
            Request::builder()
                .method(Method::PATCH)
                .uri(format!("/api/v1/session/{sid}/meta"))
                .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(cuerpo.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

/// Lee el estado de meta por HTTP y devuelve (status, cuerpo).
async fn leer_meta_http(state: &Arc<AppState>, sid: &str) -> (StatusCode, Value) {
    let app = router(Arc::clone(state));
    let res = app
        .oneshot(
            Request::builder()
                .method(Method::GET)
                .uri(format!("/api/v1/session/{sid}/meta"))
                .header(header::AUTHORIZATION, format!("Bearer {sid}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = axum::body::to_bytes(res.into_body(), 4096).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

