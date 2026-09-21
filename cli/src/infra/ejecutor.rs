//! Ejecutor real de comandos del CLI (318A-16 F3).
//!
//! Implementa el puerto `EjecutorComando` del núcleo con tokio `Command`:
//! ejecución DIRECTA sin shell (jaula `infra::jaula`, 139A-8 F1/K1), timeout
//! acotado, truncado de salida a 8 KB y tareas de fondo identificadas por id
//! (`comando_status`/`comando_matar`). El núcleo queda agnóstico: solo ve este
//! trait; un consumidor sin runner (p. ej. PROYECTO TASKS, que deniega
//! comandos) no registra la tool en absoluto (fail-closed).
//!
//! [119A-7 F0] Jaula: el ejecutor puede fijar el directorio de arranque de
//! cada hijo (`en_raiz`). Los comandos heredan ese cwd, así que las rutas
//! relativas del modelo caen dentro del workspace del run. Límite honesto:
//! el cwd no contiene `..` absolutos; la contención total la dan la jaula
//! (sin shell + allowlist/denylist) + clasificación de riesgo
//! (`bash_clasificar`) + aprobación + supervisión. `nuevo()` (sin raíz,
//! hereda el cwd del proceso) queda solo para diagnósticos sin run.
//!
//! [139A-8 F1/K1] Sin shell: `ejecutar`/`ejecutar_fondo` construyen con
//! `jaula::construir_directo` (argv directo, builtins `cmd` con veto en
//! Windows). Tuberías/redirecciones/`&&`/`$()` se DENIEGAN con mensaje
//! claro (cambio de conducta documentado en `jaula.rs`).

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::{Mutex, Notify};
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use glory_harness_core::aplicar_entorno_minimo;
use glory_harness_core::error::{Error, Result};
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::{
    ChunkConsola, EjecutorComando, InfoConsola, OrigenConsola, ResultadoEjecucionComando,
    TranscriptConsola,
};

use super::jaula::{construir_directo, dividir_argv};

/// Límite de salida capturada por comando (8 KB, contrato del plan 318A-16).
const LIMITE_SALIDA: usize = 8 * 1024;
/// Timeout por comando síncrono: 120 s (un comando colgado no bloquea el turno).
const TIMEOUT_COMANDO: Duration = Duration::from_secs(120);
/// [209A-1 F1] Tope de stream en vivo por ejecución (64 KB): pasado el tope
/// el pump deja de reenviar líneas pero SIGUE acumulando para el resultado
/// final de 8 KB. Anti-fuga mínimo (un `yes` infinito no hincha el canal ni
/// el SSE); F2 formaliza los topes de consola viva (ring + transcript).
const LIMITE_STREAM_BYTES: usize = 64 * 1024;
/// [209A-1 F1] Línea de stream recortada a 2 KB en chars (barras de progreso
/// con `\r` sin `\n` no hinchan el canal; el resultado final conserva su
/// propio truncado a 8 KB).
const LIMITE_LINEA_STREAM: usize = 2048;
/// [209A-1 F2] Máximo de consolas vivas simultáneas: la 5ª ejecución
/// desacoplada se RECHAZA (`Error::Ocupado`) en vez de ejecutarse sin
/// visor. Anti-fuga: sin tope, un modelo podría lanzar N builds en fondo.
const MAX_CONSOLAS_VIVAS: usize = 4;
/// [209A-1 F2] Ring por consola viva (128 KB): el hilo pump retiene lo
/// último para `comando_lista`/reconexión; al superar el tope se descartan
/// las líneas más antiguas (contador `bytes_descartados` en el fin).
const LIMITE_RING_BYTES: usize = 128 * 1024;
/// [209A-1 F2] Máximo de consolas recientes recordadas: el mapa `resultados`
/// no crece sin cota (una sesión larga con cientos de ejecuciones no hincha
/// la memoria del proceso CLI/web).
const MAX_CONSOLAS_RECIENTES: usize = 64;
/// [219A-3] Tope por escritura al stdin de una consola viva (64 KB): una
/// pegada accidental no hincha la tubería; el llamador trocea si necesita más.
const MAX_ESCRITURA_STDIN: usize = 64 * 1024;
/// [219A-3] Tope de líneas del transcript de `salida` (backfill de la UI al
/// abrir la tab): la UI pide el vivo + este volcado acotado, no el ring entero.
const MAX_LINEAS_TRANSCRIPT: usize = 2000;

