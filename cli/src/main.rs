//! Binario `glory-harness` (Fases 3-5): subcomandos `run`, `chat`, `daemon`,
//! `schedule`, `tools`, `doctor` y `--version`.
//!
//! - `run [--prompt "..." | --stdin] [--provider P] [--modelo M] [--dir R]`
//!   → un turno one-shot. Trabaja en la carpeta actual (o `--dir`); usa por
//!   defecto Laguna S 2.1 free (`commandcode/poolside/laguna-s-2.1-free`) y
//!   salta a gloryapi/otros si falla.
//! - `chat [--provider P] [--modelo M] [--dir R] [--tui]` → sesión interactiva
//!   que mantiene la misma conversación entre turnos. Default: REPL lineal
//!   `gh> ` (comandos `/salir`, `/nuevo`, `/ayuda`; Ctrl+C o EOF terminan).
//!   Con `--tui`: pantalla completa ratatui (paneles mensajes/estado/entrada).
//! - `daemon [--puerto N] [--mostrar-token]` → proceso de fondo NDJSON en
//!   `127.0.0.1`, multi-sesión, token obligatorio.
//! - `session <list|ver|resume|borrar> [id]` → gestiona las conversaciones
//!   durables del CLI (misma BD y usuario que `chat`); `resume` reabre el REPL
//!   sobre una conversación anterior ([069A-2]).
//! - `tools` → lista las tools agnósticas del núcleo.
//! - `doctor` → comprueba configuración (envs de proveedores) y salida.
//! - `--version`/`-V` → versión del binario + contrato core.

use chrono::{DateTime, Utc};
use glory_harness::cargar_env_usuario;
use glory_harness::{chat, daemon, ejecutor, persistencia, run, sesion, tui, PersistenciaSqlite};
use glory_harness_core::ports::{TareaProgramada, TareaProgramadaPendiente};
use glory_harness_core::{AgentPersistence, HarnessError, ProgramadorTareas};
use std::process::ExitCode;
use std::sync::Arc;
use uuid::Uuid;

fn main() -> ExitCode {
    // Carga opcional de claves LLM desde ~/.glory-harness.env (para que el
    // binario funcione "desde cualquier carpeta" sin depender del .env de un
    // proyecto). Solo define variables que aún no existan en el entorno.
    cargar_env_usuario();
    despachar(std::env::args().skip(1).collect())
}

