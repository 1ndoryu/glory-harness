//! Comandos `/` y aprobaciones del REPL `chat` (Fase 5 + 318A-16 F2/F5).
//!
//! [069A-5 F5] Extraído de `ui/chat.rs` (limite-lineas 576): el bucle REPL y
//! el turno quedan en `chat`; aquí viven el despacho de comandos slash, el
//! export de sesión y la resolución interactiva de aprobaciones.

use std::io::Write;
use std::sync::Arc;

use uuid::Uuid;

use glory_harness_core::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use glory_harness_core::historial::HistorialCompartido;
use glory_harness_core::runtime::AgentRuntime;

use super::chat::leer_linea;
use crate::ui::exportar::{guardar_export, render_markdown, ItemExport};
use crate::ui::plan::{ejecutar_undo, gestionar_plan};

/// Resultado del manejador de comandos `/` del REPL: salir, nueva
/// conversación, reemplazar el texto por un comando expandido o seguir con el
/// texto como mensaje.
pub(super) enum Comando {
    Salir,
    NuevaConversacion,
    /// [318A-17 B3-F5] `/export [archivo]` — vuelca la sesión a Markdown.
    Exportar(Option<String>),
    /// [Bloque 3, F3] Comando personalizado expandido (plantilla markdown).
    Enviar(String),
    Continuar,
}

/// [318A-16 F5] Comandos `/` del REPL (incluida la gestión del modo plan).
/// Devuelve `Salir`/`NuevaConversacion` para que el bucle principal actúe;
/// `Continuar` deja que el texto fluya como mensaje del usuario.
pub(super) async fn manejar_comando(
    texto: &str,
    runtime: &AgentRuntime,
    workspace: &Option<std::path::PathBuf>,
    historial: &HistorialCompartido,
    rx_lineas: &mut tokio::sync::mpsc::Receiver<Option<String>>,
    comandos: &[glory_harness_core::skill::ComandoSlash],
) -> Result<Comando, String> {
    if !texto.starts_with('/') {
        return Ok(Comando::Continuar);
    }
    match texto {
        "/salir" | "/exit" | "/quit" => Ok(Comando::Salir),
        "/nuevo" | "/reset" => Ok(Comando::NuevaConversacion),
        "/plan" | "/plan estado" => {
            gestionar_plan(runtime, workspace, historial, rx_lineas, false).await?;
            Ok(Comando::Continuar)
        }
        "/plan aprobar" => {
            gestionar_plan(runtime, workspace, historial, rx_lineas, true).await?;
            Ok(Comando::Continuar)
        }
        "/plan descartar" => {
            if let Some(plan) = runtime.plan_actual() {
                println!("{}", glory_harness_core::plan::descartar_plan(&plan));
            } else {
                println!(
                    "[chat] no hay propuesta de plan (modo actual: {})",
                    runtime.turno_config.modo
                );
            }
            Ok(Comando::Continuar)
        }
        "/undo" => {
            /* [318A-17 B3-F6] Revierte el último checkpoint aprobado (la
             * maquinaria vive en `ui/plan.rs` junto al modo plan). */
            match ejecutar_undo(historial, workspace) {
                Ok(msg) => println!("{msg}"),
                Err(e) => eprintln!("[chat] {e}"),
            }
            Ok(Comando::Continuar)
        }
        s if s == "/export" || s.starts_with("/export ") => {
            /* [318A-17 B3-F5] /export [archivo]: sin ruta imprime en consola,
             * con ruta escribe el archivo. Built-in que manda sobre un
             * comando personalizado que colisione (fail-closed, como F3). */
            let ruta = s
                .strip_prefix("/export ")
                .map(str::trim)
                .filter(|r| !r.is_empty());
            Ok(Comando::Exportar(ruta.map(ToString::to_string)))
        }
        "/ayuda" | "/help" | "/?" => {
            println!("comandos:");
            println!("  /salir   termina la sesión (también Ctrl+C o EOF)");
            println!("  /nuevo   reinicia la conversación (historial limpio)");
            println!("  /export  vuelca la conversación a Markdown (/export archivo.md)");
            if runtime.turno_config.modo == "plan" {
                println!("  /plan            muestra la propuesta acumulada (diff)");
                println!("  /plan aprobar    aplica la propuesta una sola vez");
                println!("  /plan descartar  descarta la propuesta sin aplicar");
                println!("  /undo            revierte el último plan aprobado (checkpoint)");
            }
            println!("  /ayuda   muestra esta ayuda");
            if !comandos.is_empty() {
                println!("comandos personalizados (.glory/comandos/):");
                for comando in comandos {
                    println!("  /{} — {}", comando.nombre, comando.descripcion);
                }
            }
            println!(
                "estado: modelo {}/{}",
                runtime.turno_config.provider, runtime.turno_config.modelo
            );
            Ok(Comando::Continuar)
        }
        _ => {
            /* [Bloque 3, F3] Comandos personalizados (`.glory/comandos/`): la
             * plantilla se expande con `$ARGUMENTOS` y `@archivo` y el
             * resultado viaja como mensaje del usuario. Sin coincidencia →
             * comando desconocido (igual que antes). */
            if let Some(expandido) =
                glory_harness_core::skill::expandir_comando(texto, comandos, workspace.as_deref())
            {
                Ok(Comando::Enviar(expandido))
            } else {
                eprintln!("[chat] comando desconocido: {texto} (usa /ayuda)");
                Ok(Comando::Continuar)
            }
        }
    }
}

/// [318A-17 B3-F5] `/export [archivo]`: vuelca la transcripción a Markdown.
/// Sin ruta imprime en consola (REPL lineal); con ruta escribe el archivo y
/// reporta la ruta.
pub(super) fn exportar_sesion(
    transcripcion: &[ItemExport],
    conversacion_id: Uuid,
    ruta: Option<&str>,
) -> Result<(), String> {
    let markdown = render_markdown(conversacion_id, transcripcion, chrono::Utc::now());
    match ruta {
        Some(ruta) => {
            guardar_export(ruta, &markdown)?;
            println!("[chat] conversación exportada a {ruta}");
        }
        None => println!("{markdown}"),
    }
    Ok(())
}

/// Qué hacer tras resolver (o no) las peticiones de aprobación del turno.
pub(super) enum Siguiente {
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
    println!(
        "⚠ {} pide aprobación (clase: {})",
        peticion.tool, peticion.clasificacion
    );
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
pub(super) async fn resolver_aprobaciones(
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
                if let Err(e) =
                    runtime.responder_aprobacion(&peticion.id, RespuestaAprobacion::Rechazar)
                {
                    eprintln!("[chat] {e}");
                } else {
                    println!("  ✗ rechazada (la clase queda denegada en esta conversación)");
                }
            }
            "p" | "permitir" | "si" | "aprobar" | "ok" => {
                if let Err(e) =
                    runtime.responder_aprobacion(&peticion.id, RespuestaAprobacion::Aprobar)
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
                        if let Err(e) =
                            runtime.responder_aprobacion(&peticion.id, RespuestaAprobacion::Siempre)
                        {
                            eprintln!("[chat] {e}");
                        } else {
                            println!(
                                "  ✓ permitida siempre: la clase '{}' ya no preguntará",
                                peticion.clasificacion
                            );
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
