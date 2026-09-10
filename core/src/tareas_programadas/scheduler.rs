//! Scheduler genérico de tareas programadas (plan 318A-13, H8/frontera §6.1).
//!
//! Port agnóstico de `src/agent/scheduler.rs` de task: **sin SQL y sin
//! `AppState`** — toda la persistencia entra por [`AgentPersistence`]
//! (`tareas_*`); la lógica de reprogramación (cron v1) es pura y testeable.
//! El consumidor (task) implementa el puerto y aporta el runner que ejecuta
//! cada tarea como un turno de su runtime concreto.
//!
//! Semántica heredada de task (a prueba de reinicios): la ejecución marca
//! 'ejecutando' ANTES de llamar al runtime, así un crash no duplica — el
//! heartbeat vencido la vuelve a encolar, nunca se lanza dos veces el mismo
//! turno simultáneamente.

use chrono::{DateTime, Datelike, Timelike, Utc};
use std::future::Future;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::ports::{AgentPersistence, TareaProgramadaPendiente};

/// Heartbeat: una tarea en 'ejecutando' más vieja que esto se considera
/// interrumpida y vuelve a 'pendiente' (recuperación post-reinicio).
pub const HEARTBEAT_STALE: Duration = Duration::from_secs(10 * 60);

/// Worker del scheduler: loop cada `intervalo`. Se ejecuta en background desde
/// el binario del consumidor; los errores se loguean y el loop continúa
/// (nunca muere).
pub async fn correr_scheduler<E, R, Fut>(persistencia: &E, ejecutar_tarea: R, intervalo: Duration)
where
    E: AgentPersistence + ?Sized,
    R: Fn(TareaProgramadaPendiente, DateTime<Utc>) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    let mut ticker = tokio::time::interval(intervalo);
    loop {
        ticker.tick().await;
        if let Err(error) = ciclo_scheduler(persistencia, &ejecutar_tarea, Utc::now()).await {
            tracing::warn!(%error, "ciclo del scheduler de tareas programadas falló");
        }
    }
}

/// Un ciclo: recuperar interrumpidas + ejecutar las que tocan. Devuelve el
/// número de tareas ejecutadas (para tests y observabilidad).
pub async fn ciclo_scheduler<E, R, Fut>(
    persistencia: &E,
    ejecutar_tarea: R,
    ahora: DateTime<Utc>,
) -> Result<u32>
where
    E: AgentPersistence + ?Sized,
    R: Fn(TareaProgramadaPendiente, DateTime<Utc>) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    persistencia.tareas_recuperar_interrumpidas().await?;

    let tareas = persistencia.tareas_pendientes(5).await?;
    let mut ejecutadas = 0u32;
    for tarea in tareas {
        /* Marcar 'ejecutando' de forma atómica: si otra réplica la tomó
         * primero, `tarea_tomar` devuelve false y se salta (no duplica). */
        if !persistencia.tarea_tomar(tarea.id).await? {
            continue;
        }

        match ejecutar_tarea(tarea.clone(), ahora).await {
            Ok(resumen) => {
                tracing::info!(tarea = %tarea.id, nombre = %tarea.nombre, "tarea programada ejecutada");
                persistencia
                    .tarea_finalizar(tarea.id, true, Some(&resumen))
                    .await?;
                /* Recurrente: calcula la próxima ejecución desde cron_expr
                 * (formatos v1: `diario`, `cada{N}min`, `cada{N}h`,
                 * `cada{N}d`). `una_vez` no reprograma (None). */
                let proxima = if tarea.tipo == "recurrente" {
                    let expr = tarea.cron_expr.as_deref().unwrap_or("diario");
                    Some(proxima_ejecucion(expr, ahora)?)
                } else {
                    None
                };
                persistencia
                    .tarea_reprogramar(tarea.id, tarea.user_id, proxima)
                    .await?;
                ejecutadas += 1;
            }
            Err(error) => {
                tracing::warn!(tarea = %tarea.id, nombre = %tarea.nombre, %error, "tarea programada falló");
                persistencia
                    .tarea_finalizar(tarea.id, false, Some(&format!("Error: {error}")))
                    .await?;
            }
        }
    }
    Ok(ejecutadas)
}

