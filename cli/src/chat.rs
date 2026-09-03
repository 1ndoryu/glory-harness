//! Subcomando `glory-harness chat` (Fase 5): REPL interactivo sobre el mismo
//! contrato `AgenteEvento` del núcleo.
//!
//! A diferencia de `run` (one-shot), el chat mantiene la MISMA conversación
//! entre turnos: reutiliza `PersistenciaMemoria` — que indexa mensajes por
//! `conversacion_id` —, pasa el historial acumulado a `ejecutar_turno` en cada
//! turno y pinta los eventos del contrato de forma legible (tools, errores).
//! Es un cliente más sobre la API existente: no toca el núcleo (R1 del plan).
//!
//! Interfaz v1: REPL lineal (`gh> `) con lectura de línea estándar (modo
//! cooked; sin dependencias de TUI). Comandos: `/ayuda`, `/nuevo`, `/salir`
//! (o `/exit`); Ctrl+C o EOF (en Windows Ctrl+Z+Enter) terminan con exit 0.

use std::io::{BufRead, Write};
use std::sync::Arc;

use uuid::Uuid;

use glory_harness_core::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::AiMessage;
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::AgentPersistence;

use crate::run::{construir_harness, OpcionesRun};

/// Convierte los mensajes persistidos de una conversación en el historial que
/// el runtime espera (`AiMessage`). Es la fuente entre turnos del chat: el
/// agente recuerda el hilo porque cada turno recibe todo lo anterior.
/// Compartido con la TUI (`tui.rs`, Fase 5 opción B): misma fuente, otra UI.
pub(crate) fn historial_desde_persistencia(mensajes: Vec<MensajePersistido>) -> Vec<AiMessage> {
    mensajes
        .into_iter()
        .map(|m| AiMessage::texto(&m.rol, m.contenido))
        .collect()
}