/// [059A-22] Despacho de subcomandos, extraído de `fn main` (que queda como
/// stub de entrada por debajo del límite de 100 líneas efectivas del gate).
fn despachar(args: Vec<String>) -> ExitCode {
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!(
                "glory-harness {} (contrato core {})",
                env!("CARGO_PKG_VERSION"),
                glory_harness_core::CONTRATO_VERSION,
            );
            ExitCode::SUCCESS
        }
        /* [059A-20 05-09] `--help`/`-h` se caían al brazo de subcomando
         * desconocido (exit 2). Ayuda explícita a stdout con exit 0. */
        Some("--help" | "-h") => {
            imprimir_ayuda();
            ExitCode::SUCCESS
        }
        Some("run") => {
            let prompt = extraer_opcion(&args, &["--prompt", "--mensaje", "-p"]);
            let usa_stdin = args.iter().any(|a| a == "--stdin");
            let opciones = run::OpcionesRun {
                provider: extraer_opcion(&args, &["--provider", "--proveedor"]),
                modelo: extraer_opcion(&args, &["--modelo", "--model"]),
                dir: extraer_opcion(&args, &["--dir", "--cwd", "--workspace"])
                    .map(std::path::PathBuf::from),
                modo: extraer_opcion(&args, &["--modo"]),
                /* [039A-1 04-09 H7] El CLI run no expone flag de razonamiento:
                 * deja el default del proveedor (None → config intacta). */
                razonamiento: None,
                /* [039A-3 P6-backend] El CLI no inyecta ventana: None → default
                 * del core (128k, intacto). Solo el desktop inyecta (150k). */
                max_ventana: None,
            };
            let prompt = if let Some(p) = prompt {
                Some(p)
            } else if usa_stdin {
                leer_stdin()
            } else {
                None
            };
            con_runtime("run", |rt| rt.block_on(run::run(prompt, opciones)))
        }
        Some("chat") => {
            let opciones = run::OpcionesRun {
                provider: extraer_opcion(&args, &["--provider", "--proveedor"]),
                modelo: extraer_opcion(&args, &["--modelo", "--model"]),
                dir: extraer_opcion(&args, &["--dir", "--cwd", "--workspace"])
                    .map(std::path::PathBuf::from),
                modo: extraer_opcion(&args, &["--modo"]),
                /* [039A-1 04-09 H7] El CLI chat/tui no expone flag de
                 * razonamiento: deja el default del proveedor. */
                razonamiento: None,
                /* [039A-3 P6-backend] Sin inyección de ventana en CLI (None →
                 * default del core 128k). */
                max_ventana: None,
            };
            let usa_tui = args.iter().any(|a| a == "--tui");
            con_runtime("chat", |rt| {
                let res = if usa_tui {
                    rt.block_on(tui::tui(opciones))
                } else {
                    rt.block_on(chat::chat(opciones))
                };
                match res {
                    Ok(()) => ExitCode::SUCCESS,
                    Err(e) => {
                        eprintln!("glory-harness chat: {e}");
                        ExitCode::from(1)
                    }
                }
            })
        }
        Some("daemon") => {
            let puerto = extraer_opcion(&args, &["--puerto", "--port"])
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8798);
            let mostrar = args.iter().any(|a| a == "--mostrar-token");
            con_runtime("daemon", |rt| rt.block_on(daemon::run(puerto, mostrar)))
        }
        Some("schedule") => cmd_schedule(&args[1..]),
        Some("session") => cmd_session(&args[1..]),
        Some("tools") => {
            listar_tools();
            ExitCode::SUCCESS
        }
        Some("doctor") => {
            doctor();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("glory-harness: subcomando desconocido '{other}'");
            eprintln!("uso: glory-harness <run|chat|daemon|schedule|session|tools|doctor|--version>");
            ExitCode::from(2)
        }
        None => {
            eprintln!(
                "glory-harness: falta subcomando (run|chat|daemon|schedule|session|tools|doctor|--version)"
            );
            ExitCode::from(2)
        }
    }
}

/// [059A-22] Ejecuta `f` con un runtime tokio nuevo; ante fallo imprime el
/// error con el nombre del subcomando y sale con código 1 (antes era un match
/// duplicado en cada brazo run/chat/daemon).
fn con_runtime<F>(nombre: &str, f: F) -> ExitCode
where
    F: FnOnce(&tokio::runtime::Runtime) -> ExitCode,
{
    match tokio::runtime::Runtime::new() {
        Ok(rt) => f(&rt),
        Err(e) => {
            eprintln!("glory-harness {nombre}: no se pudo iniciar el runtime tokio: {e}");
            ExitCode::from(1)
        }
    }
}

/// [059A-22] Texto de `--help`, extraído del brazo para acotar `despachar`.
fn imprimir_ayuda() {
    println!("uso: glory-harness <run|chat|daemon|schedule|session|tools|doctor|--version>");
    println!();
    println!("  run       turno único (--prompt/--stdin/--dir/--provider/--modelo/--modo)");
    println!("  chat      sesión interactiva; --tui para la interfaz enriquecida");
    println!("  daemon    servicio de fondo por NDJSON (consumidor-daemon.mjs)");
    println!("  schedule  tareas programadas: <list|create|remove|logs|run>");
    println!("  session   conversaciones: <list|ver|resume|borrar> [id]");
    println!("  tools     tools disponibles del núcleo");
    println!("  doctor    diagnóstico de configuración y proveedores");
    println!("  --version versión del CLI y del contrato core");
}

/// `glory-harness schedule <list|create|remove|logs|run>`: administra y
/// ejecuta las tareas programadas del CLI standalone sobre la BD durable
/// (`%APPDATA%/glory-harness/glory-harness.db`, [B3-F8a]). `PersistenciaSqlite`
/// implementa AMBAS caras (CRUD [`ProgramadorTareas`] + cola del scheduler en
/// [`AgentPersistence`]), de modo que `create` en un proceso y `run` en otro
/// comparten las tareas; el `user_id` es estable (tabla `config`). `run`
/// ejecuta las vencidas como turnos del agente y entrega el resumen en
/// `tarea_logs` (núcleo `cron::ejecutar_lista`, claim fence por tarea).
/// Resultado del subcomando `schedule`: `Uso` = error de argumentos (exit 2).
enum SalidaSchedule {
    Ok,
    Uso,
}

