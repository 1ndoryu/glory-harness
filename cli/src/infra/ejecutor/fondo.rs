//! Rama de fondo del ejecutor (parte de `super`): spawn detached, registro
//! con tope de vivas y tarea pump que bombea, espera y archiva.
//!
//! Sin cambio de comportamiento: los bloques `impl` vivían en `ejecutor.rs`
//! (partición 309A-3). `registrar_fondo` se divide además en registro +
//! `bombear_pump` (la función superaba el máximo de 100 líneas efectivas).

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::time::Instant;

use tokio::sync::{Mutex, Notify};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use glory_harness_core::aplicar_entorno_minimo;
use glory_harness_core::error::{Error, Result};
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::{ChunkConsola, OrigenConsola, ResultadoEjecucionComando};

use super::super::jaula::dividir_argv;
use super::{
    Archivada, ConsolaViva, EjecutorCliente, HandleTarea, RegistroConsolas, RegistroFondo,
    MAX_CONSOLAS_RECIENTES, MAX_CONSOLAS_VIVAS,
};

/// Contexto de la tarea pump de un fondo (ver `bombear_pump`): todo lo que
/// la tarea necesita tras el registro, en un solo valor (evita el
/// `parametros-excesivos-rs` de clippy al extraer la tarea).
struct ContextoPump {
    handle: HandleTarea,
    viva: Arc<ConsolaViva>,
    chunks: Option<UnboundedSender<ChunkConsola>>,
    id: String,
    comando: String,
}

impl EjecutorCliente {
    /// Rama de fondo: lanza el comando detached y archiva su resultado en una
    /// tarea tokio; devuelve el id para `comando_status`/`comando_matar`.
    /// [209A-1 F1] `id` lo genera la tool (no aquí): es la clave de registro
    /// y viaja en `id_ejecucion`/`id_fondo` tal cual. El pump detached
    /// bombea líneas a `chunks` mientras haya receptor (turno vivo).
    /// [209A-1 F2] Registra la viva (comando, conversación, anillo) con tope
    /// `MAX_CONSOLAS_VIVAS`: la 5ª se RECHAZA (`Error::Ocupado`, hijo matado
    /// nada más nacer para no dejar zombis). El pump retira la viva al salir
    /// el hijo y archiva en `resultados` (reap automático). `stdin` con
    /// tubería retenida [219A-3]: la UI puede escribir a un fondo vivo; un
    /// fondo nunca lee el stdin del OPERADOR (no se hereda).
    pub(super) async fn ejecutar_fondo_con_id(
        &self,
        id: &str,
        comando: &str,
        conversacion_id: Uuid,
        chunks: Option<UnboundedSender<ChunkConsola>>,
        origen: OrigenConsola,
    ) -> Result<ResultadoEjecucionComando> {
        let mut hijo_inicial = self
            .construir_comando(comando)?
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // [209A-1 F4] Si el turno se cancela y la tarea pump se aborta,
            // el hijo no queda zombi: al soltar el `Child` se mata solo.
            .kill_on_drop(true)
            .spawn()?;
        let stdin_hijo = hijo_inicial.stdin.take();
        self.registrar_fondo(RegistroFondo {
            id: id.to_string(),
            comando: comando.to_string(),
            conversacion_id,
            chunks,
            origen,
            hijo_inicial,
            stdin_hijo,
        })
        .await
    }

