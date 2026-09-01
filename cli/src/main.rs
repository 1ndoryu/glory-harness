//! Binario `glory-harness` (Fase 3: `run` one-shot y `daemon` SSE en
//! loopback). Fase 0: verifica el arranque y reporta la versión del contrato.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("--version" | "-V") => {
            println!(
                "glory-harness {} (contrato core {})",
                env!("CARGO_PKG_VERSION"),
                glory_harness_core::CONTRATO_VERSION,
            );
            ExitCode::SUCCESS
        }
        Some("run" | "daemon" | "tools" | "doctor") => {
            eprintln!(
                "glory-harness: el subcomando '{}' estará disponible en la Fase 3 del plan 318A-13.",
                args[1],
            );
            ExitCode::from(2)
        }
        Some(other) => {
            eprintln!("glory-harness: subcomando desconocido '{other}'");
            eprintln!("uso: glory-harness <run|daemon|tools|doctor|--version>");
            ExitCode::from(2)
        }
        None => {
            eprintln!("glory-harness: falta subcomando (run|daemon|tools|doctor|--version)");
            ExitCode::from(2)
        }
    }
}