/// Abre la BD de la app y resuelve el usuario estable del CLI (se crea una
/// vez en `config` y se reutiliza: las tareas sobreviven a los procesos).
/// [069A-2] Delegación al helper compartido de `run` (misma tienda y usuario
/// que `chat`/`session`).
fn abrir_tiendas_schedule() -> Result<(Arc<PersistenciaSqlite>, Uuid), HarnessError> {
    run::abrir_tiendas_durables().map_err(HarnessError::Persistencia)
}

/// Filtro puro de vencimiento: pendientes con próxima pasada (las
/// desprogramadas —`proxima` ausente— y las canceladas/completadas no tocan).
fn seleccionar_vencidas(
    tareas: &[TareaProgramada],
    ahora: DateTime<Utc>,
) -> Vec<TareaProgramadaPendiente> {
    tareas
        .iter()
        .filter(|t| {
            t.estado == "pendiente" && t.proxima_ejecucion.map(|p| p <= ahora).unwrap_or(false)
        })
        .map(|t| TareaProgramadaPendiente {
            id: t.id,
            user_id: t.user_id,
            nombre: t.nombre.clone(),
            prompt: t.prompt.clone(),
            tipo: t.tipo.clone(),
            cron_expr: t.cron_expr.clone(),
        })
        .collect()
}

fn cmd_schedule(args: &[String]) -> ExitCode {
    let Some(accion) = args.first().map(String::as_str) else {
        eprintln!("uso: glory-harness schedule <list|create|remove|logs|run>");
        return ExitCode::from(2);
    };
    let (tiendas, user_id) = match abrir_tiendas_schedule() {
        Ok(par) => par,
        Err(e) => {
            eprintln!("glory-harness schedule: {e}");
            return ExitCode::from(1);
        }
    };

    let resultado = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(cmd_schedule_impl(accion, args, &tiendas, user_id)),
        Err(e) => {
            eprintln!("glory-harness schedule: no se pudo iniciar el runtime tokio: {e}");
            return ExitCode::from(1);
        }
    };
    match resultado {
        Ok(SalidaSchedule::Ok) => ExitCode::SUCCESS,
        Ok(SalidaSchedule::Uso) => ExitCode::from(2),
        Err(e) => {
            eprintln!("glory-harness schedule: {e}");
            ExitCode::from(1)
        }
    }
}

/// `glory-harness session <list|ver|resume|borrar> [id]` ([069A-2]):
/// gestiona las conversaciones durables del CLI (misma BD sqlite y `user_id`
/// estable que `chat`/`schedule`). `resume` necesita provider/modelo/dir
/// como `chat` (reabre el REPL sobre la conversación); el resto solo toca la
/// BD. Reutiliza el patrón `cmd_schedule`: `Uso` → exit 2, fallo → exit 1.
fn cmd_session(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("uso: glory-harness session <list|ver|resume|borrar> [id]");
        return ExitCode::from(2);
    }
    let opciones = run::OpcionesRun {
        provider: extraer_opcion(args, &["--provider", "--proveedor"]),
        modelo: extraer_opcion(args, &["--modelo", "--model"]),
        dir: extraer_opcion(args, &["--dir", "--cwd", "--workspace"])
            .map(std::path::PathBuf::from),
        modo: extraer_opcion(args, &["--modo"]),
        razonamiento: None,
        max_ventana: None,
    };
    let resultado = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(sesion::sesion(args, opciones)),
        Err(e) => {
            eprintln!("glory-harness session: no se pudo iniciar el runtime tokio: {e}");
            return ExitCode::from(1);
        }
    };
    use glory_harness::sesion::SalidaSesion;
    match resultado {
        Ok(SalidaSesion::Ok) => ExitCode::SUCCESS,
        Ok(SalidaSesion::Uso) => {
            eprintln!("uso: glory-harness session <list|ver|resume|borrar> [id]");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("glory-harness session: {e}");
            ExitCode::from(1)
        }
    }
}

