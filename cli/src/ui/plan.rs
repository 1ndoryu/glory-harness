//! [318A-17 B3-F6] Interfaz del modo plan + checkpoint/undo del REPL lineal
//! (`chat`): mostrar la propuesta acumulada, aprobarla dejando un checkpoint
//! recuperable y revertir el último con `/undo`. Extraída de `chat.rs` para
//! que el bucle quede como orquestador y esta maquinaria tenga un único
//! hogar (patrón `exportar.rs` de B3-F5).
//!
//! Es un cliente más sobre la API del núcleo: no toca el runtime. La lectura
//! de línea comparte la semántica de cancelación del REPL (`leer_linea` de
//! `chat.rs`: Ctrl+C o EOF → `None`).

use std::io::Write;

use glory_harness_core::historial::HistorialCompartido;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::sandbox::SandboxArchivos;

use super::chat::leer_linea;

/// [318A-16 F5] Si el modo es `plan` y hay cambios acumulados, muestra la
/// propuesta al terminar el turno. Fuera de modo plan no hace nada.
pub async fn mostrar_plan_si_aplica(
    runtime: &AgentRuntime,
    workspace: &Option<std::path::PathBuf>,
    historial: &HistorialCompartido,
    rx_lineas: &mut tokio::sync::mpsc::Receiver<Option<String>>,
) -> Result<(), String> {
    if runtime.turno_config.modo == "plan"
        && runtime.plan_actual().is_some_and(|p| glory_harness_core::plan::tiene_cambios(&p))
    {
        gestionar_plan(runtime, workspace, historial, rx_lineas, false).await?;
    }
    Ok(())
}

/// [318A-16 F5] Muestra la propuesta del modo plan y, si el usuario aprueba,
/// la aplica una sola vez sobre el workspace (misma semántica que las tools
/// de archivo: ruta relativa al sandbox). Con `auto_preguntar` se entra en
/// modo pregunta; con `false` solo muestra el estado.
pub async fn gestionar_plan(
    runtime: &AgentRuntime,
    workspace: &Option<std::path::PathBuf>,
    historial: &HistorialCompartido,
    rx_lineas: &mut tokio::sync::mpsc::Receiver<Option<String>>,
    auto_preguntar: bool,
) -> Result<(), String> {
    let Some(plan) = runtime.plan_actual() else {
        println!("[plan] no hay propuesta (modo actual: {})", runtime.turno_config.modo);
        return Ok(());
    };
    println!("───────────────── propuesta en modo plan ─────────────────");
    println!("{}", glory_harness_core::plan::resumen_plan(&plan));
    println!("───────────────────────────────────────────────────────────");
    if !auto_preguntar {
        return Ok(());
    }
    if !glory_harness_core::plan::tiene_cambios(&plan) {
        return Ok(());
    }
    let sandbox = match workspace
        .as_deref()
        .map(SandboxArchivos::nuevo)
        .transpose()
    {
        Ok(Some(sandbox)) => sandbox,
        Ok(None) => {
            eprintln!("[plan] sin workspace: no se puede aplicar");
            return Ok(());
        }
        Err(e) => {
            eprintln!("[plan] sandbox inválido: {e}");
            return Ok(());
        }
    };
    print!("¿Aprobar y aplicar? (s=aprobar · n=dejar pendiente · d=descartar) ");
    let _ = std::io::stdout().flush();
    match leer_linea(rx_lineas).await {
        Some(linea) if matches!(linea.trim().to_lowercase().as_str(), "s" | "si" | "y" | "yes" | "aprobar") => {
            /* [318A-17 B3-F6] Aprobar deja checkpoint recuperable: la imagen
             * previa se captura ANTES de escribir (fail-closed) y `/undo`
             * restaura. */
            match glory_harness_core::plan::aplicar_plan_con_checkpoint(&plan, &sandbox, historial) {
                Ok(msg) => println!("[plan] {msg}"),
                Err(e) => eprintln!("[plan] no se pudo aplicar: {e}"),
            }
        }
        Some(linea) if matches!(linea.trim().to_lowercase().as_str(), "d" | "descartar") => {
            println!("[plan] {}", glory_harness_core::plan::descartar_plan(&plan));
        }
        Some(_) => println!("[plan] propuesta pendiente (usa /plan aprobar o /plan descartar)"),
        None => return Err("fin de sesión durante la pregunta del plan".into()),
    }
    Ok(())
}

/// [318A-17 B3-F6] `/undo`: revierte el último checkpoint de la sesión (la
/// imagen previa que `aplicar_plan_con_checkpoint` dejó al aprobar el plan).
/// Devuelve el mensaje para el usuario; `Err` con el motivo si no se pudo.
/// Solo de sesión y sin tocar git real; sin checkpoint avisa (no es error).
pub fn ejecutar_undo(
    historial: &HistorialCompartido,
    workspace: &Option<std::path::PathBuf>,
) -> Result<String, String> {
    let sandbox = workspace
        .as_deref()
        .map(SandboxArchivos::nuevo)
        .transpose()
        .map_err(|e| format!("sandbox inválido: {e}"))?;
    let Some(sandbox) = sandbox else {
        return Ok("[chat] sin workspace: no hay nada que deshacer".to_string());
    };
    match glory_harness_core::historial::revertir_ultimo(historial, &sandbox) {
        Ok(Some(msg)) => Ok(msg),
        Ok(None) => Ok(
            "[chat] no hay checkpoint que deshacer (esta sesión no ha aprobado un plan)".to_string(),
        ),
        Err(e) => Err(format!("no se pudo revertir: {e}")),
    }
}