    /// [219A-4] Spawn directo del shell del operador, SIN jaula (la jaula
    /// protege del modelo y deniega shells; el operador en loopback + sesión
    /// es otro nivel de confianza). `comando=None` = shell por defecto del
    /// SO; `Some(c)` = argv directo (troceo sin shell, sin reglas de jaula).
    /// El hijo hereda el cwd del ejecutor (`raiz` si hay).
    pub(super) async fn ejecutar_fondo_propio(
        &self,
        id: &str,
        comando: Option<&str>,
    ) -> Result<ResultadoEjecucionComando> {
        let (programa, argumentos, etiqueta) = match comando {
            Some(c) if !c.trim().is_empty() => {
                let argv =
                    dividir_argv(c).map_err(|motivo| Error::Sandbox(format!("argv: {motivo}")))?;
                let (primero, resto) = argv
                    .split_first()
                    .ok_or_else(|| Error::Sandbox("comando vacío".to_string()))?;
                (primero.clone(), resto.to_vec(), c.to_string())
            }
            _ => {
                let shell = if cfg!(windows) { "cmd" } else { "sh" };
                (shell.to_string(), Vec::new(), shell.to_string())
            }
        };
        let mut construido = tokio::process::Command::new(&programa);
        construido.args(&argumentos);
        construido
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if let Some(raiz) = self.raiz.as_ref() {
            construido.current_dir(raiz);
        }
        aplicar_entorno_minimo(&mut construido);
        let mut hijo_inicial = construido.spawn().map_err(|e| {
            Error::Sandbox(format!("no se pudo abrir la consola ({programa}): {e}"))
        })?;
        let stdin_hijo = hijo_inicial.stdin.take();
        self.registrar_fondo(RegistroFondo {
            id: id.to_string(),
            comando: etiqueta,
            conversacion_id: Uuid::nil(),
            chunks: None,
            origen: OrigenConsola::Usuario,
            hijo_inicial,
            stdin_hijo,
        })
        .await
    }

    /// [219A-5 F2] Reap ATÓMICO bajo un solo lock: la consola pasa de viva
    /// a archivada sin ventana invisible para `lista` o `estado`. Solo el
    /// pump archiva (la carrera con `matar` la gana por `remove`: `matar`
    /// NO toca el registro). `orden` = fin real; la evicción saca la más
    /// antigua, no una arbitraria del mapa.
    /// [219A-5 F3] La archivada retiene el dueño de la viva.
    async fn archivar(
        registro: &Arc<Mutex<RegistroConsolas>>,
        id: &str,
        resultado: ResultadoEjecucionComando,
        origen: OrigenConsola,
    ) {
        let mut reg = registro.lock().await;
        reg.vivas.remove(id);
        reg.archivadas.insert(
            id.to_string(),
            Archivada {
                resultado,
                origen,
            },
        );
        reg.orden.push_back(id.to_string());
        while reg.archivadas.len() > MAX_CONSOLAS_RECIENTES {
            let Some(viejo) = reg.orden.pop_front() else {
                break;
            };
            reg.archivadas.remove(&viejo);
        }
    }

    /// Registro + pump de un hijo ya spawneado (común al modelo y al
    /// operador): tope de vivas bajo lock, anillo, reap al salir.
    async fn registrar_fondo(&self, reg: RegistroFondo) -> Result<ResultadoEjecucionComando> {
        let RegistroFondo {
            id,
            comando,
            conversacion_id,
            chunks,
            origen,
            mut hijo_inicial,
            stdin_hijo,
        } = reg;
        // Tope de vivas BAJO EL MISMO LOCK que el registro (sin carrera:
        // dos fondos concurrentes no pueden colar la 5ª).
        let viva = {
            let mut reg = self.registro.lock().await;
            if reg.vivas.len() >= MAX_CONSOLAS_VIVAS {
                let _ = hijo_inicial.kill().await;
                let _ = hijo_inicial.wait().await;
                return Err(Error::Limite(format!(
                    "límite de consolas vivas alcanzado ({MAX_CONSOLAS_VIVAS}); usa comando_lista y espera o mata alguna"
                )));
            }
            let viva = Arc::new(ConsolaViva {
                comando: comando.to_string(),
                conversacion_id,
                origen,
                inicio: Instant::now(),
                suelta: AtomicBool::new(false),
                matar: Notify::new(),
                anillo: Mutex::new(VecDeque::new()),
                bytes_anillo: AtomicUsize::new(0),
                bytes_descartados: AtomicUsize::new(0),
                stdin: Mutex::new(stdin_hijo),
            });
            reg.vivas.insert(id.to_string(), Arc::clone(&viva));
            viva
        };
        let handle: HandleTarea = Arc::new(Mutex::new(Some(hijo_inicial)));
        self.tareas.lock().await.insert(id.to_string(), handle.clone());
        let tareas = self.tareas.clone();
        let registro = self.registro.clone();
        let ctx = ContextoPump {
            handle,
            viva,
            chunks,
            id: id.to_string(),
            comando: comando.to_string(),
        };
        tokio::spawn(async move {
            Self::bombear_pump(tareas, registro, ctx).await;
        });
        Ok(ResultadoEjecucionComando {
            codigo_salida: None,
            salida: "(comando lanzado en segundo plano; usa comando_status para consultar)"
                .to_string(),
            truncada: false,
            fondo: true,
            id_fondo: Some(id.to_string()),
            comando: comando.to_string(),
            id_ejecucion: id.to_string(),
        })
    }