/// Cuerpo asíncrono del subcomando (extraído a `async fn` porque un bloque
/// `async move` en línea no admite anotación de tipo de retorno).
async fn cmd_schedule_impl(
    accion: &str,
    args: &[String],
    tiendas: &Arc<PersistenciaSqlite>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let programador: Arc<dyn ProgramadorTareas> = tiendas.clone();
    match accion {
        "list" | "listar" => accion_listar(programador, user_id).await,
        "create" | "crear" => accion_crear(args, programador, user_id).await,
        "remove" | "cancelar" => accion_remove(args, programador, user_id).await,
        "logs" => accion_logs(args, programador, user_id).await,
        "run" | "ejecutar" => accion_run(args, tiendas, user_id).await,
        otra => {
            eprintln!("schedule: acción desconocida '{otra}' (list|create|remove|logs|run)");
            Ok(SalidaSchedule::Uso)
        }
    }
}

/// `schedule run [--limite N]`: ejecuta las tareas vencidas como turnos del
/// agente y entrega el resumen en `tarea_logs` ([B3-F8a]). Construye el
/// harness sobre la misma BD (sin MCP en v1: registry base; documentado como
/// límite). Sin claves LLM los turnos fallan con error presentable y la
/// tarea queda pendiente con el fallo registrado (reintento visible).
async fn accion_run(
    args: &[String],
    tiendas: &Arc<PersistenciaSqlite>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let limite = extraer_opcion(args, &["--limite", "--limit"])
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(1, 100);
    let persistencia: Arc<dyn AgentPersistence> = tiendas.clone();
    let programador: Arc<dyn ProgramadorTareas> = tiendas.clone();
    // Recupera ejecuciones interrumpidas (proceso muerto a mitad de turno)
    // antes de seleccionar: sin esto una tarea 'ejecutando' huérfana no
    // vuelve a tocar nunca (hermes `recover_abandoned`).
    let recuperadas = persistencia.tareas_recuperar_interrumpidas().await?;
    if recuperadas > 0 {
        println!("schedule run: {recuperadas} interrumpida(s) recuperada(s)");
    }
    let todas = programador.tareas_listar(user_id).await?;
    let mut vencidas = seleccionar_vencidas(&todas, Utc::now());
    vencidas.truncate(limite as usize);
    if vencidas.is_empty() {
        println!("(sin tareas vencidas)");
        return Ok(SalidaSchedule::Ok);
    }
    let opciones = run::OpcionesRun {
        provider: extraer_opcion(args, &["--provider", "--proveedor"]),
        modelo: extraer_opcion(args, &["--modelo", "--model"]),
        dir: None,
        modo: None,
        razonamiento: None,
        max_ventana: None,
    };
    let harness = run::construir_harness_con(
        &opciones,
        persistencia.clone(),
        programador.clone(),
        user_id,
    );
    for v in &vencidas {
        eprintln!("schedule run: ejecutando {} [{}] …", v.id, v.nombre);
    }
    let resumen = glory_harness_core::cron::ejecutar_lista(
        &harness.runtime,
        &persistencia,
        &programador,
        &vencidas,
    )
    .await?;
    println!(
        "schedule run: vencidas={} ejecutadas={} fallidas={} omitidas={} reprogramadas={}",
        resumen.pendientes,
        resumen.ejecutadas,
        resumen.fallidas,
        resumen.omitidas,
        resumen.reprogramadas,
    );
    Ok(SalidaSchedule::Ok)
}

/// `schedule list`: imprime las tareas programadas del usuario estable.
async fn accion_listar(
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let tareas = programador.tareas_listar(user_id).await?;
    if tareas.is_empty() {
        println!("(sin tareas programadas)");
        return Ok(SalidaSchedule::Ok);
    }
    for t in &tareas {
        let proxima = t
            .proxima_ejecucion
            .map(|p| p.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "—".into());
        println!(
            "{} [{}] cron='{}' próximo='{proxima}' estado={} \"{}\"",
            t.id,
            t.nombre,
            t.cron_expr.as_deref().unwrap_or("—"),
            t.estado,
            t.prompt
        );
    }
    Ok(SalidaSchedule::Ok)
}