/// [209A-1 F2] Consola viva: metadatos + anillo de una ejecución desacoplada
/// en curso. El hijo lo posee la tarea pump (`tareas`, como en F1: quien lo
/// toma —pump o `matar`— gana); la viva solo retiene lo visible para
/// `comando_lista`/el tab Consola. El pump la retira de `vivas` al salir el
/// hijo y archiva en `resultados` (reap automático); `matar` mata al hijo y
/// el pump hace el resto. `desacoplar` marca `suelta` para que el pump deje
/// de intentar el envío al turno (sigue acumulando + anillo).
struct ConsolaViva {
    comando: String,
    conversacion_id: Uuid,
    inicio: Instant,
    /// [219A-4] Dueño (agente por defecto; `Usuario` en consolas propias).
    origen: OrigenConsola,
    /// Señal de `desacoplar`: el pump suelta el envío al turno.
    suelta: AtomicBool,
    /// [209A-1 F4] Señal de kill tardío: si `matar` llega cuando el pump ya
    /// tomó el hijo (el handle quedó en `None`), el pump lo mata él mismo
    /// (es el único que posee el `Child` en ese momento).
    matar: Notify,
    /// Anillo de líneas recientes (capado a `LIMITE_RING_BYTES`).
    /// [219A-3] Guarda el chunk con su flujo: el transcript de `salida` lo
    /// necesita para el backfill de la UI (stdout vs stderr).
    anillo: Mutex<VecDeque<ChunkConsola>>,
    bytes_anillo: AtomicUsize,
    bytes_descartados: AtomicUsize,
    /// [219A-3] Stdin del hijo para `escribir` (interactuar desde la UI).
    /// El pump nunca lo toca; `None` cuando el hijo ya salió (el `Child` lo
    /// posee la tarea pump y al salir no hay a quién escribir).
    stdin: Mutex<Option<ChildStdin>>,
}

/// [219A-4] Parámetros del registro común `registrar_fondo` (un solo
/// registro + pump para el modelo y el operador; el struct evita el
/// `too_many_arguments` de clippy). El hijo ya viene spawneado.
struct RegistroFondo {
    id: String,
    comando: String,
    conversacion_id: Uuid,
    chunks: Option<UnboundedSender<ChunkConsola>>,
    origen: OrigenConsola,
    hijo_inicial: Child,
    stdin_hijo: Option<ChildStdin>,
}

/// Handle compartido de una tarea de fondo: el spawner y `matar` compiten por
/// el `Child`; quien lo toma (o mata) lo deja en `None`.
type HandleTarea = Arc<Mutex<Option<Child>>>;

/// Implementación concreta del puerto para el CLI.
pub struct EjecutorCliente {
    tareas: Arc<Mutex<HashMap<String, HandleTarea>>>,
    resultados: Arc<Mutex<HashMap<String, ResultadoEjecucionComando>>>,
    /// [209A-1 F2] Consolas vivas por `id_ejecucion`: ejecución desacoplada
    /// en curso (hijo + anillo). El reaper las retira al salir el hijo.
    vivas: Arc<Mutex<HashMap<String, Arc<ConsolaViva>>>>,
    /// [119A-7 F0] Raíz enjaulada: cwd de arranque de cada hijo.
    /// `None` = heredar el cwd del proceso (solo diagnósticos sin run).
    raiz: Option<PathBuf>,
}

impl EjecutorCliente {
    #[must_use]
    pub fn nuevo() -> Self {
        Self {
            tareas: Arc::default(),
            resultados: Arc::default(),
            vivas: Arc::default(),
            raiz: None,
        }
    }

    /// Ejecutor enjaulado: cada comando arranca con cwd = `raiz`.
    #[must_use]
    pub fn en_raiz(raiz: PathBuf) -> Self {
        Self {
            tareas: Arc::default(),
            resultados: Arc::default(),
            vivas: Arc::default(),
            raiz: Some(raiz),
        }
    }

    /// [139A-8 F1/K1] Construcción SIN shell vía `jaula::construir_directo`.
    /// La denegación de la jaula se traduce a `Error::Sandbox` (el sandbox
    /// bloqueó el comando) con el mensaje claro de la jaula.
    /// [139A-8 F3n/K2] Punto único de spawn del modelo: el hijo NO hereda el
    /// entorno del operador (claves LLM) — solo el subconjunto mínimo
    /// (`aplicar_entorno_minimo`). Cubre `ejecutar_sincrono` y
    /// `ejecutar_fondo` (ambos pasan por aquí).
    fn construir_comando(&self, comando: &str) -> Result<Command> {
        let mut construido = construir_directo(comando, self.raiz.as_ref())
            .map_err(|motivo| Error::Sandbox(format!("jaula: {motivo}")))?;
        aplicar_entorno_minimo(&mut construido);
        Ok(construido)
    }