/// Ejecuta el subcomando `chat`: abre la sesión interactiva y no devuelve
/// hasta que el usuario salga (`/salir`, Ctrl+C o EOF).
pub async fn chat(opciones: OpcionesRun) -> Result<(), String> {
    let harness = construir_harness(&opciones);
    let persistencia = harness.persistencia;
    let user_id = harness.user_id;
    let workspace = harness.workspace;
    let config = harness.config;
    let runtime = harness.runtime;

    let raiz = workspace
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    println!(
        "glory-harness chat — modelo {}/{} · workspace {raiz}",
        config.provider, config.modelo
    );
    println!("escribe un mensaje, o /ayuda para los comandos");

    /* Un hilo lee stdin línea a línea (modo cooked, sin TUI); EOF o error → None
     * y el bucle termina con exit 0. El canal evita bloquear el runtime. */
    let (tx_lineas, mut rx_lineas) = tokio::sync::mpsc::channel::<Option<String>>(16);
    std::thread::spawn(move || {
        let mut linea = String::new();
        loop {
            linea.clear();
            let leidas = std::io::stdin().lock().read_line(&mut linea);
            match leidas {
                Ok(0) | Err(_) => {
                    let _ = tx_lineas.blocking_send(None);
                    break;
                }
                Ok(_) => {
                    let _ = tx_lineas.blocking_send(Some(std::mem::take(&mut linea)));
                }
            }
        }
    });

    let mut conversacion_id = Uuid::new_v4();
    /* [318A-16 F2] Texto que reintentar tras resolver aprobaciones (tres
     * vías). El REPL reenvía el último mensaje para que el agente ejecute lo
     * aprobado SIN que el usuario escriba dos veces; el CLI no persiste
     * mensajes de usuario, así que no duplica historial. */
    let mut reintento: Option<String> = None;
    loop {
        let linea = match reintento.take() {
            Some(linea) => linea,
            None => {
                print!("gh> ");
                let _ = std::io::stdout().flush();
                match leer_linea(&mut rx_lineas).await {
                    Some(linea) => linea,
                    None => {
                        println!();
                        return Ok(()); // Ctrl+C o EOF
                    }
                }
            }
        };
        let texto = linea.trim().to_string();
        if texto.is_empty() {
            continue;
        }
        if texto.starts_with('/') {
            match texto.as_str() {
                "/salir" | "/exit" | "/quit" => return Ok(()),
                "/nuevo" | "/reset" => {
                    conversacion_id = Uuid::new_v4();
                    println!("[chat] conversación nueva (el agente ya no recuerda lo anterior)");
                    continue;
                }
                "/ayuda" | "/help" | "/?" => {
                    println!("comandos:");
                    println!("  /salir   termina la sesión (también Ctrl+C o EOF)");
                    println!("  /nuevo   reinicia la conversación (historial limpio)");
                    println!("  /ayuda   muestra esta ayuda");
                    println!(
                        "estado: workspace {raiz} · modelo {}/{}",
                        config.provider, config.modelo
                    );
                    continue;
                }
                _ => {
                    eprintln!("[chat] comando desconocido: {texto} (usa /ayuda)");
                    continue;
                }
            }
        }

        /* Historial acumulado de la conversación → el agente recuerda el hilo. */
        let historial = match persistencia.listar_mensajes(conversacion_id).await {
            Ok(mensajes) => historial_desde_persistencia(mensajes),
            Err(e) => {
                eprintln!("[chat] no se pudo leer el historial: {e}");
                continue;
            }
        };

        /* El runtime persiste ambos mensajes vía puerto (`guardar_mensaje`);
         * aquí solo se reporta el resultado. Un fallo no acaba el chat: se
         * muestra y se vuelve al prompt para reintentar. `texto.clone()`:
         * el resolver de aprobaciones (F2) reintenta el mismo mensaje tras
         * decidir, así que el texto se conserva tras el turno. */
        match procesar_turno(Arc::clone(&runtime), user_id, conversacion_id, historial, texto.clone(), |evento| {
            match evento {
                AgenteEvento::Token { .. } => {}
                AgenteEvento::ToolStart { tool, .. } => eprintln!("  → {tool}"),
                AgenteEvento::PeticionAprobacion {
                    tool,
                    clasificacion,
                    ..
                } => eprintln!("  ⚠ {tool} pide aprobación (clase: {clasificacion})"),
                AgenteEvento::RequiereAprobacion { tool, .. } => {
                    eprintln!("  ⚠ {tool} requiere aprobación (modo predeterminado)")
                }
                AgenteEvento::SubagenteInicio {
                    perfil,
                    instruccion,
                } => {
                    eprintln!(
                        "  └ subagente [{perfil}]: {}",
                        instruccion.lines().next().unwrap_or("")
                    )
                }
                AgenteEvento::SubagenteFin { ok, .. } => eprintln!("  └ subagente: {}", if ok { "fin" } else { "sin resumen" }),
                AgenteEvento::ToolResult {
                    tool,
                    ok: false,
                    resumen,
                    ..
                } => eprintln!("  ✗ {tool}: {resumen}"),
                AgenteEvento::Error { mensaje, .. } => eprintln!("  ✗ error: {mensaje}"),
                AgenteEvento::Telemetria {
                    motivo_cierre,
                    compactaciones,
                    denegaciones,
                    herramientas,
                    ..
                } => eprintln!(
                    "  ─ telemetría: {motivo_cierre} · {compactaciones} compactaciones · {denegaciones} denegaciones · {} tools",
                    herramientas.len()
                ),
                AgenteEvento::Done { .. } => {}
                _ => {}
            }
        })
        .await
        {
            Ok(respuesta) => {
                if !respuesta.tools.is_empty() {
                    eprintln!("  tools: {}", respuesta.tools.join(", "));
                }
                println!();
                println!("{}", respuesta.texto);
                println!();
            }
            Err(err) => eprintln!("[chat] el turno falló: {err} (puedes reintentar)"),
        }

        /* [318A-16 F2] Tras cada turno, resolver las peticiones `ask` que
         * quedaron pendientes (el SSE es unidireccional: la decisión se aplica
         * entre turnos y el re-envío del mensaje la consume). Si el usuario
         * escribe texto libre en vez de una opción, ese texto es su mensaje al
         * agente (p. ej. "adelante" responde a la pregunta del modelo). */
        match resolver_aprobaciones(&runtime, &mut rx_lineas, &texto).await {
            Siguiente::Reenviar(texto) => reintento = Some(texto),
            Siguiente::Prompt => {}
            Siguiente::Salir => return Ok(()),
        }
    }
}

