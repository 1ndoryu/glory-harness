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
//! - `tools` → lista las tools agnósticas del núcleo.
//! - `doctor` → comprueba configuración (envs de proveedores) y salida.
//! - `--version`/`-V` → versión del binario + contrato core.

use glory_harness::cargar_env_usuario;
use glory_harness::{chat, daemon, ejecutor, persistencia, run, tui};
use glory_harness_core::{HarnessError, ProgramadorTareas};
use std::process::ExitCode;
use std::sync::Arc;
use uuid::Uuid;

fn main() -> ExitCode {
    // Carga opcional de claves LLM desde ~/.glory-harness.env (para que el
    // binario funcione "desde cualquier carpeta" sin depender del .env de un
    // proyecto). Solo define variables que aún no existan en el entorno.
    cargar_env_usuario();

    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version" | "-V") => {
            println!(
                "glory-harness {} (contrato core {})",
                env!("CARGO_PKG_VERSION"),
                glory_harness_core::CONTRATO_VERSION,
            );
            ExitCode::SUCCESS
        }
        Some("run") => {
            let prompt = extraer_opcion(&args, &["--prompt", "--mensaje", "-p"]);
            let usa_stdin = args.iter().any(|a| a == "--stdin");
            let opciones = run::OpcionesRun {
                provider: extraer_opcion(&args, &["--provider", "--proveedor"]),
                modelo: extraer_opcion(&args, &["--modelo", "--model"]),
                dir: extraer_opcion(&args, &["--dir", "--cwd", "--workspace"]).map(std::path::PathBuf::from),
                modo: extraer_opcion(&args, &["--modo"]),
                /* [039A-1 04-09 H7] El CLI run no expone flag de razonamiento:
                 * deja el default del proveedor (None → config intacta). */
                razonamiento: None,
            };
            let prompt = if let Some(p) = prompt {
                Some(p)
            } else if usa_stdin {
                leer_stdin()
            } else {
                None
            };
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(run::run(prompt, opciones)),
                Err(e) => {
                    eprintln!("glory-harness run: no se pudo iniciar el runtime tokio: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("chat") => {
            let opciones = run::OpcionesRun {
                provider: extraer_opcion(&args, &["--provider", "--proveedor"]),
                modelo: extraer_opcion(&args, &["--modelo", "--model"]),
                dir: extraer_opcion(&args, &["--dir", "--cwd", "--workspace"]).map(std::path::PathBuf::from),
                modo: extraer_opcion(&args, &["--modo"]),
                /* [039A-1 04-09 H7] El CLI chat/tui no expone flag de
                 * razonamiento: deja el default del proveedor. */
                razonamiento: None,
            };
            let usa_tui = args.iter().any(|a| a == "--tui");
            match tokio::runtime::Runtime::new() {
                Ok(rt) => {
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
                }
                Err(e) => {
                    eprintln!("glory-harness chat: no se pudo iniciar el runtime tokio: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("daemon") => {
            let puerto = extraer_opcion(&args, &["--puerto", "--port"])
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8798);
            let mostrar = args.iter().any(|a| a == "--mostrar-token");
            match tokio::runtime::Runtime::new() {
                Ok(rt) => rt.block_on(daemon::run(puerto, mostrar)),
                Err(e) => {
                    eprintln!("glory-harness daemon: no se pudo iniciar el runtime tokio: {e}");
                    ExitCode::from(1)
                }
            }
        }
        Some("schedule") => cmd_schedule(&args[1..]),
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
            eprintln!("uso: glory-harness <run|chat|daemon|schedule|tools|doctor|--version>");
            ExitCode::from(2)
        }
        None => {
            eprintln!("glory-harness: falta subcomando (run|chat|daemon|schedule|tools|doctor|--version)");
            ExitCode::from(2)
        }
    }
}

/// `glory-harness schedule <list|create|remove|logs>`: administra las tareas
/// programadas del CLI standalone (store en memoria, [318A-16 F6]). Usa el
/// mismo puerto [`ProgramadorTareas`] que registra la tool `programar_tarea`
/// en las sesiones de `chat`/`daemon`, y la traducción NL→cron pura del
/// núcleo. El worker de producción lo corre el consumidor (PT ya tiene cron +
/// heartbeat); este subcomando expone la cara CRUD para el proceso actual.
/// Resultado del subcomando `schedule`: `Uso` = error de argumentos (exit 2).
enum SalidaSchedule {
    Ok,
    Uso,
}

fn cmd_schedule(args: &[String]) -> ExitCode {
    use glory_harness_core::ProgramadorTareas;
    use std::sync::Arc;
    use uuid::Uuid;

    let Some(accion) = args.first().map(String::as_str) else {
        eprintln!("uso: glory-harness schedule <list|create|remove|logs>");
        return ExitCode::from(2);
    };
    let programador: Arc<dyn ProgramadorTareas> =
        Arc::new(persistencia::ProgramadorMemoria::nuevo());
    let user_id = Uuid::new_v4();

    let resultado = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(cmd_schedule_impl(accion, args, programador, user_id)),
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

/// Cuerpo asíncrono del subcomando (extraído a `async fn` porque un bloque
/// `async move` en línea no admite anotación de tipo de retorno).
async fn cmd_schedule_impl(
    accion: &str,
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    use glory_harness_core::ports::NuevaTareaProgramada;
    use glory_harness_core::tareas::frase_a_cron;

    match accion {
                "list" | "listar" => {
                    let tareas = programador.tareas_listar(user_id).await?;
                    if tareas.is_empty() {
                        println!("(sin tareas programadas en este proceso)");
                    } else {
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
                    }
                    Ok(SalidaSchedule::Ok)
                }
                "create" | "crear" => {
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
                    let proxima =
                        glory_harness_core::scheduler::proxima_ejecucion(&cron, chrono::Utc::now())?;
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
                "remove" | "cancelar" => {
                    let id_texto: String = match args.get(1) {
                        Some(v) => v.clone(),
                        None => match extraer_opcion(args, &["--id"]) {
                            Some(v) => v,
                            None => {
                                eprintln!("uso: glory-harness schedule remove <id>");
                                return Ok(SalidaSchedule::Uso);
                            }
                        },
                    };
                    let id = match Uuid::parse_str(&id_texto) {
                        Ok(id) => id,
                        Err(_) => {
                            eprintln!("schedule remove: id inválido '{id_texto}'");
                            return Ok(SalidaSchedule::Uso);
                        }
                    };
                    if programador.tarea_cancelar(id, user_id).await? {
                        println!("tarea {id} cancelada");
                        Ok(SalidaSchedule::Ok)
                    } else {
                        eprintln!("schedule remove: no existe la tarea {id}");
                        Ok(SalidaSchedule::Uso)
                    }
                }
                "logs" => {
                    let id_texto: String = match args.get(1) {
                        Some(v) => v.clone(),
                        None => match extraer_opcion(args, &["--id"]) {
                            Some(v) => v,
                            None => {
                                eprintln!("uso: glory-harness schedule logs <id>");
                                return Ok(SalidaSchedule::Uso);
                            }
                        },
                    };
                    let id = match Uuid::parse_str(&id_texto) {
                        Ok(id) => id,
                        Err(_) => {
                            eprintln!("schedule logs: id inválido '{id_texto}'");
                            return Ok(SalidaSchedule::Uso);
                        }
                    };
                    let registros = programador.tarea_logs(id, user_id, 10).await?;
                    if registros.is_empty() {
                        println!("(la tarea {id} no tiene ejecuciones registradas)");
                    } else {
                        for r in &registros {
                            let estado = if r.ok { "ok" } else { "fallo" };
                            println!(
                                "{} [{estado}] {}",
                                r.ejecutada_en.format("%Y-%m-%d %H:%M UTC"),
                                r.resumen
                            );
                        }
                    }
                    Ok(SalidaSchedule::Ok)
                }
                otra => {
                    eprintln!("schedule: acción desconocida '{otra}' (list|create|remove|logs)");
                    Ok(SalidaSchedule::Uso)
                }
    }
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
            if t.is_empty() { None } else { Some(t) }
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