    /// [209A-1 F1] Bomba de un pipe: reenvía cada línea a `chunks` (hasta el
    /// tope compartido `presupuesto`) y devuelve los bytes CRUDOS para el
    /// resultado final. Conversión `lossy` como `truncar` (la salida OEM de
    /// Windows no es UTF-8 y `read_line` la cortaría en seco). Si el receptor
    /// se fue (turno cerrado) solo acumula. `None` = modo sin streaming.
    /// [209A-1 F2] `anillo`: la viva a cuyo ring se empuja cada línea
    /// (recortada igual que el stream); `None` en la rama síncrona (su
    /// resultado final ya es el transcript).
    async fn bombear<T>(
        tubo: Option<T>,
        flujo: FlujoConsola,
        chunks: Option<UnboundedSender<ChunkConsola>>,
        presupuesto: Arc<AtomicUsize>,
        anillo: Option<Arc<ConsolaViva>>,
    ) -> Vec<u8>
    where
        T: AsyncRead + Unpin + Send + 'static,
    {
        let Some(tubo) = tubo else {
            return Vec::new();
        };
        let mut lector = BufReader::new(tubo);
        let mut crudo = Vec::new();
        let mut segmento = Vec::new();
        loop {
            segmento.clear();
            match lector.read_until(b'\n', &mut segmento).await {
                Ok(0) => break,
                Ok(_) => {
                    crudo.extend_from_slice(&segmento);
                    let texto = String::from_utf8_lossy(&segmento);
                    let linea = texto.trim_end_matches(['\r', '\n']);
                    let recorte: String =
                        linea.chars().take(LIMITE_LINEA_STREAM).collect();
                    let recortada = recorte.len() < linea.len();
                    let mut linea = recorte;
                    if recortada {
                        linea.push_str("…(línea recortada)");
                    }
                    // [219A-4] Sin `chunks` (consola propia: ningún turno la
                    // emite) no hay a quién enviar, pero el anillo SÍ se
                    // empuja: `salida` y la UI lo leen de ahí.
                    if let Some(tx) = &chunks {
                        // Turno cerrado o consola desacoplada: no se envía, pero
                        // la viva sigue visible en `comando_lista` vía su anillo.
                        let suelta = anillo
                            .as_ref()
                            .is_some_and(|v| v.suelta.load(Ordering::Relaxed));
                        if !tx.is_closed() && !suelta {
                            let reservado =
                                presupuesto.fetch_add(linea.len(), Ordering::Relaxed);
                            if reservado < LIMITE_STREAM_BYTES {
                                let _ = tx.send(ChunkConsola { flujo, linea: linea.clone() });
                            }
                        }
                    }
                    if let Some(v) = &anillo {
                        Self::empujar_anillo(v, flujo, &linea).await;
                    }
                }
                Err(_) => break,
            }
        }
        crudo
    }

    /// [209A-1 F2] Empuja una línea al anillo, descartando las más antiguas
    /// al superar `LIMITE_RING_BYTES` (contador para el fin).
    async fn empujar_anillo(viva: &Arc<ConsolaViva>, flujo: FlujoConsola, linea: &str) {
        let mut anillo = viva.anillo.lock().await;
        let mut bytes = viva.bytes_anillo.load(Ordering::Relaxed);
        bytes += linea.len();
        anillo.push_back(ChunkConsola {
            flujo,
            linea: linea.to_string(),
        });
        while bytes > LIMITE_RING_BYTES {
            if let Some(vieja) = anillo.pop_front() {
                bytes = bytes.saturating_sub(vieja.linea.len());
                viva.bytes_descartados
                    .fetch_add(vieja.linea.len(), Ordering::Relaxed);
            } else {
                break;
            }
        }
        viva.bytes_anillo.store(bytes, Ordering::Relaxed);
    }