/// Calcula la próxima ejecución para los formatos de cron_expr:
///
/// - v1 (heredado de task): `diario`, `cada{N}min`, `cada{N}h`, `cada{N}d`.
/// - v2 [318A-16 F6]: cron estándar de 5 campos, subconjunto
///   `M H * * DOW` (dom/mes solo `*`; dow `*` o 0-7, 0 y 7 = domingo) — el
///   formato que produce la traducción de lenguaje natural (hermes/grok).
pub fn proxima_ejecucion(expr: &str, desde: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let expr = expr.trim().to_ascii_lowercase();
    if expr == "diario" {
        return Ok(desde + chrono::Duration::days(1));
    }
    if let Some(resto) = expr.strip_prefix("cada") {
        let (numero, unidad) = parse_cantidad_unidad(resto)?;
        let duracion = match unidad.as_str() {
            "min" => chrono::Duration::minutes(numero),
            "h" => chrono::Duration::hours(numero),
            "d" => chrono::Duration::days(numero),
            _ => return Err(Error::Validacion(format!("Unidad cron inválida: {unidad}"))),
        };
        return Ok(desde + duracion);
    }
    /* v2: una expresión que arranca con dígito o `*` es cron estándar. */
    if expr.starts_with('*') || expr.as_bytes().first().is_some_and(u8::is_ascii_digit) {
        return proxima_cron_v2(&expr, desde);
    }
    Err(Error::Validacion(format!(
        "cron_expr no soportado: {expr} (use diario, cadaNmin, cadaNh, cadaNd o 'M H * * DOW')"
    )))
}

/// Próxima ocurrencia de un cron v2 `M H * * DOW` estrictamente posterior a
/// `desde`. Búsqueda minuto a minuto con horizonte de 8 días (una tarea
/// semanal siempre cae dentro); si no encaja, error claro en vez de silencio.
fn proxima_cron_v2(expr: &str, desde: DateTime<Utc>) -> Result<DateTime<Utc>> {
    let campos: Vec<&str> = expr.split_whitespace().collect();
    if campos.len() != 5 {
        return Err(Error::Validacion(format!(
            "cron v2 inválido: '{expr}' — se esperan 5 campos 'M H * * DOW'"
        )));
    }
    let (minuto, hora, dom, mes, dow) = (campos[0], campos[1], campos[2], campos[3], campos[4]);
    if dom != "*" || mes != "*" {
        return Err(Error::Validacion(format!(
            "cron v2 inválido: '{expr}' — el subconjunto v1 soporta solo día-de-semana (dom y mes deben ser '*')"
        )));
    }
    let parse_rango = |campo: &str, max: u32, nombre: &str| -> Result<Option<u32>> {
        if campo == "*" {
            return Ok(None);
        }
        let valor: u32 = campo.parse().map_err(|_| {
            Error::Validacion(format!(
                "cron v2 inválido: '{nombre}'='{campo}' (número o '*')"
            ))
        })?;
        if valor > max {
            return Err(Error::Validacion(format!(
                "cron v2 inválido: '{nombre}'={valor} fuera de rango 0-{max}"
            )));
        }
        Ok(Some(valor))
    };
    let min = parse_rango(minuto, 59, "minuto")?;
    let hora = parse_rango(hora, 23, "hora")?;
    let dow = parse_rango(dow, 7, "dow")?.map(|d| if d == 7 { 0 } else { d });

    let mut candidato = desde + chrono::Duration::seconds(60);
    /* 8 días × 24 h × 60 min: cota superior para cualquier dow fijo. */
    let horizonte_minutos: u32 = 8 * 24 * 60;
    for _ in 0..horizonte_minutos {
        let dia_semana = candidato.weekday().num_days_from_sunday();
        let coincide = min.is_none_or(|m| candidato.minute() == m)
            && hora.is_none_or(|h| candidato.hour() == h)
            && dow.is_none_or(|d| d == dia_semana);
        if coincide {
            return Ok(candidato);
        }
        candidato += chrono::Duration::seconds(60);
    }
    Err(Error::Validacion(format!(
        "cron v2 sin ocurrencia en el horizonte de 8 días: '{expr}'"
    )))
}

