//! Ejecutor de tareas vencidas: el cron ejecuta TURNOS del agente, no solo
//! comandos (Bloque 3, Fase 8a). Extiende `schedule`: las tareas que tocan
//! ejecutar se corren como un turno completo del agente y el resumen se
//! entrega de forma durable (`tarea_finalizar` + `tarea_registrar_log`).
//!
//! Referencia: hermes-agent `cron/jobs.py` (claim fence, estados
//! terminales, reintentos visibles) + `cron/delivery_queue.py` (encolar →
//! reclamar → terminar → recuperar abandonadas). Aquí el claim fence es
//! [`AgentPersistence::tarea_tomar`], la entrega es
//! [`ProgramadorTareas::tarea_registrar_log`] y la recuperación es
//! [`AgentPersistence::tareas_recuperar_interrumpidas`].
//!
//! El ejecutor no conoce el transporte (lo llama `schedule run`, el daemon o
//! el consumidor task) y no filtra por fecha: ejecuta lo que la tienda le
//! entrega (contrato de [`AgentPersistence::tareas_pendientes`]: "las que
//! tocan ejecutar"). Quien SÍ tiene `proxima_ejecucion` (la cara CRUD)
//! filtra antes y llama a [`ejecutar_lista`].

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::Result;
use crate::evento::AgenteEvento;
use crate::ports::{AgentPersistence, ProgramadorTareas, TareaProgramadaPendiente};
use crate::runtime::AgentRuntime;
use crate::scheduler;

/// Resumen entregable acotado a 2000 caracteres (la tienda durable guarda el
/// texto completo del turno en sus propias tablas; el log lleva el resumen).
const RESUMEN_MAX_CHARS: usize = 2000;

/// Tope de un turno del cron: sin él, un proveedor que deja el stream abierto
/// sin responder bloquearía la cola para siempre (hermes clasifica
/// `grace/late`; aquí el turno se entrega como fallo con reintento visible).
const TURNO_TIMEOUT_SECS: u64 = 300;

/// Salida agregada de un turno ejecutado por el cron (se pliegan los
/// [`AgenteEvento`] del turno; el texto completo queda en la auditoría).
#[derive(Debug, Default)]
pub struct SalidaTurnoCron {
    /// Texto final acumulado del asistente (fragmentos `Token`).
    pub texto: String,
    /// Tools iniciadas durante el turno (sin duplicados, orden de aparición).
    pub herramientas: Vec<String>,
    /// Error presentable si el turno no cerró bien (`Error` del turno o fallo
    /// del motor). `None` = turno sano.
    pub error: Option<String>,
}

/// Motor que ejecuta el prompt de una tarea como un turno del agente.
/// [`AgentRuntime`] lo implementa; los tests inyectan un stub (sin red).
#[async_trait::async_trait]
pub trait MotorTurno: Send + Sync {
    /// Ejecuta `prompt` como turno de `user_id` y devuelve la salida agregada.
    async fn ejecutar(&self, user_id: Uuid, prompt: String) -> Result<SalidaTurnoCron>;
}

#[async_trait::async_trait]
impl MotorTurno for Arc<AgentRuntime> {
    async fn ejecutar(&self, user_id: Uuid, prompt: String) -> Result<SalidaTurnoCron> {
        let turno_id = Uuid::new_v4();
        let conversacion_id = Uuid::new_v4();
        // Cada ejecución del cron es una conversación propia (como hermes:
        // cada job tiene su transcript), nunca reutiliza una ajena.
        //
        // Patrón spawn+Done-break (daemon/run): el turno corre en una tarea
        // propia y la colecta corta en `Done`. Un `join!` en línea con el `tx`
        // prestado NO termina nunca: la colecta espera `None` (todos los
        // senders dropeados) pero el `tx` original vive en este frame hasta
        // que el `join!` completa — espera circular. Y el turno en línea
        // tampoco avanza (observado: cuelgue total; con spawn responde en
        // segundos con el mismo harness).
        let runtime = Arc::clone(self);
        let (tx, mut rx) = mpsc::channel::<AgenteEvento>(256);
        let tarea_turno = tokio::spawn(async move {
            runtime
                .ejecutar_turno(user_id, turno_id, conversacion_id, Vec::new(), prompt, &tx)
                .await
        });
        let carrera = tokio::time::timeout(Duration::from_secs(TURNO_TIMEOUT_SECS), async {
            let mut salida = SalidaTurnoCron::default();
            while let Some(evento) = rx.recv().await {
                let fin = matches!(evento, AgenteEvento::Done { .. });
                plegar_evento(evento, &mut salida);
                if fin {
                    break;
                }
            }
            // El turno ya emitió `Done` (último evento por contrato); solo
            // queda recoger su `Result` (el `?` interno ya viajó como evento
            // `Error` o como `Err` aquí).
            let resultado = tarea_turno.await;
            (resultado, salida)
        })
        .await;
        let (resultado, mut salida) = match carrera {
            Ok(par) => par,
            // El `timeout` dropea el `rx`: el turno ve el canal cerrado y
            // aborta (`tx.is_closed()`); la tarea spawneada termina sola.
            Err(_) => {
                return Ok(SalidaTurnoCron {
                    error: Some(format!(
                        "el turno superó el tope de {TURNO_TIMEOUT_SECS}s sin cerrar"
                    )),
                    ..SalidaTurnoCron::default()
                });
            }
        };
        match resultado {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                // No fallo silencioso: si el runtime devolvió error sin evento
                // `Error` previo, el motor lo entrega como error del turno.
                salida.error.get_or_insert_with(|| err.to_string());
            }
            Err(err) => {
                salida
                    .error
                    .get_or_insert_with(|| format!("el turno abortó con pánico: {err}"));
            }
        }
        Ok(salida)
    }
}