/// Línea siguiente del hilo de stdin del REPL. `None` = Ctrl+C o EOF (fin de
/// sesión). Comparte la semántica de cancelación del bucle principal.
async fn leer_linea(rx: &mut tokio::sync::mpsc::Receiver<Option<String>>) -> Option<String> {
    tokio::select! {
        _ = tokio::signal::ctrl_c() => None,
        opt = rx.recv() => match opt {
            Some(Some(linea)) => Some(linea),
            _ => None,
        },
    }
}

/// Qué hacer tras resolver (o no) las peticiones de aprobación del turno.
enum Siguiente {
    /// Volver al prompt `gh> ` normal (sin peticiones pendientes).
    Prompt,
    /// Reenviar este texto al agente (decisión tomada o texto libre del usuario).
    Reenviar(String),
    /// Fin de sesión (Ctrl+C/EOF durante una pregunta).
    Salir,
}

/// Pinta una petición de aprobación con su detalle para que el humano decida.
fn pintar_peticion(peticion: &PeticionAprobacion) {
    println!();
    println!("⚠ {} pide aprobación (clase: {})", peticion.tool, peticion.clasificacion);
    if let Some(objeto) = peticion.argumentos.as_object() {
        for (clave, valor) in objeto {
            println!("    {clave}: {valor}");
        }
    }
}

/// Resuelve las peticiones de aprobación pendientes con tres vías (paridad
/// opencode `permission.shared.ts`): Rechazar / Permitir una vez / Permitir
/// siempre. "Permitir siempre" pide confirmación antes de persistir la regla
/// de la CLASE (categoría + `**`), no del comando exacto.
async fn resolver_aprobaciones(
    runtime: &Arc<AgentRuntime>,
    rx_lineas: &mut tokio::sync::mpsc::Receiver<Option<String>>,
    ultimo_texto: &str,
) -> Siguiente {
    /* Paso único: si tras responder quedan nuevas peticiones (turno re-enviado
     * con más `ask`), el llamador vuelve a invocar esta función — el bucle
     * externo no iteraría nunca porque toda vía acaba en `return`. */
    let pendientes = runtime.peticiones_aprobacion_pendientes();
    if pendientes.is_empty() {
        return Siguiente::Prompt;
    }
        for peticion in &pendientes {
            pintar_peticion(peticion);
            let decision = loop {
                print!("  [n] Rechazar · [p] Permitir una vez · [s] Permitir siempre → ");
                let _ = std::io::stdout().flush();
                match leer_linea(rx_lineas).await {
                    Some(linea) => {
                        let d = linea.trim().to_lowercase();
                        if !d.is_empty() {
                            break d;
                        }
                    }
                    None => return Siguiente::Salir,
                }
            };
            match decision.as_str() {
                "n" | "no" | "rechazar" | "denegar" => {
                    if let Err(e) = runtime.responder_aprobacion(
                        &peticion.id,
                        RespuestaAprobacion::Rechazar,
                    ) {
                        eprintln!("[chat] {e}");
                    } else {
                        println!("  ✗ rechazada (la clase queda denegada en esta conversación)");
                    }
                }
                "p" | "permitir" | "si" | "aprobar" | "ok" => {
                    if let Err(e) = runtime
                        .responder_aprobacion(&peticion.id, RespuestaAprobacion::Aprobar)
                    {
                        eprintln!("[chat] {e}");
                    } else {
                        println!("  ✓ permitida (solo esta vez)");
                    }
                }
                "s" | "siempre" | "always" | "allow" => {
                    /* opencode exige confirmación antes de persistir "always". */
                    print!(
                        "  ¿Permitir SIEMPRE la clase '{}'? [s/n] → ",
                        peticion.clasificacion
                    );
                    let _ = std::io::stdout().flush();
                    match leer_linea(rx_lineas).await {
                        Some(linea)
                            if matches!(
                                linea.trim().to_lowercase().as_str(),
                                "s" | "si" | "siempre" | "y" | "yes" | "confirmar"
                            ) =>
                        {
                            if let Err(e) = runtime
                                .responder_aprobacion(&peticion.id, RespuestaAprobacion::Siempre)
                            {
                                eprintln!("[chat] {e}");
                            } else {
                                println!("  ✓ permitida siempre: la clase '{}' ya no preguntará", peticion.clasificacion);
                            }
                        }
                        Some(_) => {
                            println!("  (cancelado — la petición sigue pendiente)");
                            return Siguiente::Prompt;
                        }
                        None => return Siguiente::Salir,
                    }
                }
                _ => {
                    /* Texto libre: el humano respondió al agente (p. ej.
                     * "adelante"). No resuelve la petición estructurada; se
                     * reenvía como mensaje. */
                    return Siguiente::Reenviar(decision);
                }
            }
        }
        /* Todas las peticiones se respondieron con palabras clave: reintentar
         * el último mensaje para que el agente ejecute lo aprobado. */
        Siguiente::Reenviar(ultimo_texto.to_string())
}