fn parse_cantidad_unidad(resto: &str) -> Result<(i64, String)> {
    let i = resto
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(resto.len());
    let (num, unidad) = resto.split_at(i);
    let numero: i64 = num
        .parse()
        .map_err(|_| Error::Validacion("Cantidad cron inválida".into()))?;
    if numero <= 0 {
        return Err(Error::Validacion("Cantidad cron debe ser positiva".into()));
    }
    Ok((numero, unidad.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    use crate::error::Result as CoreResult;

    /// Runner falso: registra las ejecuciones y devuelve el resumen o el
    /// error configurado, sin tocar ningún LLM.
    fn runner_falso(
        fallar: bool,
        ejecuciones: Arc<AtomicUsize>,
    ) -> impl Fn(
        TareaProgramadaPendiente,
        DateTime<Utc>,
    ) -> std::pin::Pin<Box<dyn Future<Output = CoreResult<String>> + Send>> {
        move |tarea, _ahora| {
            let contador = ejecuciones.clone();
            Box::pin(async move {
                contador.fetch_add(1, Ordering::SeqCst);
                if fallar {
                    Err(Error::Proveedor {
                        detalle: "proveedor caído".into(),
                        causa: None,
                    })
                } else {
                    Ok(format!("Tarea '{}' ejecutada", tarea.nombre))
                }
            })
        }
    }

    fn tarea_pendiente(nombre: &str, tipo: &str, cron: Option<&str>) -> TareaProgramadaPendiente {
        TareaProgramadaPendiente {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            nombre: nombre.into(),
            prompt: "haz algo".into(),
            tipo: tipo.into(),
            cron_expr: cron.map(str::to_string),
        }
    }

    /// Persistencia que registra las llamadas del scheduler en memoria.
    #[derive(Default)]
    struct PersistenciaScheduler {
        tomadas: Mutex<Vec<Uuid>>,
        tomadas_fallan: Mutex<Vec<Uuid>>,
        finalizadas: Mutex<Vec<(Uuid, bool, String)>>,
        reprogramadas: Mutex<Vec<(Uuid, Option<DateTime<Utc>>)>>,
        tareas: Mutex<Vec<TareaProgramadaPendiente>>,
    }

    #[async_trait::async_trait]
    impl AgentPersistence for PersistenciaScheduler {
        async fn guardar_turno(&self, _t: &crate::ports::TurnoPersistido) -> CoreResult<()> {
            Ok(())
        }
        async fn finalizar_turno(
            &self,
            _id: Uuid,
            _estado: &str,
            _resumen: Option<&str>,
        ) -> CoreResult<()> {
            Ok(())
        }
        async fn guardar_mensaje(&self, _m: &crate::ports::MensajePersistido) -> CoreResult<()> {
            Ok(())
        }
        async fn listar_mensajes(
            &self,
            _c: Uuid,
        ) -> CoreResult<Vec<crate::ports::MensajePersistido>> {
            Ok(Vec::new())
        }
        async fn conversacion_tocar(&self, _c: Uuid) -> CoreResult<()> {
            Ok(())
        }
        async fn registrar_accion(&self, _a: &crate::ports::AccionAuditable) -> CoreResult<()> {
            Ok(())
        }
        async fn memoria_listar(
            &self,
            _u: Uuid,
            _a: crate::ports::AmbitoMemoria,
        ) -> CoreResult<Vec<crate::ports::MemoriaEntrada>> {
            Ok(Vec::new())
        }
        async fn memoria_upsert(
            &self,
            _u: Uuid,
            _a: crate::ports::AmbitoMemoria,
            _e: &crate::ports::MemoriaEntrada,
        ) -> CoreResult<()> {
            Ok(())
        }
        async fn memoria_borrar(
            &self,
            _u: Uuid,
            _a: crate::ports::AmbitoMemoria,
            _c: &str,
        ) -> CoreResult<()> {
            Ok(())
        }
        async fn skills_listar(&self, _u: Uuid) -> CoreResult<Vec<crate::ports::SkillEntrada>> {
            Ok(Vec::new())
        }
        async fn tareas_recuperar_interrumpidas(&self) -> CoreResult<u64> {
            Ok(0)
        }
        async fn tareas_pendientes(
            &self,
            _limite: u32,
        ) -> CoreResult<Vec<TareaProgramadaPendiente>> {
            Ok(self.tareas.lock().expect("lock").clone())
        }
        async fn tarea_tomar(&self, id: Uuid) -> CoreResult<bool> {
            let falla = self.tomadas_fallan.lock().expect("lock").contains(&id);
            if !falla {
                self.tomadas.lock().expect("lock").push(id);
            }
            Ok(!falla)
        }
        async fn tarea_finalizar(
            &self,
            id: Uuid,
            ok: bool,
            resumen: Option<&str>,
        ) -> CoreResult<()> {
            self.finalizadas.lock().expect("lock").push((
                id,
                ok,
                resumen.unwrap_or_default().to_string(),
            ));
            Ok(())
        }
        async fn tarea_reprogramar(
            &self,
            id: Uuid,
            _user_id: Uuid,
            proxima: Option<DateTime<Utc>>,
        ) -> CoreResult<()> {
            self.reprogramadas.lock().expect("lock").push((id, proxima));
            Ok(())
        }
    }

    #[test]
    fn cron_diario_avanza_un_dia() {
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        let prox = proxima_ejecucion("diario", desde).expect("válido");
        assert_eq!(prox, desde + chrono::Duration::days(1));
    }

    #[test]
    fn cron_cada_horas() {
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        assert_eq!(
            proxima_ejecucion("cada2h", desde).expect("válido"),
            desde + chrono::Duration::hours(2)
        );
        assert_eq!(
            proxima_ejecucion("cada30min", desde).expect("válido"),
            desde + chrono::Duration::minutes(30)
        );
        assert_eq!(
            proxima_ejecucion("cada3d", desde).expect("válido"),
            desde + chrono::Duration::days(3)
        );
    }

    #[test]
    fn cron_invalido_rechazado() {
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        /* Campo fuera de rango. */
        assert!(proxima_ejecucion("61 9 * * *", desde).is_err());
        /* Subconjunto v2: dom/mes deben ser '*'. */
        assert!(proxima_ejecucion("0 9 1 * *", desde).is_err());
        /* Demasiados campos. */
        assert!(proxima_ejecucion("0 9 * * 1 2026", desde).is_err());
        assert!(proxima_ejecucion("cada0h", desde).is_err());
        assert!(proxima_ejecucion("semanal", desde).is_err());
    }

    #[test]
    fn cron_v2_diario_a_las_9() {
        /* Domingo 2026-08-30 10:00 → lunes 31 a las 09:00. */
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        let prox = proxima_ejecucion("0 9 * * *", desde).expect("válido");
        assert_eq!(
            prox,
            desde
                .date_naive()
                .succ_opt()
                .unwrap()
                .and_hms_opt(9, 0, 0)
                .unwrap()
                .and_utc()
        );
    }

    #[test]
    fn cron_v2_semanal_lunes() {
        /* Domingo 2026-08-30 10:00 → lunes 31 a las 09:00 (dow 1). */
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        let prox = proxima_ejecucion("0 9 * * 1", desde).expect("válido");
        assert_eq!(prox.weekday().num_days_from_sunday(), 1);
        assert_eq!((prox.hour(), prox.minute()), (9, 0));
        assert!(prox > desde);
    }

    #[test]
    fn cron_v2_si_el_dia_ya_paso_espera_semana() {
        /* Lunes 2026-08-31 10:00 → el próximo lunes 09:00 cae el 07-09. */
        let desde = DateTime::parse_from_rfc3339("2026-08-31T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        let prox = proxima_ejecucion("0 9 * * 1", desde).expect("válido");
        let esperado = chrono::NaiveDate::from_ymd_opt(2026, 9, 7)
            .unwrap()
            .and_hms_opt(9, 0, 0)
            .unwrap()
            .and_utc();
        assert_eq!(prox, esperado);
    }

    #[test]
    fn cron_v2_domingo_dow_0_y_7() {
        let desde = DateTime::parse_from_rfc3339("2026-08-31T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        let prox = proxima_ejecucion("0 9 * * 0", desde).expect("válido");
        assert_eq!(prox.weekday().num_days_from_sunday(), 0);
        let prox7 = proxima_ejecucion("0 9 * * 7", desde).expect("válido");
        assert_eq!(prox, prox7, "0 y 7 son ambos domingo");
    }

    #[test]
    fn cron_v2_hora_y_minuto_sin_dow() {
        let desde = DateTime::parse_from_rfc3339("2026-08-30T10:00:00Z")
            .expect("fecha")
            .with_timezone(&chrono::Utc);
        /* 30 14 * * *: la primera ocurrencia es hoy 14:30. */
        let prox = proxima_ejecucion("30 14 * * *", desde).expect("válido");
        assert_eq!((prox.hour(), prox.minute()), (14, 30));
        assert_eq!(prox.date_naive(), desde.date_naive());
    }

    #[tokio::test]
    async fn ciclo_ejecuta_recurrente_y_reprograma() {
        let ahora = chrono::Utc::now();
        let tarea = tarea_pendiente("resumen", "recurrente", Some("diario"));
        let persistencia = PersistenciaScheduler::default();
        persistencia
            .tareas
            .lock()
            .expect("lock")
            .push(tarea.clone());
        let ejecuciones = Arc::new(AtomicUsize::new(0));

        let n = ciclo_scheduler(
            &persistencia,
            runner_falso(false, ejecuciones.clone()),
            ahora,
        )
        .await
        .expect("ciclo ok");

        assert_eq!(n, 1);
        assert_eq!(ejecuciones.load(Ordering::SeqCst), 1);
        assert_eq!(
            persistencia.tomadas.lock().expect("lock").clone(),
            vec![tarea.id]
        );
        let finalizadas = persistencia.finalizadas.lock().expect("lock").clone();
        assert_eq!(finalizadas.len(), 1);
        assert_eq!(finalizadas[0].0, tarea.id);
        assert!(finalizadas[0].1, "se finaliza como completada");
        let reprogramadas = persistencia.reprogramadas.lock().expect("lock").clone();
        assert_eq!(reprogramadas.len(), 1);
        assert_eq!(
            reprogramadas[0].1,
            Some(ahora + chrono::Duration::days(1)),
            "recurrente diario reprograma +1 día"
        );
    }

    #[tokio::test]
    async fn ciclo_una_vez_no_reprograma() {
        let ahora = chrono::Utc::now();
        let tarea = tarea_pendiente("limpiar", "una_vez", None);
        let persistencia = PersistenciaScheduler::default();
        persistencia.tareas.lock().expect("lock").push(tarea);
        let ejecuciones = Arc::new(AtomicUsize::new(0));

        ciclo_scheduler(&persistencia, runner_falso(false, ejecuciones), ahora)
            .await
            .expect("ciclo ok");

        let reprogramadas = persistencia.reprogramadas.lock().expect("lock").clone();
        assert_eq!(reprogramadas.len(), 1);
        assert_eq!(reprogramadas[0].1, None, "una_vez queda desprogramada");
    }

    #[tokio::test]
    async fn ciclo_fallo_marca_fallida_y_no_reprograma() {
        let ahora = chrono::Utc::now();
        let tarea = tarea_pendiente("resumen", "recurrente", Some("cada2h"));
        let persistencia = PersistenciaScheduler::default();
        persistencia
            .tareas
            .lock()
            .expect("lock")
            .push(tarea.clone());
        let ejecuciones = Arc::new(AtomicUsize::new(0));

        let n = ciclo_scheduler(
            &persistencia,
            runner_falso(true, ejecuciones.clone()),
            ahora,
        )
        .await
        .expect("ciclo no aborta por fallo de una tarea");

        assert_eq!(n, 0);
        assert_eq!(ejecuciones.load(Ordering::SeqCst), 1, "el runner se llamó");
        let finalizadas = persistencia.finalizadas.lock().expect("lock").clone();
        assert_eq!(finalizadas.len(), 1);
        assert!(!finalizadas[0].1, "fallida");
        assert!(finalizadas[0].2.contains("Error:"));
        assert!(
            persistencia.reprogramadas.lock().expect("lock").is_empty(),
            "una tarea fallida no se reprograma"
        );
    }

    #[tokio::test]
    async fn ciclo_salta_tarea_tomada_por_otra_replica() {
        let ahora = chrono::Utc::now();
        let tarea = tarea_pendiente("resumen", "recurrente", Some("diario"));
        let persistencia = PersistenciaScheduler::default();
        persistencia
            .tomadas_fallan
            .lock()
            .expect("lock")
            .push(tarea.id);
        persistencia.tareas.lock().expect("lock").push(tarea);
        let ejecuciones = Arc::new(AtomicUsize::new(0));

        let n = ciclo_scheduler(
            &persistencia,
            runner_falso(false, ejecuciones.clone()),
            ahora,
        )
        .await
        .expect("ciclo ok");

        assert_eq!(n, 0);
        assert_eq!(
            ejecuciones.load(Ordering::SeqCst),
            0,
            "si otra réplica la tomó no se ejecuta"
        );
        assert!(
            persistencia.finalizadas.lock().expect("lock").is_empty(),
            "ni se finaliza"
        );
    }
}
