//! [059A-S3/B3] Gate de aprobación (tres vías) de la TUI entre turnos:
//! Rechazar / Permitir una vez / Permitir siempre. Extraído de `bucle.rs`
//! (el worker está al límite de tamaño del gate). Misma semántica que
//! el REPL (`chat.rs::resolver_aprobaciones`): "Permitir siempre" pide
//! confirmación antes de persistir la regla de la CLASE (categoría + `**`);
//! el texto libre se convierte en el siguiente mensaje del usuario.

use super::{AgentRuntime, EventoTui};

/// [059A-S3] Gate de aprobación (tres vías) entre turnos, extraído de
/// `spawn_worker`: si el turno dejó peticiones `ask` pendientes, ofrece
/// Rechazar / Permitir una vez / Permitir siempre por cada una y lee la
/// decisión del canal de entrada (la UI queda viva; el prompt lo indica).
/// "Permitir siempre" pide confirmación antes de persistir la regla de la
/// CLASE (categoría + `**`). Devuelve `Some(todo_resuelto)` salvo que el
/// canal de entrada se cierre (`None` = Fin ya enviado, terminar worker).
pub(crate) async fn resolver_gate_aprobaciones(
    runtime: &AgentRuntime,
    rx_entrada: &mut tokio::sync::mpsc::Receiver<String>,
    tx_eventos: &tokio::sync::mpsc::UnboundedSender<EventoTui>,
    reintento: &mut Option<String>,
) -> Option<bool> {
    /* [318A-16 F2] Gate de aprobación (tres vías) entre turnos: si el
     * turno dejó peticiones `ask` pendientes, el worker ofrece
     * Rechazar / Permitir una vez / Permitir siempre por cada una y
     * lee la decisión del canal de entrada (la UI queda viva; el
     * prompt lo indica). "Permitir siempre" pide confirmación antes
     * de persistir la regla de la CLASE (categoría + `**`). */
    let mut todo_resuelto = true;
    'gate: loop {
        let pendientes = runtime.peticiones_aprobacion_pendientes();
        if pendientes.is_empty() {
            break 'gate;
        }
        for peticion in &pendientes {
            let _ = tx_eventos.send(EventoTui::Estado(format!(
                "⚠ {} pide aprobación (clase: {}) — [n] Rechazar · [p] Permitir una vez · [s] Permitir siempre",
                peticion.tool, peticion.clasificacion
            )));
            let Some(linea) = rx_entrada.recv().await else {
                let _ = tx_eventos.send(EventoTui::Fin);
                return None;
            };
            let decision = linea.trim().to_lowercase();
            let aplicada = match decision.as_str() {
                "n" | "no" | "rechazar" | "denegar" => runtime
                    .responder_aprobacion(
                        &peticion.id,
                        glory_harness_core::aprobacion::RespuestaAprobacion::Rechazar,
                    )
                    .map(|_| {
                        let _ = tx_eventos.send(EventoTui::Estado(format!(
                            "✗ clase '{}' denegada en esta conversación",
                            peticion.clasificacion
                        )));
                    })
                    .is_ok(),
                "p" | "permitir" | "si" | "aprobar" | "ok" => runtime
                    .responder_aprobacion(
                        &peticion.id,
                        glory_harness_core::aprobacion::RespuestaAprobacion::Aprobar,
                    )
                    .map(|_| {
                        let _ = tx_eventos
                            .send(EventoTui::Estado("✓ permitida (solo esta vez)".into()));
                    })
                    .is_ok(),
                "s" | "siempre" | "always" | "allow" => {
                    /* Confirmación previa (opencode exige Confirm/Cancel
                     * antes de persistir "always"). */
                    let _ = tx_eventos.send(EventoTui::Estado(format!(
                        "¿Permitir SIEMPRE la clase '{}'? [s/n]",
                        peticion.clasificacion
                    )));
                    let Some(conf) = rx_entrada.recv().await else {
                        let _ = tx_eventos.send(EventoTui::Fin);
                        return None;
                    };
                    if matches!(
                        conf.trim().to_lowercase().as_str(),
                        "s" | "si" | "siempre" | "y" | "yes" | "confirmar"
                    ) {
                        runtime
                            .responder_aprobacion(
                                &peticion.id,
                                glory_harness_core::aprobacion::RespuestaAprobacion::Siempre,
                            )
                            .map(|_| {
                                let _ = tx_eventos.send(EventoTui::Estado(format!(
                                    "✓ permitida siempre: la clase '{}' ya no preguntará",
                                    peticion.clasificacion
                                )));
                            })
                            .is_ok()
                    } else {
                        let _ = tx_eventos.send(EventoTui::Estado(
                            "(cancelado — la petición sigue pendiente)".into(),
                        ));
                        false
                    }
                }
                _ => {
                    /* Texto libre: respuesta del usuario al agente. Se
                     * convierte en el siguiente mensaje sin resolver
                     * la petición estructurada. */
                    *reintento = Some(linea);
                    todo_resuelto = false;
                    break 'gate;
                }
            };
            if !aplicada {
                todo_resuelto = false;
            }
        }
    }

    Some(todo_resuelto)
}