/// Resultado de un turno de chat: texto del asistente + tools ejecutadas.
/// Compartido entre el REPL lineal y la TUI (`chat --tui`). Los errores se
/// muestran en vivo por el callback de eventos, no se acumulan aquí.
pub struct TurnoResultado {
    pub texto: String,
    pub tools: Vec<String>,
}

/// Ejecuta un turno sobre la conversación y consume el contrato `AgenteEvento`
/// con un callback por evento (cada UI decide cómo pintarlo: el REPL imprime
/// en vivo; la TUI lo acumula en sus paneles). El runtime persiste ambos
/// mensajes vía puerto; aquí solo se recolecta el resultado. Un fallo se
/// devuelve como `Err` y no acaba la sesión: el llamador puede reintentar.
pub(crate) async fn procesar_turno(
    runtime: Arc<AgentRuntime>,
    user_id: Uuid,
    conversacion_id: Uuid,
    historial: Vec<AiMessage>,
    texto: String,
    mut on_evento: impl FnMut(AgenteEvento),
) -> Result<TurnoResultado, String> {
    let turno_id = Uuid::new_v4();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgenteEvento>(64);
    let handle = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move {
            runtime
                .ejecutar_turno(user_id, turno_id, conversacion_id, historial, texto, &tx)
                .await
        }
    });

    let mut texto_respuesta = String::new();
    let mut tools = Vec::new();
    while let Some(evento) = rx.recv().await {
        match &evento {
            AgenteEvento::Token { texto: t } => texto_respuesta.push_str(t),
            AgenteEvento::ToolStart { tool, .. } => tools.push(tool.clone()),
            AgenteEvento::Done { .. } => break,
            _ => {}
        }
        on_evento(evento);
    }

    match handle.await {
        Ok(Ok(())) => Ok(TurnoResultado {
            texto: texto_respuesta,
            tools,
        }),
        Ok(Err(err)) => Err(err.to_string()),
        Err(err) => Err(format!("el turno abortó con pánico: {err}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn historial_preserva_orden_y_roles() {
        let ahora = Utc::now();
        let conv = Uuid::new_v4();
        let mensajes = vec![
            MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: "user".into(),
                contenido: "primero".into(),
                creado_en: ahora,
            },
            MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: "assistant".into(),
                contenido: "respuesta".into(),
                creado_en: ahora,
            },
            MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: "user".into(),
                contenido: "segundo".into(),
                creado_en: ahora,
            },
        ];
        let historial = historial_desde_persistencia(mensajes);
        assert_eq!(historial.len(), 3);
        assert_eq!(historial[0].role, "user");
        assert_eq!(historial[0].content, serde_json::Value::String("primero".into()));
        assert_eq!(historial[1].role, "assistant");
        assert_eq!(historial[1].content, serde_json::Value::String("respuesta".into()));
        assert_eq!(historial[2].content, serde_json::Value::String("segundo".into()));
    }

    #[test]
    fn historial_vacio_es_vacio() {
        assert!(historial_desde_persistencia(vec![]).is_empty());
    }
}