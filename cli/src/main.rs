//! Binario `glory-harness` (Fases 3-5): subcomandos `run`, `chat`, `daemon`,
//! `web`, `schedule`, `session`, `tools`, `doctor` y `--version`.
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
//! - `web [--puerto N] [--dir-ui R] [--fixture]` → servidor HTTP loopback
//!   con SSE para el modo web local (`http://127.0.0.1:8799`).
//! - `session <list|ver|resume|borrar> [id]` → gestiona las conversaciones
//!   durables del CLI (misma BD y usuario que `chat`); `resume` reabre el REPL
//!   sobre una conversación anterior ([069A-2]).
//! - `tools` → lista las tools agnósticas del núcleo.
//! - `doctor` → comprueba configuración (envs de proveedores) y salida.
//! - `--version`/`-V` → versión del binario + contrato core.

use glory_harness::cargar_env_usuario;
use glory_harness::{chat, daemon, ejecutor, persistencia, run, sesion, tui, web};
use std::process::ExitCode;

mod schedule_cmd;

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
                /* [069A-3] Toast de Windows al terminar el turno o pedir un
                 * permiso (solo CLI interactivo, tras flag explícito). */
                notificar: args.iter().any(|a| a == "--notificar"),
                navegador: None,
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
                /* [069A-3] Como en `run` (vale también para `session resume`,
                 * que reabre este mismo REPL). */
                notificar: args.iter().any(|a| a == "--notificar"),
                navegador: None,
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
        /* [069A-2 F1] Modo web local: servidor HTTP loopback con SSE. */
        Some("web") => {
            let puerto = extraer_opcion(&args, &["--puerto", "--port"])
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8799);
            let dir_ui = args.iter()
                .position(|a| a == "--dir-ui")
                .and_then(|i| args.get(i + 1).cloned())
                .or_else(|| {
                    /* Default: ../desktop/ui/dist relativo al binario */
                    std::env::current_exe().ok().and_then(|p| {
                        p.parent()
                            .map(|p| p.join("../desktop/ui/dist"))
                            .filter(|p| p.exists())
                            .map(|p| p.to_string_lossy().into_owned())
                    })
                });
            let fixture = args.iter().any(|a| a == "--fixture");
            con_runtime("web", |rt| rt.block_on(web::run(puerto, dir_ui, fixture)))
        }
        Some("schedule") => schedule_cmd::cmd_schedule(&args[1..]),
        Some("session") => cmd_session(&args[1..]),
        Some("memoria") => cmd_memoria(&args[1..]),
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
            eprintln!("uso: glory-harness <run|chat|daemon|schedule|session|memoria|tools|doctor|--version>");
            ExitCode::from(2)
        }
        None => {
            eprintln!(
                "glory-harness: falta subcomando (run|chat|daemon|schedule|session|memoria|tools|doctor|--version)"
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
    println!(
        "uso: glory-harness <run|chat|daemon|schedule|session|memoria|tools|doctor|--version>"
    );
    println!();
    println!(
        "  run       turno único (--prompt/--stdin/--dir/--provider/--modelo/--modo/--notificar)"
    );
    println!("  chat      sesión interactiva; --tui para la interfaz enriquecida; --notificar para toast de Windows");
    println!("  daemon    servicio de fondo por NDJSON (consumidor-daemon.mjs)");
    println!("  schedule  tareas programadas: <list|create|remove|logs|run>");
    println!("  session   conversaciones: <list|ver|resume|borrar> [id]");
    println!("  memoria   recuerdos: <listar|recordar|guardar|borrar|curar> [args]");
    println!("  tools     tools disponibles del núcleo");
    println!("  doctor    diagnóstico de configuración y proveedores");
    println!("  --version versión del CLI y del contrato core");
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
        dir: extraer_opcion(args, &["--dir", "--cwd", "--workspace"]).map(std::path::PathBuf::from),
        modo: extraer_opcion(args, &["--modo"]),
        razonamiento: None,
        max_ventana: None,
        /* [069A-3] `session resume` reabre el REPL: admite el mismo flag. */
        notificar: args.iter().any(|a| a == "--notificar"),
        navegador: None,
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

/// [069A-4] `glory-harness memoria <listar|recordar|guardar|borrar|curar>`:
/// mantiene a mano la memoria de aprendizaje (misma BD durable y usuario
/// estable que `chat`/`session`). Sin subacción o con argumentos
/// incompletos → exit 2 (mismo contrato que `session`).
fn cmd_memoria(args: &[String]) -> ExitCode {
    if args.is_empty() {
        eprintln!("uso: glory-harness memoria <listar|recordar|guardar|borrar|curar> [args]");
        return ExitCode::from(2);
    }
    let resultado = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(glory_harness::memoria::memoria(args)),
        Err(e) => {
            eprintln!("glory-harness memoria: no se pudo iniciar el runtime tokio: {e}");
            return ExitCode::from(1);
        }
    };
    use glory_harness::memoria::SalidaMemoria;
    match resultado {
        Ok(SalidaMemoria::Ok) => ExitCode::SUCCESS,
        Ok(SalidaMemoria::Uso) => {
            eprintln!("uso: glory-harness memoria <listar|recordar|guardar|borrar|curar> [args]");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("glory-harness memoria: {e}");
            ExitCode::from(1)
        }
    }
}

/// Lee el valor de la primera opción que coincida con uno de `nombres`,
/// devolviendo el siguiente argumento. `None` si no aparece.
/// `pub(crate)`: la usa el dominio `schedule_cmd`.
pub(crate) fn extraer_opcion(args: &[String], nombres: &[&str]) -> Option<String> {
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
        navegador: None,
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