/// `schedule create`: valida argumentos, traduce NL→cron y registra la tarea.
async fn accion_crear(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    use glory_harness_core::ports::NuevaTareaProgramada;
    use glory_harness_core::tareas::frase_a_cron;

    let nombre = extraer_opcion(args, &["--nombre", "--name"]);
    let prompt = extraer_opcion(args, &["--prompt", "--mensaje"]);
    let cuando = extraer_opcion(args, &["--cuando", "--cron", "--programacion"]);
    let (Some(nombre), Some(prompt), Some(cuando)) = (nombre, prompt, cuando) else {
        eprintln!("uso: glory-harness schedule create --nombre <n> --prompt <p> --cuando \"cada lunes a las 9\"");
        return Ok(SalidaSchedule::Uso);
    };
    let cron = match frase_a_cron(&cuando) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("schedule create: {e}");
            return Ok(SalidaSchedule::Uso);
        }
    };
    let proxima = glory_harness_core::scheduler::proxima_ejecucion(&cron, chrono::Utc::now())?;
    let id = programador
        .tarea_crear(&NuevaTareaProgramada {
            user_id,
            nombre,
            prompt,
            tipo: "recurrente".into(),
            cron_expr: cron.clone(),
            proxima_ejecucion: proxima,
        })
        .await?;
    println!(
        "tarea creada: {id} — cron '{cron}' — próxima ejecución {}",
        proxima.format("%Y-%m-%d %H:%M UTC")
    );
    Ok(SalidaSchedule::Ok)
}

/// Lee el id de tarea de `args` (posicional o `--id`) o imprime el uso.
fn id_desde_args(args: &[String], comando: &str) -> Result<Uuid, SalidaSchedule> {
    let id_texto: String = match args.get(1) {
        Some(v) => v.clone(),
        None => match extraer_opcion(args, &["--id"]) {
            Some(v) => v,
            None => {
                eprintln!("uso: glory-harness schedule {comando} <id>");
                return Err(SalidaSchedule::Uso);
            }
        },
    };
    match Uuid::parse_str(&id_texto) {
        Ok(id) => Ok(id),
        Err(_) => {
            eprintln!("schedule {comando}: id inválido '{id_texto}'");
            Err(SalidaSchedule::Uso)
        }
    }
}

/// `schedule remove`: cancela la tarea indicada.
async fn accion_remove(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let id = match id_desde_args(args, "remove") {
        Ok(id) => id,
        Err(salida) => return Ok(salida),
    };
    if programador.tarea_cancelar(id, user_id).await? {
        println!("tarea {id} cancelada");
        Ok(SalidaSchedule::Ok)
    } else {
        eprintln!("schedule remove: no existe la tarea {id}");
        Ok(SalidaSchedule::Uso)
    }
}

/// `schedule logs`: imprime las últimas ejecuciones de la tarea indicada.
async fn accion_logs(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let id = match id_desde_args(args, "logs") {
        Ok(id) => id,
        Err(salida) => return Ok(salida),
    };
    let registros = programador.tarea_logs(id, user_id, 10).await?;
    if registros.is_empty() {
        println!("(la tarea {id} no tiene ejecuciones registradas)");
        return Ok(SalidaSchedule::Ok);
    }
    for r in &registros {
        let estado = if r.ok { "ok" } else { "fallo" };
        println!(
            "{} [{estado}] {}",
            r.ejecutada_en.format("%Y-%m-%d %H:%M UTC"),
            r.resumen
        );
    }
    Ok(SalidaSchedule::Ok)
}

/// Lee el valor de la primera opción que coincida con uno de `nombres`,
/// devolviendo el siguiente argumento. `None` si no aparece.
fn extraer_opcion(args: &[String], nombres: &[&str]) -> Option<String> {
    for (i, arg) in args.iter().enumerate() {
        if nombres.contains(&arg.as_str()) {
            return args.get(i + 1).cloned();
        }
    }
    None
}

