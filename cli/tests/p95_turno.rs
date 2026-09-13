//! Medidor p95 del bootstrap de turno (139A-8 F2 — R1/R2/R3; Decisión §2).
//!
//! Fixture: conversación con 5k mensajes + punto de compactación que deja
//! ~100 posteriores; N=2 sesiones concurrentes (`multi_thread`, 4 workers),
//! 50 iteraciones de `preparar_turno` (bootstrap + `listar_mensajes` +
//! escrituras del turno). Reporta p50/p95 con `std::time` + histograma manual
//! (ordenar + percentil); sin infra nueva.
//!
//! Flujo: `P95_BASELINE_MS = None` → modo recogida (imprime y pasa). Tras
//! medir el baseline en la rama sin cambios se fija `Some(ms)` y el test
//! exige `p95 <= 0,5 × baseline` (aceptación de la Decisión §2).

use std::time::Instant;

use chrono::{SecondsFormat, Utc};
use uuid::Uuid;

use glory_harness::servicio::sesion::{OpcionesSesion, SesionComun};
use glory_harness::PersistenciaSqlite;
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::AgentPersistence;

/// Baseline medido ANTES del cambio (13-09, debug, en memoria):
/// p50=61ms p95=88ms. `Some(ms)` = modo verificación (`p95 <= 0,5 × ms`).
const P95_BASELINE_MS: Option<f64> = Some(88.0);

const N_MENSAJES: usize = 5000;
const N_RECIENTES: usize = 100;
const N_TAREAS: usize = 2;
const ITERS_POR_TAREA: usize = 25;

fn percentil(mut muestras: Vec<f64>, p: f64) -> f64 {
    muestras.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let i = ((p * muestras.len() as f64).ceil() as usize).saturating_sub(1);
    muestras[i.min(muestras.len().saturating_sub(1))]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn p95_bootstrap_turno_5k() {
    let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
    let opciones = OpcionesSesion {
        nueva_conversacion: true,
        ..OpcionesSesion::default()
    };
    let (sesion, _) =
        SesionComun::abrir_con_persistencia(opciones, persistencia, None).expect("abrir sesión");
    let user = sesion.user_id;
    let conv = sesion
        .persistencia
        .conversacion_crear(user, "Nueva conversación")
        .expect("crear conversación");

    // 4900 antiguos + 100 recientes; la marca deja ~100 posteriores (`>=`).
    let antiguos_en = Utc::now() - chrono::Duration::hours(2);
    let relleno = "x".repeat(180);
    for i in 0..N_MENSAJES {
        let reciente = i >= N_MENSAJES - N_RECIENTES;
        sesion
            .persistencia
            .guardar_mensaje(&MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: if i % 2 == 0 { "user" } else { "assistant" }.into(),
                contenido: format!("mensaje de relleno {i} {relleno}"),
                creado_en: if reciente { Utc::now() } else { antiguos_en },
            })
            .await
            .expect("insertar mensaje");
    }
    let marca = (Utc::now() - chrono::Duration::minutes(30))
        .to_rfc3339_opts(SecondsFormat::Secs, true);
    assert!(
        sesion
            .persistencia
            .conversacion_compactar(user, conv, &marca, "resumen del tramo antiguo")
            .expect("marcar compactación"),
        "la conversación debe existir"
    );

    let mut asas = Vec::with_capacity(N_TAREAS);
    for t in 0..N_TAREAS {
        let s = sesion.clone();
        asas.push(tokio::spawn(async move {
            let mut tiempos = Vec::with_capacity(ITERS_POR_TAREA);
            for i in 0..ITERS_POR_TAREA {
                let t0 = Instant::now();
                s.preparar_turno(conv, format!("ping t{t} i{i}"), None)
                    .await
                    .expect("preparar turno");
                tiempos.push(t0.elapsed().as_secs_f64() * 1000.0);
            }
            tiempos
        }));
    }
    let mut muestras = Vec::with_capacity(N_TAREAS * ITERS_POR_TAREA);
    for a in asas {
        muestras.extend(a.await.expect("tarea"));
    }
    assert_eq!(muestras.len(), N_TAREAS * ITERS_POR_TAREA);

    let p50 = percentil(muestras.clone(), 0.50);
    let p95 = percentil(muestras, 0.95);
    println!("p95_turno: n=50 p50={p50:.2}ms p95={p95:.2}ms");

    if let Some(base) = P95_BASELINE_MS {
        assert!(
            p95 <= 0.5 * base,
            "p95 {p95:.2}ms supera la mitad del baseline {base:.2}ms"
        );
    }
}