/// Pliega un evento del turno en la salida agregada (observador puro).
fn plegar_evento(evento: AgenteEvento, salida: &mut SalidaTurnoCron) {
    match evento {
        AgenteEvento::Token { texto } => salida.texto.push_str(&texto),
        AgenteEvento::ToolStart { tool, .. } if !salida.herramientas.contains(&tool) => {
            salida.herramientas.push(tool);
        }
        AgenteEvento::Error { mensaje, .. } => {
            salida.error.get_or_insert(mensaje);
        }
        _ => {}
    }
}

/// Agregado de una pasada del ejecutor (observabilidad del tick).
#[derive(Debug, Default)]
pub struct ResumenCron {
    /// Tareas que la tienda entregó en esta pasada.
    pub pendientes: usize,
    /// Turnos ejecutados con cierre sano.
    pub ejecutadas: u32,
    /// Turnos fallidos o con cron inválido (ver `tarea_logs` para el motivo).
    pub fallidas: u32,
    /// Reclamadas por otra réplica (`tarea_tomar` = false; hermes fence).
    pub omitidas: u32,
    /// Recurrentes reprogramadas con su próxima ejecución.
    pub reprogramadas: u32,
}

/// Empaqueta la salida de un turno en `(ok, resumen)` entregable.
fn empaquetar_entrega(salida: &SalidaTurnoCron) -> (bool, String) {
    let ok = salida.error.is_none();
    let mut cuerpo = salida.error.clone().unwrap_or_else(|| salida.texto.clone());
    if cuerpo.chars().count() > RESUMEN_MAX_CHARS {
        cuerpo = cuerpo.chars().take(RESUMEN_MAX_CHARS).collect();
        cuerpo.push('…');
    }
    let resumen = if salida.herramientas.is_empty() {
        cuerpo
    } else {
        format!("{cuerpo} [tools: {}]", salida.herramientas.join(", "))
    };
    (ok, resumen)
}