    /// [139A-8 F1] Truncado a 8 KB en BYTES (no en chars): la salida OEM
    /// (`dir`, `ping`…) llega con tildes/ñ que `from_utf8_lossy` convierte
    /// en `�` (3 bytes); contar chars dejaba pasar hasta 3× el límite y
    /// rompía el contrato de 8 KB. El corte respeta borde de char.
    fn truncar(salida: &[u8]) -> (String, bool) {
        let texto = String::from_utf8_lossy(salida);
        if texto.len() <= LIMITE_SALIDA {
            (texto.into_owned(), false)
        } else {
            let mut fin = LIMITE_SALIDA;
            while !texto.is_char_boundary(fin) {
                fin -= 1;
            }
            let mut cortado: String = texto[..fin].to_string();
            cortado.push_str("\n…(salida truncada por límite del harness)");
            (cortado, true)
        }
    }
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
    async fn ejecutar_fondo_con_id(
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
    async fn ejecutar_fondo_propio(
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
        let mut construido = Command::new(&programa);
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
            let mut vivas = self.vivas.lock().await;
            if vivas.len() >= MAX_CONSOLAS_VIVAS {
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
            vivas.insert(id.to_string(), Arc::clone(&viva));
            viva
        };
        let handle: HandleTarea = Arc::new(Mutex::new(Some(hijo_inicial)));
        self.tareas.lock().await.insert(id.to_string(), handle.clone());
        let tareas = self.tareas.clone();
        let resultados = self.resultados.clone();
        let vivas = self.vivas.clone();
        let id_detach = id.to_string();
        let comando_archivo = comando.to_string();
        tokio::spawn(async move {
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
                    id_fondo: Some(id_detach.clone()),
                    comando: comando_archivo,
                    id_ejecucion: id_detach.clone(),
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
                                id_fondo: Some(id_detach.clone()),
                                comando: comando_archivo,
                                id_ejecucion: id_detach.clone(),
                            }
                        }
                        Err(_) => ResultadoEjecucionComando {
                            codigo_salida: None,
                            salida: "(error capturando salida del proceso)".to_string(),
                            truncada: false,
                            fondo: true,
                            id_fondo: Some(id_detach.clone()),
                            comando: comando_archivo,
                            id_ejecucion: id_detach.clone(),
                        },
                    }
                }
            };
            // Reap: retirar la viva y archivar (el `remove` hace idempotente
            // la carrera con `matar`, que NO toca `vivas` a propósito: el
            // pump es el único que archiva).
            tareas.lock().await.remove(&id_detach);
            vivas.lock().await.remove(&id_detach);
            {
                let mut r = resultados.lock().await;
                r.insert(id_detach.clone(), resultado);
                let sobran = r.len().saturating_sub(MAX_CONSOLAS_RECIENTES);
                if sobran > 0 {
                    let ids: Vec<String> =
                        r.keys().take(sobran).cloned().collect();
                    for id in ids {
                        r.remove(&id);
                    }
                }
            }
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

    /// Rama síncrona: corre con timeout y devuelve la salida truncada a 8 KB.
    /// [209A-1 F1] Las lectoras ahora son bombas por línea (`bombear`): sin
    /// streaming (`chunks=None`) se comportan como el drenaje anterior.
    async fn ejecutar_sincrono_en_vivo(
        &self,
        id: &str,
        comando: &str,
        chunks: Option<UnboundedSender<ChunkConsola>>,
    ) -> Result<ResultadoEjecucionComando> {
        let mut child = self
            .construir_comando(comando)?
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // [209A-1 F4] Cancelación del turno a mitad de `wait()`: soltar
            // el `Child` mata al hijo en vez de dejarlo huérfano.
            .kill_on_drop(true)
            .spawn()?;
        // Tomamos los pipes antes de `wait()` (que solo presta `child`, así el
        // timeout puede matarlo después sin mover el valor).
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        // [139A-8 F1] Drenaje CONCURRENTE de los pipes: si el hijo escribe
        // más que el buffer del pipe (~64 KB) y nadie lee, se bloquea y el
        // `wait()` muere por timeout aunque el comando sea instantáneo
        // (`dir System32` = 299 KB lo demostró). Las bombas son dueñas de
        // los pipes; el `wait()` solo presta `child`.
        let presupuesto = Arc::new(AtomicUsize::new(0));
        let drenar_out = tokio::spawn(Self::bombear(
            stdout,
            FlujoConsola::Stdout,
            chunks.clone(),
            Arc::clone(&presupuesto),
            None,
        ));
        let drenar_err = tokio::spawn(Self::bombear(
            stderr,
            FlujoConsola::Stderr,
            chunks,
            presupuesto,
            None,
        ));

        let estado = tokio::time::timeout(TIMEOUT_COMANDO, child.wait()).await;
        if estado.is_err() {
            // Timeout: matar PRIMERO para que los pipes lleguen a EOF y las
            // bombas terminen; solo después se reúnen. (Al revés se cuelga:
            // el hijo vivo retiene la escritura y el `await` no vuelve.)
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        // Tras el `wait` (o el kill por timeout) los pipes llegan a EOF: las
        // bombas siempre terminan y se pueden reunir sin timeout extra.
        let mut capturado = drenar_out.await.unwrap_or_default();
        capturado.extend_from_slice(&drenar_err.await.unwrap_or_default());
        match estado {
            Ok(Ok(status)) => {
                let (texto, truncada) = Self::truncar(&capturado);
                Ok(ResultadoEjecucionComando {
                    codigo_salida: status.code(),
                    salida: texto,
                    truncada,
                    fondo: false,
                    id_fondo: None,
                    comando: comando.to_string(),
                    id_ejecucion: id.to_string(),
                })
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                // Timeout: devolvemos lo capturado hasta el kill.
                let mut texto = format!(
                    "⏱ el comando excedió el límite de {} s y fue terminado\n",
                    TIMEOUT_COMANDO.as_secs()
                );
                texto.push_str(&String::from_utf8_lossy(&capturado));
                Ok(ResultadoEjecucionComando {
                    codigo_salida: None,
                    salida: texto,
                    truncada: true,
                    fondo: false,
                    id_fondo: None,
                    comando: comando.to_string(),
                    id_ejecucion: id.to_string(),
                })
            }
        }
    }

    /// Mata el hijo de una viva SIN tocar `vivas`: el pump es el único que
    /// retira y archiva (idempotente ante la carrera matar↔salida natural).
    /// [209A-1 F4] Cubre las dos ventanas: handle aún en `tareas` (kill
    /// directo) o ya tomado por el pump (señal `matar`: el pump lo mata él
    /// mismo). Devuelve `true` si había algo que matar.
    async fn matar_handle(&self, id: &str) -> bool {
        {
            let tareas = self.tareas.lock().await;
            if let Some(handle) = tareas.get(id) {
                let mut h = handle.lock().await;
                if let Some(hijo) = h.as_mut() {
                    let _ = hijo.kill().await;
                    let _ = hijo.wait().await;
                    return true;
                }
            }
        }
        if let Some(viva) = self.vivas.lock().await.get(id) {
            viva.matar.notify_one();
            return true;
        }
        false
    }

    /// [209A-1 F4] Reap por conversación: mata las vivas de `conv` (las de
    /// otras conversaciones siguen). El pump de cada una retira su viva y
    /// archiva el transcript acotado. Devuelve cuántas mató. Sin vivas
    /// coincide: devuelve 0, sin error.
    pub async fn matar_por_conversacion(&self, conv: Uuid) -> usize {
        let ids: Vec<String> = {
            self.vivas
                .lock()
                .await
                .iter()
                .filter(|(_, v)| v.conversacion_id == conv)
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut matadas = 0;
        for id in ids {
            if self.matar_handle(&id).await {
                matadas += 1;
            }
        }
        matadas
    }

    /// [209A-1 F4] Reap global (cierre de app): mata TODAS las vivas, de
    /// cualquier conversación. Cada pump retira y archiva; devuelve el
    /// conteo. Punto de enganche para el cierre ordenado de cada
    /// transporte (hoy sin cablear: ver plan 209A-1 §F4).
    pub async fn matar_todas(&self) -> usize {
        let ids: Vec<String> = { self.vivas.lock().await.keys().cloned().collect() };
        let mut matadas = 0;
        for id in ids {
            if self.matar_handle(&id).await {
                matadas += 1;
            }
        }
        matadas
    }
}

#[async_trait]
impl EjecutorComando for EjecutorCliente {
    async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando> {
        // Sin streaming: el id solo rellena los campos nuevos del resultado.
        // Sin conversación conocida: `nil` (diagnósticos sin run).
        let id = uuid::Uuid::new_v4().to_string();
        if fondo {
            self.ejecutar_fondo_con_id(&id, comando, Uuid::nil(), None, OrigenConsola::Agente)
                .await
        } else {
            self.ejecutar_sincrono_en_vivo(&id, comando, None).await
        }
    }

    async fn ejecutar_en_vivo(
        &self,
        id: &str,
        comando: &str,
        conversacion_id: Uuid,
        fondo: bool,
        chunks: UnboundedSender<ChunkConsola>,
    ) -> Result<ResultadoEjecucionComando> {
        if fondo {
            self.ejecutar_fondo_con_id(
                id,
                comando,
                conversacion_id,
                Some(chunks),
                OrigenConsola::Agente,
            )
            .await
        } else {
            self.ejecutar_sincrono_en_vivo(id, comando, Some(chunks))
                .await
        }
    }

    async fn desacoplar(&self, id: &str) -> Result<()> {
        let vivas = self.vivas.lock().await;
        let viva = vivas
            .get(id)
            .ok_or_else(|| Error::NoEncontrado(format!("consola desconocida: {id}")))?;
        viva.suelta.store(true, Ordering::Relaxed);
        Ok(())
    }

    /// [219A-4] Consola propia del operador (ver `ejecutar_fondo_propio`).
    async fn ejecutar_propia(&self, comando: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.ejecutar_fondo_propio(&id, comando).await?;
        Ok(id)
    }

    async fn lista(&self) -> Result<Vec<InfoConsola>> {
        let mut infos: Vec<(Instant, InfoConsola)> = Vec::new();
        {
            let vivas = self.vivas.lock().await;
            for (id, viva) in vivas.iter() {
                infos.push((
                    viva.inicio,
                    InfoConsola {
                        id_ejecucion: id.clone(),
                        comando: viva.comando.clone(),
                        conversacion_id: viva.conversacion_id,
                        viva: true,
                        codigo_salida: None,
                        bytes: viva.bytes_anillo.load(Ordering::Relaxed),
                        origen: viva.origen,
                    },
                ));
            }
        }
        {
            let resultados = self.resultados.lock().await;
            for (id, r) in resultados.iter() {
                infos.push((
                    // Archivadas sin marca temporal: van después de las vivas.
                    Instant::now(),
                    InfoConsola {
                        id_ejecucion: id.clone(),
                        comando: r.comando.clone(),
                        conversacion_id: Uuid::nil(),
                        viva: false,
                        codigo_salida: r.codigo_salida,
                        bytes: r.salida.len(),
                        // [219A-4] Archivadas sin dueño retenido: Agente.
                        origen: OrigenConsola::Agente,
                    },
                ));
            }
        }
        infos.sort_by_key(|(inicio, _)| *inicio);
        Ok(infos.into_iter().map(|(_, info)| info).collect())
    }

    async fn estado(&self, id_fondo: &str) -> Result<ResultadoEjecucionComando> {
        if let Some(r) = self.resultados.lock().await.get(id_fondo) {
            return Ok(r.clone());
        }
        // Viva: informar el comando real (F2; antes `String::new()`).
        if let Some(viva) = self.vivas.lock().await.get(id_fondo) {
            return Ok(ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(aún en ejecución)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id_fondo.to_string()),
                comando: viva.comando.clone(),
                id_ejecucion: id_fondo.to_string(),
            });
        }
        if self.tareas.lock().await.contains_key(id_fondo) {
            return Ok(ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(aún en ejecución)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id_fondo.to_string()),
                comando: String::new(),
                id_ejecucion: id_fondo.to_string(),
            });
        }
        Ok(ResultadoEjecucionComando {
            codigo_salida: None,
            salida: format!("(tarea de fondo desconocida: {id_fondo})"),
            truncada: false,
            fondo: true,
            id_fondo: Some(id_fondo.to_string()),
            comando: String::new(),
            id_ejecucion: id_fondo.to_string(),
        })
    }

    async fn matar(&self, id_fondo: &str) -> Result<()> {
        // `matar` NO toca `vivas` a propósito: el pump es el único que
        // retira y archiva (idempotente ante la carrera matar↔salida).
        self.matar_handle(id_fondo).await;
        Ok(())
    }

    async fn escribir(&self, id: &str, datos: &[u8]) -> Result<usize> {
        // [219A-3] Solo vivas de fondo: las transitorias síncronas no
        // retienen stdin y una terminada ya no tiene tubería (ambas →
        // `NoEncontrado`, la UI muestra el error honesto).
        if datos.len() > MAX_ESCRITURA_STDIN {
            return Err(Error::Limite(format!(
                "escritura a consola limitada a {MAX_ESCRITURA_STDIN} bytes por llamada"
            )));
        }
        let viva = self
            .vivas
            .lock()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| {
                Error::NoEncontrado(format!("consola desconocida o terminada: {id}"))
            })?;
        let mut guardia = viva.stdin.lock().await;
        let stdin = guardia.as_mut().ok_or_else(|| {
            Error::NoEncontrado(format!("consola sin stdin (ya terminó): {id}"))
        })?;
        stdin.write_all(datos).await.map_err(|_| {
            Error::NoEncontrado(format!("la consola ya terminó (tubería rota): {id}"))
        })?;
        stdin.flush().await.map_err(|_| {
            Error::NoEncontrado(format!("la consola ya terminó (tubería rota): {id}"))
        })?;
        Ok(datos.len())
    }

    async fn salida(&self, id: &str) -> Result<TranscriptConsola> {
        // [219A-3] Backfill de la UI: viva = volcado del anillo (con flujo);
        // archivada = líneas del resultado guardado (flujo stdout: el archivo
        // mezcla ambos). Acotado a `MAX_LINEAS_TRANSCRIPT`.
        if let Some(viva) = self.vivas.lock().await.get(id) {
            let anillo = viva.anillo.lock().await;
            let total = anillo.len();
            let desde = total.saturating_sub(MAX_LINEAS_TRANSCRIPT);
            return Ok(TranscriptConsola {
                id_ejecucion: id.to_string(),
                comando: viva.comando.clone(),
                viva: true,
                codigo_salida: None,
                lineas: anillo.iter().skip(desde).cloned().collect(),
                origen: viva.origen,
            });
        }
        if let Some(r) = self.resultados.lock().await.get(id) {
            let mut lineas: Vec<ChunkConsola> = r
                .salida
                .lines()
                .rev()
                .take(MAX_LINEAS_TRANSCRIPT)
                .map(|l| ChunkConsola {
                    flujo: FlujoConsola::Stdout,
                    linea: l.to_string(),
                })
                .collect();
            lineas.reverse();
            return Ok(TranscriptConsola {
                id_ejecucion: id.to_string(),
                comando: r.comando.clone(),
                viva: false,
                codigo_salida: r.codigo_salida,
                lineas,
                // [219A-4] Archivadas sin dueño retenido: Agente.
                origen: OrigenConsola::Agente,
            });
        }
        Err(Error::NoEncontrado(format!(
            "consola desconocida (el runner ya no la retiene): {id}"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comando_lento(segundos: u64) -> String {
        if cfg!(windows) {
            // `ping -n N` en Windows espera ~N-1 segundos y sale con código 0.
            // [139A-8 F1/K1] Sin `>nul`: la redirección la deniega la jaula
            // (la salida capturada la archiva el ejecutor igualmente).
            format!("ping -n {} 127.0.0.1", segundos + 1)
        } else {
            format!("sleep {segundos}")
        }
    }

    fn comando_mucho_eco() -> String {
        if cfg!(windows) {
            // [139A-8 F1/K1] El `for /L … do @echo` es sintaxis cmd y la
            // jaula lo deniega: `dir` de System32 (~5000 entradas, >200 KB)
            // fuerza el truncado en segundos sin shell ni redirección.
            r"dir C:\Windows\System32".to_string()
        } else {
            // Sin tubería (la jaula la deniega): 90 KB de NULes bastan para
            // forzar el truncado a 8 KB.
            "head -c 90000 /dev/zero".to_string()
        }
    }

    #[tokio::test]
    async fn sincrono_devuelve_salida_y_codigo() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar("echo hola-harness", false).await.unwrap();
        assert!(!r.fondo);
        assert_eq!(r.codigo_salida, Some(0));
        assert!(r.salida.contains("hola-harness"), "salida: {}", r.salida);
        assert!(!r.truncada);
    }

    #[tokio::test]
    async fn salida_larga_se_trunca_a_8kb() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_mucho_eco(), false).await.unwrap();
        assert_eq!(r.codigo_salida, Some(0));
        assert!(
            r.truncada,
            "salida inesperadamente corta: {} bytes",
            r.salida.len()
        );
        assert!(
            r.salida.len() <= LIMITE_SALIDA + 64,
            "longitud: {}",
            r.salida.len()
        );
        assert!(r.salida.contains("truncada"));
    }

    #[tokio::test]
    async fn fondo_devuelve_id_y_status_espera_resultado() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(3), true).await.unwrap();
        assert!(r.fondo);
        let id = r.id_fondo.expect("fondo debe devolver id");

        // Poll hasta que la tarea termine (máx. 15 s) y quede archivada.
        let mut resultado = None;
        for _ in 0..30 {
            let s = e.estado(&id).await.unwrap();
            if s.codigo_salida.is_some() {
                resultado = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let resultado = resultado.expect("la tarea de fondo debió terminar");
        assert_eq!(resultado.codigo_salida, Some(0));
        assert_eq!(resultado.id_fondo.as_deref(), Some(id.as_str()));
    }

    #[tokio::test]
    async fn matar_termina_la_tarea_de_fondo() {        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(60), true).await.unwrap();
        let id = r.id_fondo.expect("fondo debe devolver id");
        // Estado inmediato: debe seguir en ejecución (el comando dura ~60 s).
        let s = e.estado(&id).await.unwrap();
        assert!(
            s.codigo_salida.is_none(),
            "aún corriendo, salida: {}",
            s.salida
        );
        e.matar(&id).await.unwrap();
        // Tras matar, la tarea deja de estar "en ejecución" en ≤ 5 s.
        for _ in 0..10 {
            let s = e.estado(&id).await.unwrap();
            if !s.salida.contains("aún en ejecución") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("la tarea siguió reportando ejecución tras matar");
    }

    /// [209A-1 F2] La viva aparece en `lista()` con comando y conversación,
    /// y el pump la reapea al terminar (pasa a archivada).
    #[tokio::test]
    async fn fondo_registra_viva_visible_en_lista_y_reapea_al_terminar() {
        let e = EjecutorCliente::nuevo();
        let conv = Uuid::new_v4();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-viva-{}", Uuid::new_v4());
        let r = e
            .ejecutar_en_vivo(&id, &comando_lento(3), conv, true, tx)
            .await
            .unwrap();
        assert!(r.fondo);
        let vivas: Vec<_> =
            e.lista().await.unwrap().into_iter().filter(|i| i.viva).collect();
        assert_eq!(vivas.len(), 1, "una viva, lista: {:?}", e.lista().await.unwrap());
        assert_eq!(vivas[0].id_ejecucion, id);
        assert_eq!(vivas[0].conversacion_id, conv);
        assert!(vivas[0].comando.contains("ping") || vivas[0].comando.contains("sleep"));
        // Esperar el fin: la viva se reapea y queda archivada con código.
        let mut archivada = None;
        for _ in 0..30 {
            let infos = e.lista().await.unwrap();
            if infos.iter().all(|i| !i.viva) && !infos.is_empty() {
                archivada = infos.into_iter().find(|i| i.id_ejecucion == id);
                if archivada.is_some() {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let archivada = archivada.expect("la viva debió reapearse al terminar");
        assert!(!archivada.viva);
        assert_eq!(archivada.codigo_salida, Some(0));
    }

    /// [219A-3] `escribir` acepta bytes en una viva; `salida` vuelca el anillo
    /// (viva) o el resultado archivado; tras `matar`, stdin deja de existir.
    #[tokio::test]
    async fn escribir_acepta_en_viva_y_falla_tras_matar() {
        let e = EjecutorCliente::nuevo();
        let conv = Uuid::new_v4();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-stdin-{}", Uuid::new_v4());
        let r = e
            .ejecutar_en_vivo(&id, &comando_lento(25), conv, true, tx)
            .await
            .unwrap();
        assert!(r.fondo);
        let escritos = e.escribir(&id, b"hola\n").await.unwrap();
        assert_eq!(escritos, 5);
        let viva = e.salida(&id).await.unwrap();
        assert!(viva.viva);
        assert_eq!(viva.id_ejecucion, id);
        assert!(
            matches!(e.escribir("test-stdin-inexistente", b"x").await, Err(Error::NoEncontrado(_))),
            "id desconocido debe dar NoEncontrado"
        );
        e.matar(&id).await.unwrap();
        // Tras matar, el pump reapea y stdin deja de existir (≤ 5 s).
        let mut cerro = false;
        for _ in 0..10 {
            if matches!(e.escribir(&id, b"x").await, Err(Error::NoEncontrado(_))) {
                cerro = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        assert!(cerro, "stdin siguió aceptando tras matar");
        // Archivada: `salida` sigue disponible con el comando y sin viva.
        let fin = e.salida(&id).await.unwrap();
        assert!(!fin.viva);
        assert_eq!(fin.id_ejecucion, id);
    }

    /// [209A-1 F2] `desacoplar` no mata: la viva sigue corriendo y visible.
    #[tokio::test]
    async fn desacoplar_mantiene_la_viva_en_marcha() {
        let e = EjecutorCliente::nuevo();
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let id = format!("test-suelta-{}", Uuid::new_v4());
        e.ejecutar_en_vivo(&id, &comando_lento(60), Uuid::nil(), true, tx)
            .await
            .unwrap();
        e.desacoplar(&id).await.unwrap();
        // Sigue viva tras desacoplar…
        let sigue = e.lista().await.unwrap().into_iter().find(|i| i.id_ejecucion == id);
        assert!(sigue.is_some_and(|i| i.viva), "desacoplar no mata");
        // …y se puede matar igual (el pump archiva).
        e.matar(&id).await.unwrap();
        for _ in 0..10 {
            let s = e.estado(&id).await.unwrap();
            if !s.salida.contains("aún en ejecución") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("la viva siguió corriendo tras matar");
    }

    /// [209A-1 F2] El tope `MAX_CONSOLAS_VIVAS` rechaza la 5ª sin zombis.
    #[tokio::test]
    async fn tope_de_vivas_rechaza_la_quinta() {
        let e = EjecutorCliente::nuevo();
        let mut ids = Vec::new();
        for _ in 0..MAX_CONSOLAS_VIVAS {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            let id = format!("test-tope-{}", Uuid::new_v4());
            e.ejecutar_en_vivo(&id, &comando_lento(60), Uuid::nil(), true, tx)
                .await
                .unwrap();
            ids.push(id);
        }
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let err = e
            .ejecutar_en_vivo("test-tope-extra", &comando_lento(60), Uuid::nil(), true, tx)
            .await
            .expect_err("la 5ª viva debe rechazarse");
        assert!(matches!(err, Error::Limite(_)), "error: {err}");
        // Sin zombis: solo 4 vivas registradas.
        let vivas = e.lista().await.unwrap().into_iter().filter(|i| i.viva).count();
        assert_eq!(vivas, MAX_CONSOLAS_VIVAS);
        for id in ids {
            e.matar(&id).await.unwrap();
        }
    }

    /// [209A-1 F2] `desacoplar` de id desconocido da `NoEncontrado`.
    #[tokio::test]
    async fn desacoplar_id_desconocido_da_no_encontrado() {
        let e = EjecutorCliente::nuevo();
        let err = e
            .desacoplar("no-existe")
            .await
            .expect_err("id desconocido debe fallar");
        assert!(matches!(err, Error::NoEncontrado(_)), "error: {err}");
        assert!(e.lista().await.unwrap().is_empty());
    }

    /// [209A-1 F4] Reap por conversación: mata solo las vivas de `conv`;
    /// las de otras conversaciones siguen corriendo.
    #[tokio::test]
    async fn matar_por_conversacion_solo_mata_las_suyas() {
        let e = EjecutorCliente::nuevo();
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        for n in 0..2 {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            e.ejecutar_en_vivo(
                &format!("test-reap-a-{n}"),
                &comando_lento(60),
                conv_a,
                true,
                tx,
            )
            .await
            .unwrap();
        }
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        e.ejecutar_en_vivo("test-reap-b", &comando_lento(60), conv_b, true, tx)
            .await
            .unwrap();
        assert_eq!(e.matar_por_conversacion(conv_a).await, 2);
        // Las de A se reapean; la de B sigue viva.
        for _ in 0..20 {
            let infos = e.lista().await.unwrap();
            let vivas_a = infos.iter().filter(|i| i.viva && i.conversacion_id == conv_a).count();
            let viva_b = infos.iter().any(|i| i.viva && i.id_ejecucion == "test-reap-b");
            if vivas_a == 0 && viva_b {
                // Limpieza: no dejar la de B corriendo al salir del test.
                e.matar("test-reap-b").await.unwrap();
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        e.matar("test-reap-b").await.unwrap();
        panic!("el reap por conversación no retiró las vivas de A");
    }

    /// [209A-1 F4] Reap global: mata todas las vivas; repetir en vacío
    /// devuelve 0 sin error (idempotente, apto para el cierre de app).
    #[tokio::test]
    async fn matar_todas_vacia_las_vivas_y_en_vacio_da_cero() {
        let e = EjecutorCliente::nuevo();
        for n in 0..2 {
            let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
            e.ejecutar_en_vivo(
                &format!("test-reap-todas-{n}"),
                &comando_lento(60),
                Uuid::new_v4(),
                true,
                tx,
            )
            .await
            .unwrap();
        }
        assert_eq!(e.matar_todas().await, 2);
        for _ in 0..20 {
            let vivas = e.lista().await.unwrap().into_iter().filter(|i| i.viva).count();
            if vivas == 0 {
                assert_eq!(e.matar_todas().await, 0);
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("matar_todas no vació las vivas");
    }

    /// [119A-7 F0] Humo de la jaula: el hijo arranca con cwd = la raíz
    /// enjaulada (`en_raiz`), no con el cwd del proceso de test.
    #[tokio::test]
    async fn en_raiz_arranca_los_comandos_en_la_jaula() {
        let jaula = std::env::temp_dir().join(format!(
            "gh-jaula-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("reloj")
                .as_nanos()
        ));
        std::fs::create_dir_all(&jaula).expect("crear jaula");
        let canonica = jaula.canonicalize().expect("canonizar jaula");
        let e = EjecutorCliente::en_raiz(jaula.clone());
        // `cd` (Windows) / `pwd` (unix) reportan el cwd del hijo.
        let sonda = if cfg!(windows) { "cd" } else { "pwd" };
        let r = e.ejecutar(sonda, false).await.expect("sonda cwd");
        assert_eq!(r.codigo_salida, Some(0));
        // Windows: `canonicalize` devuelve ruta verbatim (`\\?\C:\...`)
        // mientras `cd` imprime `C:\...`; además el FS no distingue
        // mayúsculas. Se normaliza por ambos lados antes de comparar.
        let normalizar = |s: &str| {
            s.strip_prefix(r"\\?\")
                .unwrap_or(s)
                .replace('/', "\\")
                .to_lowercase()
        };
        let salida = normalizar(r.salida.trim());
        let esperada = normalizar(&canonica.to_string_lossy());
        assert!(
            salida.contains(&esperada),
            "el hijo arranca en la jaula '{esperada}', salida: {}",
            r.salida.trim()
        );
        std::fs::remove_dir_all(&jaula).ok();
    }
}