    /// Tarea pump de un fondo ([209A-1 F1/F2/F4], [219A-4/5]): toma el hijo
    /// (`None` si `matar` se adelantó y ya lo mató), bombea stdout/stderr al
    /// stream y al anillo, espera la salida o el kill tardío, y archiva el
    /// resultado con reap atómico (retira la viva + archiva con dueño).
    async fn bombear_pump(
        tareas: Arc<Mutex<HashMap<String, HandleTarea>>>,
        registro: Arc<Mutex<RegistroConsolas>>,
        ctx: ContextoPump,
    ) {
        let ContextoPump {
            handle,
            viva,
            chunks,
            id,
            comando,
        } = ctx;
        // Tomamos el hijo (None si `matar` se adelantó y ya lo mató).
        let hijo = {
            let mut h = handle.lock().await;
            h.take()
        };
        let resultado = match hijo {
            None => ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(tarea de fondo terminada por comando_matar)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id.clone()),
                comando,
                id_ejecucion: id.clone(),
            },
            Some(mut hijo) => {
                // Drenaje CONCURRENTE con streaming (mismo motivo que en
                // síncrono: sin lector el hijo se bloquea al llenar el
                // pipe). Tras `wait()` los pipes llegan a EOF y las
                // bombas terminan solas.
                let presupuesto = Arc::new(AtomicUsize::new(0));
                let bomba_out = tokio::spawn(Self::bombear(
                    hijo.stdout.take(),
                    FlujoConsola::Stdout,
                    chunks.clone(),
                    Arc::clone(&presupuesto),
                    Some(Arc::clone(&viva)),
                ));
                let bomba_err = tokio::spawn(Self::bombear(
                    hijo.stderr.take(),
                    FlujoConsola::Stderr,
                    chunks,
                    presupuesto,
                    Some(Arc::clone(&viva)),
                ));
                // [209A-1 F4] Kill tardío: si `matar` llegó tras el
                // `take`, la señal lo pide aquí (el pump posee el hijo).
                let estado = tokio::select! {
                    estado = hijo.wait() => estado,
                    () = viva.matar.notified() => {
                        let _ = hijo.kill().await;
                        hijo.wait().await
                    }
                };
                let mut bytes = bomba_out.await.unwrap_or_default();
                bytes.extend_from_slice(&bomba_err.await.unwrap_or_default());
                match estado {
                    Ok(status) => {
                        let (texto, truncada) = Self::truncar(&bytes);
                        ResultadoEjecucionComando {
                            codigo_salida: status.code(),
                            salida: texto,
                            truncada,
                            fondo: true,
                            id_fondo: Some(id.clone()),
                            comando,
                            id_ejecucion: id.clone(),
                        }
                    }
                    Err(_) => ResultadoEjecucionComando {
                        codigo_salida: None,
                        salida: "(error capturando salida del proceso)".to_string(),
                        truncada: false,
                        fondo: true,
                        id_fondo: Some(id.clone()),
                        comando,
                        id_ejecucion: id.clone(),
                    },
                }
            }
        };
        // [219A-5 F2/F3] Reap atómico con dueño retenido (ver `archivar`).
        tareas.lock().await.remove(&id);
        Self::archivar(&registro, &id, resultado, viva.origen).await;
    }
}