/// Ejecuta una pasada sobre las tareas dadas: reclamar → turno → entregar →
/// reprogramar recurrentes. Complementa a [`scheduler::ciclo_scheduler`]
/// (cola del store + runner del consumidor): aquí el llamador ya filtró por
/// vencimiento con la cara CRUD (que sí ve `proxima_ejecucion`) y el motor
/// ejecuta turnos con entrega durable. La reprogramación usa
/// [`scheduler::proxima_ejecucion`]; una recurrente con cron inválido NO se
/// ejecuta (fail-closed: se finaliza como fallo y se registra el motivo, sin
/// quemar un turno de LLM) y NO se reprograma (queda pendiente con su
/// próxima anterior: reintento visible que el operador cancela).
pub async fn ejecutar_lista(
    motor: &dyn MotorTurno,
    persistencia: &Arc<dyn AgentPersistence>,
    programador: &Arc<dyn ProgramadorTareas>,
    tareas: &[TareaProgramadaPendiente],
) -> Result<ResumenCron> {
    let mut resumen = ResumenCron {
        pendientes: tareas.len(),
        ..ResumenCron::default()
    };
    for tarea in tareas {
        if !persistencia.tarea_tomar(tarea.id).await? {
            resumen.omitidas += 1;
            continue;
        }
        // Recurrente con cron inválido: fallo sin ejecutar (ver doc superior).
        let proxima = if tarea.tipo == "recurrente" {
            match tarea.cron_expr.as_deref() {
                Some(expr) => match scheduler::proxima_ejecucion(expr, Utc::now()) {
                    Ok(fecha) => Some(fecha),
                    Err(_) => {
                        let motivo = format!("cron inválido: '{expr}' (tarea no ejecutada)");
                        persistencia
                            .tarea_finalizar(tarea.id, false, Some(&motivo))
                            .await?;
                        programador
                            .tarea_registrar_log(tarea.id, tarea.user_id, false, &motivo)
                            .await?;
                        resumen.fallidas += 1;
                        None
                    }
                },
                None => None,
            }
        } else {
            None
        };
        // Si la recurrente traía cron inválido ya se entregó el fallo arriba.
        let cron_roto =
            tarea.tipo == "recurrente" && tarea.cron_expr.is_some() && proxima.is_none();
        if cron_roto {
            continue;
        }
        let salida = match motor.ejecutar(tarea.user_id, tarea.prompt.clone()).await {
            Ok(salida) => salida,
            Err(err) => SalidaTurnoCron {
                error: Some(err.to_string()),
                ..SalidaTurnoCron::default()
            },
        };
        let (ok, entrega) = empaquetar_entrega(&salida);
        persistencia
            .tarea_finalizar(tarea.id, ok, Some(&entrega))
            .await?;
        programador
            .tarea_registrar_log(tarea.id, tarea.user_id, ok, &entrega)
            .await?;
        if ok {
            resumen.ejecutadas += 1;
        } else {
            resumen.fallidas += 1;
        }
        // Solo la recurrente con cron válido avanza su próxima ejecución; la
        // `una_vez` queda en el estado que `tarea_finalizar` le dio.
        if tarea.tipo == "recurrente" {
            if let Some(fecha) = proxima {
                persistencia
                    .tarea_reprogramar(tarea.id, tarea.user_id, Some(fecha))
                    .await?;
                resumen.reprogramadas += 1;
            }
        }
    }
    Ok(resumen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::{LogTareaEjecucion, MemoriaEntrada, SkillEntrada};
    use crate::ports::{MensajePersistido, TurnoPersistido};
    use std::collections::HashSet;
    use std::sync::Mutex;

    /// Motor stub: devuelve la salida programada (sin red ni LLM).
    struct MotorStub {
        salida: SalidaTurnoCron,
        ejecutadas: Mutex<Vec<(Uuid, String)>>,
    }

    impl MotorStub {
        fn sano(texto: &str) -> Self {
            Self {
                salida: SalidaTurnoCron {
                    texto: texto.into(),
                    herramientas: vec!["repo_map".into()],
                    error: None,
                },
                ejecutadas: Mutex::new(Vec::new()),
            }
        }

        fn roto() -> Self {
            Self {
                salida: SalidaTurnoCron {
                    error: Some("boom del motor".into()),
                    ..SalidaTurnoCron::default()
                },
                ejecutadas: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl MotorTurno for MotorStub {
        async fn ejecutar(&self, user_id: Uuid, prompt: String) -> Result<SalidaTurnoCron> {
            self.ejecutadas
                .lock()
                .expect("lock")
                .push((user_id, prompt));
            Ok(SalidaTurnoCron {
                texto: self.salida.texto.clone(),
                herramientas: self.salida.herramientas.clone(),
                error: self.salida.error.clone(),
            })
        }
    }

    /// Doble de persistencia con grabación (claim configurable por id).
    struct PersistenciaGrabadora {
        pendientes: Vec<TareaProgramadaPendiente>,
        tomadas: Mutex<HashSet<Uuid>>,
        niega: HashSet<Uuid>,
        finalizadas: Mutex<Vec<(Uuid, bool, String)>>,
        reprogramadas: Mutex<Vec<(Uuid, Option<chrono::DateTime<chrono::Utc>>)>>,
    }

    impl PersistenciaGrabadora {
        fn nueva(pendientes: Vec<TareaProgramadaPendiente>, niega: HashSet<Uuid>) -> Self {
            Self {
                pendientes,
                tomadas: Mutex::new(HashSet::new()),
                niega,
                finalizadas: Mutex::new(Vec::new()),
                reprogramadas: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl AgentPersistence for PersistenciaGrabadora {
        async fn guardar_turno(&self, _t: &TurnoPersistido) -> Result<()> {
            Ok(())
        }
        async fn finalizar_turno(&self, _: Uuid, _: &str, _: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn guardar_mensaje(&self, _: &MensajePersistido) -> Result<()> {
            Ok(())
        }
        async fn listar_mensajes(&self, _: Uuid) -> Result<Vec<MensajePersistido>> {
            Ok(Vec::new())
        }
        async fn conversacion_tocar(&self, _: Uuid) -> Result<()> {
            Ok(())
        }
        async fn registrar_accion(&self, _: &crate::ports::AccionAuditable) -> Result<()> {
            Ok(())
        }
        async fn memoria_listar(&self, _: Uuid) -> Result<Vec<MemoriaEntrada>> {
            Ok(Vec::new())
        }
        async fn memoria_upsert(&self, _: Uuid, _: &MemoriaEntrada) -> Result<()> {
            Ok(())
        }
        async fn memoria_borrar(&self, _: Uuid, _: &str) -> Result<()> {
            Ok(())
        }
        async fn skills_listar(&self, _: Uuid) -> Result<Vec<SkillEntrada>> {
            Ok(Vec::new())
        }
        async fn tareas_recuperar_interrumpidas(&self) -> Result<u64> {
            Ok(0)
        }
        async fn tareas_pendientes(&self, _: u32) -> Result<Vec<TareaProgramadaPendiente>> {
            Ok(self.pendientes.clone())
        }
        async fn tarea_tomar(&self, id: Uuid) -> Result<bool> {
            if self.niega.contains(&id) {
                return Ok(false);
            }
            self.tomadas.lock().expect("lock").insert(id);
            Ok(true)
        }
        async fn tarea_finalizar(&self, id: Uuid, ok: bool, resumen: Option<&str>) -> Result<()> {
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
            _: Uuid,
            proxima: Option<chrono::DateTime<chrono::Utc>>,
        ) -> Result<()> {
            self.reprogramadas.lock().expect("lock").push((id, proxima));
            Ok(())
        }
    }

    /// Doble de programador que graba las entregas.
    struct ProgramadorGrabador {
        entregas: Mutex<Vec<LogTareaEjecucion>>,
    }

    #[async_trait::async_trait]
    impl ProgramadorTareas for ProgramadorGrabador {
        async fn tarea_crear(&self, _: &crate::ports::NuevaTareaProgramada) -> Result<Uuid> {
            Ok(Uuid::new_v4())
        }
        async fn tareas_listar(&self, _: Uuid) -> Result<Vec<crate::ports::TareaProgramada>> {
            Ok(Vec::new())
        }
        async fn tarea_cancelar(&self, _: Uuid, _: Uuid) -> Result<bool> {
            Ok(false)
        }
        async fn tarea_logs(&self, _: Uuid, _: Uuid, _: u32) -> Result<Vec<LogTareaEjecucion>> {
            Ok(Vec::new())
        }
        async fn tarea_registrar_log(
            &self,
            id: Uuid,
            _user_id: Uuid,
            ok: bool,
            resumen: &str,
        ) -> Result<()> {
            self.entregas.lock().expect("lock").push(LogTareaEjecucion {
                id: Uuid::new_v4(),
                tarea_id: id,
                ok,
                resumen: resumen.to_string(),
                ejecutada_en: chrono::Utc::now(),
            });
            Ok(())
        }
    }

    fn pendiente(tipo: &str, cron: Option<&str>) -> TareaProgramadaPendiente {
        TareaProgramadaPendiente {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            nombre: "t".into(),
            prompt: "haz algo".into(),
            tipo: tipo.into(),
            cron_expr: cron.map(String::from),
        }
    }

    /// Tiendas del test: caras del puerto + dobles concretos para espiar.
    type TiendasPrueba = (
        Arc<dyn AgentPersistence>,
        Arc<dyn ProgramadorTareas>,
        Arc<PersistenciaGrabadora>,
        Arc<ProgramadorGrabador>,
    );

    fn arnes(pendientes: Vec<TareaProgramadaPendiente>, niega: HashSet<Uuid>) -> TiendasPrueba {
        let persistencia = Arc::new(PersistenciaGrabadora::nueva(pendientes, niega));
        let programador = Arc::new(ProgramadorGrabador {
            entregas: Mutex::new(Vec::new()),
        });
        let p: Arc<dyn AgentPersistence> = persistencia.clone();
        let g: Arc<dyn ProgramadorTareas> = programador.clone();
        (p, g, persistencia, programador)
    }

    #[tokio::test]
    async fn recurrente_sana_finaliza_entrega_y_reprograma() {
        let tarea = pendiente("recurrente", Some("0 9 * * *"));
        let (p, g, persistencia, programador) = arnes(vec![tarea.clone()], HashSet::new());
        let motor = MotorStub::sano("listo");
        let resumen = ejecutar_lista(&motor, &p, &g, &[tarea])
            .await
            .expect("ejecuta");
        assert_eq!(resumen.pendientes, 1);
        assert_eq!(resumen.ejecutadas, 1);
        assert_eq!(resumen.fallidas, 0);
        assert_eq!(resumen.reprogramadas, 1);
        let fin = persistencia.finalizadas.lock().expect("lock");
        assert_eq!(fin.len(), 1);
        assert!(fin[0].1, "finaliza ok=true");
        assert!(fin[0].2.contains("listo"), "el resumen lleva el texto");
        assert!(fin[0].2.contains("repo_map"), "el resumen cita tools");
        let entregas = programador.entregas.lock().expect("lock");
        assert_eq!(entregas.len(), 1, "la entrega queda en el log");
        assert!(entregas[0].ok);
        let reprog = persistencia.reprogramadas.lock().expect("lock");
        assert_eq!(reprog.len(), 1);
        assert!(reprog[0].1.is_some(), "avanza su próxima ejecución");
    }

    #[tokio::test]
    async fn claim_perdido_omite_sin_finalizar_ni_entregar() {
        let tarea = pendiente("recurrente", Some("0 9 * * *"));
        let mut niega = HashSet::new();
        niega.insert(tarea.id);
        let (p, g, persistencia, programador) = arnes(vec![tarea.clone()], niega);
        let motor = MotorStub::sano("x");
        let resumen = ejecutar_lista(&motor, &p, &g, &[tarea])
            .await
            .expect("ejecuta");
        assert_eq!(resumen.omitidas, 1);
        assert_eq!(resumen.ejecutadas, 0);
        assert!(motor.ejecutadas.lock().expect("lock").is_empty());
        assert!(persistencia.finalizadas.lock().expect("lock").is_empty());
        assert!(programador.entregas.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn motor_roto_finaliza_fallo_y_recurrente_reprograma() {
        let tarea = pendiente("recurrente", Some("0 9 * * *"));
        let (p, g, persistencia, programador) = arnes(vec![tarea.clone()], HashSet::new());
        let motor = MotorStub::roto();
        let resumen = ejecutar_lista(&motor, &p, &g, &[tarea])
            .await
            .expect("ejecuta");
        assert_eq!(resumen.ejecutadas, 0);
        assert_eq!(resumen.fallidas, 1);
        assert_eq!(resumen.reprogramadas, 1, "el cron sigue pese al fallo");
        let fin = persistencia.finalizadas.lock().expect("lock");
        assert!(!fin[0].1);
        assert!(fin[0].2.contains("boom del motor"));
        assert_eq!(programador.entregas.lock().expect("lock").len(), 1);
    }

    #[tokio::test]
    async fn una_vez_no_reprograma() {
        let tarea = pendiente("una_vez", None);
        let (p, g, persistencia, _) = arnes(vec![tarea.clone()], HashSet::new());
        let motor = MotorStub::sano("hecho");
        let resumen = ejecutar_lista(&motor, &p, &g, &[tarea])
            .await
            .expect("ejecuta");
        assert_eq!(resumen.ejecutadas, 1);
        assert_eq!(resumen.reprogramadas, 0);
        assert!(persistencia.reprogramadas.lock().expect("lock").is_empty());
    }

    #[tokio::test]
    async fn cron_invalido_no_ejecuta_y_entrega_fallo() {
        let tarea = pendiente("recurrente", Some("no-es-un-cron"));
        let (p, g, persistencia, programador) = arnes(vec![tarea.clone()], HashSet::new());
        let motor = MotorStub::sano("x");
        let resumen = ejecutar_lista(&motor, &p, &g, &[tarea])
            .await
            .expect("ejecuta");
        assert_eq!(resumen.fallidas, 1);
        assert_eq!(resumen.ejecutadas, 0);
        assert!(motor.ejecutadas.lock().expect("lock").is_empty());
        assert!(persistencia.reprogramadas.lock().expect("lock").is_empty());
        let entregas = programador.entregas.lock().expect("lock");
        assert_eq!(entregas.len(), 1);
        assert!(entregas[0].resumen.contains("cron inválido"));
    }

    #[test]
    fn entrega_trunca_texto_largo_y_cita_tools() {
        let salida = SalidaTurnoCron {
            texto: "a".repeat(2500),
            herramientas: vec!["repo_map".into()],
            error: None,
        };
        let (ok, resumen) = empaquetar_entrega(&salida);
        assert!(ok);
        assert!(resumen.chars().count() <= RESUMEN_MAX_CHARS + 40);
        assert!(resumen.ends_with("[tools: repo_map]"));
    }
}
