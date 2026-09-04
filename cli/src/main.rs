//! Binario `glory-harness` (Fases 3-5): subcomandos `run`, `chat`, `daemon`,
//! `tools`, `doctor` y `--version`.
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

mod chat;
mod daemon;
mod ejecutor;
mod persistencia;
mod reglas;
mod run;
mod tui;

use std::process::ExitCode;

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
            eprintln!("uso: glory-harness <run|chat|daemon|tools|doctor|--version>");
            ExitCode::from(2)
        }
        None => {
            eprintln!("glory-harness: falta subcomando (run|chat|daemon|tools|doctor|--version)");
            ExitCode::from(2)
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

/// Carga `~/.glory-harness.env` si existe (formato `CLAVE=valor`, `#` = comentario).
/// Solo define variables aún ausentes, así el entorno real del proceso (o las
/// de un proyecto) siempre tienen prioridad. Nunca imprime valores.
fn cargar_env_usuario() {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
    else {
        return;
    };
    let ruta = home.join(".glory-harness.env");
    let contenido = match std::fs::read_to_string(&ruta) {
        Ok(c) => c,
        Err(_) => return, // no existe o no legible: sin claves extra, no es error
    };
    for linea in contenido.lines() {
        let linea = linea.trim();
        if linea.is_empty() || linea.starts_with('#') {
            continue;
        }
        let Some((clave, valor)) = linea.split_once('=') else {
            continue;
        };
        let clave = clave.trim();
        let valor = valor.trim();
        if clave.is_empty() || valor.is_empty() {
            continue;
        }
        // Solo si no está ya definida (edition 2021: set_var es seguro).
        if std::env::var_os(clave).is_none() {
            std::env::set_var(clave, valor);
        }
    }
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
        ejecutor_comando: Some(Arc::new(crate::ejecutor::EjecutorCliente::nuevo())),
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