/// Lee todo stdin como prompt (modo pipeline: `echo "x" | glory-harness run --stdin`).
fn leer_stdin() -> Option<String> {
    use std::io::Read;
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map(|_| {
            let t = buf.trim().to_string();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
        .unwrap_or(None)
}

/// `doctor`: valida la configuración del entorno (proveedores LLM) y reporta
/// el estado. No toca red ni requiere gate; es una comprobación local.
fn doctor() {
    let llaves = glory_harness_core::llm::LlavesProveedor::from_env();
    let con_key = [
        ("cerebras", llaves.cerebras.len()),
        ("groq", llaves.groq.len()),
        ("deepseek", llaves.deepseek.len()),
        ("glory/empero", llaves.glory.len()),
        ("commandcode", llaves.commandcode.len()),
    ]
    .into_iter()
    .map(|(nombre, n)| format!("  {nombre}: {} clave(s)", n))
    .collect::<Vec<_>>()
    .join("\n");
    let proveedores = llaves.cerebras.len()
        + llaves.groq.len()
        + llaves.deepseek.len()
        + llaves.glory.len()
        + llaves.commandcode.len();
    println!("glory-harness doctor");
    println!("  contrato core: {}", glory_harness_core::CONTRATO_VERSION);
    println!("  proveedores LLM (claves en env):\n{con_key}");
    if proveedores == 0 {
        eprintln!(
            "  AVISO: no hay claves LLM en el entorno (CEREBRAS_API_KEY, GROQ_API, \
             DEEPSEEK_API, GLORY_API_KEY, COMMAND_CODE_API_KEY). 'run' fallará sin una."
        );
    } else {
        println!("  total: {proveedores} clave(s) disponibles");
    }
}

/// Lista las tools agnósticas del núcleo (las que el runtime registra en cada
/// runtime nuevo: web_search siempre; file_* solo con sandbox local).
fn listar_tools() {
    use std::sync::Arc;

    use glory_harness_core::runtime::AgentRuntime;
    use glory_harness_core::tool::AgentToolRegistry;

    print!("tools agnósticas del núcleo: ");
    let persistencia = Arc::new(persistencia::PersistenciaMemoria::nuevo());
    let persistencia_port: Arc<dyn glory_harness_core::AgentPersistence> = persistencia.clone();
    let llm = Arc::new(glory_harness_core::llm::LlmProviderService::new(
        glory_harness_core::llm::LlavesProveedor::default(),
    ));
    let puertos = glory_harness_core::runtime::PuertosHarness {
        persistencia: persistencia_port,
        llm,
        web_search: None,
        web_fetch: None,
        dominio: None,
        ejecutor_comando: Some(Arc::new(ejecutor::EjecutorCliente::nuevo())),
        programador_tareas: Some(Arc::new(persistencia::ProgramadorMemoria::nuevo())),
    };
    let runtime = AgentRuntime::nuevo(
        AgentToolRegistry::new(),
        puertos,
        glory_harness_core::runtime::TurnoConfig::default(),
    );
    let ids = runtime.tools_registradas();
    if ids.is_empty() {
        println!("(ninguna)");
    } else {
        println!("{}", ids.join(", "));
    }
    println!("  (file_* requiere AGENTE_MODO=local + workspace accesible; web_search siempre)");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tarea(estado: &str, proxima: Option<DateTime<Utc>>) -> TareaProgramada {
        TareaProgramada {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            nombre: "t".into(),
            prompt: "p".into(),
            tipo: "recurrente".into(),
            cron_expr: Some("0 9 * * *".into()),
            proxima_ejecucion: proxima,
            estado: estado.into(),
            creado_en: Utc::now(),
        }
    }

    #[test]
    fn vencidas_solo_pendientes_con_proxima_pasada() {
        let ahora = Utc::now();
        let pasado = ahora - chrono::Duration::hours(1);
        let futuro = ahora + chrono::Duration::hours(1);
        let tareas = vec![
            tarea("pendiente", Some(pasado)),
            tarea("pendiente", Some(futuro)),
            tarea("pendiente", None),
            tarea("cancelada", Some(pasado)),
            tarea("completada", Some(pasado)),
        ];
        let vencidas = seleccionar_vencidas(&tareas, ahora);
        assert_eq!(vencidas.len(), 1, "solo la pendiente vencida toca");
        assert_eq!(vencidas[0].id, tareas[0].id);
    }
}
