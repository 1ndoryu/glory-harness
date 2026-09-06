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
use glory_harness_core::historial::{historial_compartido, HistorialCompartido};
use glory_harness_core::ports::AgentPersistence;
use glory_harness_core::runtime::AgentRuntime;

use crate::run::{construir_harness, OpcionesRun};
use crate::ui::exportar::{guardar_export, render_markdown, ItemExport};
use crate::ui::plan::{ejecutar_undo, gestionar_plan, mostrar_plan_si_aplica};
use crate::ui::turno::{historial_desde_persistencia, procesar_turno};

/// Ejecuta el subcomando `chat`: abre la sesión interactiva y no devuelve
/// hasta que el usuario salga (`/salir`, Ctrl+C o EOF).
pub async fn chat(opciones: OpcionesRun) -> Result<(), String> {
    let harness = construir_harness(&opciones).await?;
    let persistencia = harness.persistencia;
    let user_id = harness.user_id;
    let workspace = harness.workspace;
    let config = harness.config;
    let runtime = harness.runtime;

    /* [Bloque 3, F3] Comandos slash personalizados: plantillas markdown en
     * `<workspace>/.glory/comandos/` (`tipo: comando` en frontmatter) que se
     * expanden con `$ARGUMENTOS` y `@archivo`. Los built-ins (`/salir`,
     * `/nuevo`, `/plan`, `/ayuda`) mandan: un comando personalizado que
     * colisione se ignora (fail-closed). */
    let comandos = workspace
        .as_deref()
        .map(|ws| glory_harness_core::skill::descubrir_comandos(&ws.join(".glory").join("comandos")))
        .unwrap_or_default();

    let raiz = workspace
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    println!(
        "glory-harness chat — modelo {}/{} · workspace {raiz}",
        config.provider, config.modelo
    );
    println!("escribe un mensaje, o /ayuda para los comandos");

    /* [318A-16 F2] Texto que reintentar tras resolver aprobaciones (tres
     * vías). El REPL reenvía el último mensaje para que el agente ejecute lo
     * aprobado SIN que el usuario escriba dos veces; el CLI no persiste
     * mensajes de usuario, así que no duplica historial. */
    let mut rx_lineas = lanzar_lector_lineas();
    let mut conversacion_id = Uuid::new_v4();
    let mut reintento: Option<String> = None;
    /* [318A-17 B3-F5] Transcripción de la sesión para `/export`: mensajes de
     * usuario + respuestas del asistente con sus tools. Se resetea con
     * `/nuevo` (nueva conversación). */
    let mut transcripcion: Vec<ItemExport> = Vec::new();
    /* [318A-17 B3-F6] Historial de checkpoints de la sesión para `/undo`:
     * aprobar un plan deja imagen previa recuperable; el undo revierte el
     * último checkpoint. Vive toda la sesión del REPL (los cambios de disco
     * son reales y un undo posterior sigue siendo válido aunque la
     * conversación se haya reiniciado con `/nuevo`). */
    let historial = historial_compartido();
    loop {
        let es_reenvio = reintento.is_some();
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
        let mut texto = linea.trim().to_string();
        if texto.is_empty() {
            continue;
        }
        match manejar_comando(
            &texto,
            &runtime,
            &workspace,
            &historial,
            &mut rx_lineas,
            &comandos,
        )
        .await?
        {
            Comando::Salir => return Ok(()),
            Comando::NuevaConversacion => {
                conversacion_id = Uuid::new_v4();
                transcripcion.clear();
                println!("[chat] conversación nueva (el agente ya no recuerda lo anterior)");
                continue;
            }
            /* [318A-17 B3-F5] `/export [archivo]`: vuelca la transcripción
             * de la sesión a Markdown (consola sin ruta, archivo con ruta).
             * No ejecuta turno. */
            Comando::Exportar(ruta) => {
                exportar_sesion(&transcripcion, conversacion_id, ruta.as_deref())?;
                continue;
            }
            /* [Bloque 3, F3] Un comando personalizado expandido sustituye el
             * texto del usuario: la plantilla (con `$ARGUMENTOS` y `@archivo`
             * embebidos) fluye como mensaje normal al turno. */
            Comando::Enviar(expandido) => {
                eprintln!("[comando] plantilla expandida ({} caracteres)", expandido.len());
                texto = expandido;
            }
            Comando::Continuar => {}
        }

        /* El turno (historial + ejecución + aprobaciones + modo plan) vive en
         * `ejecutar_turno_chat` para mantener el bucle REPL legible. El
         * mensaje del usuario se registra solo si es nuevo (los reenvíos tras
         * aprobar repiten el mismo texto y no deben duplicarse). */
        if !es_reenvio {
            transcripcion.push(ItemExport::usuario(texto.clone()));
        }
        match ejecutar_turno_chat(
            CtxTurnoChat {
                runtime: runtime.clone(),
                persistencia: persistencia.clone(),
                user_id,
                conversacion_id,
                workspace: &workspace,
                historial: historial.clone(),
            },
            &mut rx_lineas,
            &texto,
            &mut transcripcion,
        )
        .await?
        {
            Siguiente::Reenviar(texto) => reintento = Some(texto),
            Siguiente::Prompt => {}
            Siguiente::Salir => return Ok(()),
        }
    }
}

/// Lanza el hilo que lee stdin línea a línea (modo cooked, sin TUI); EOF o
/// error → `None` y el bucle termina con exit 0. El canal evita bloquear el
/// runtime del consumidor.
fn lanzar_lector_lineas() -> tokio::sync::mpsc::Receiver<Option<String>> {
    let (tx_lineas, rx_lineas) = tokio::sync::mpsc::channel::<Option<String>>(16);
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
    rx_lineas
}

/// Estado estable de una sesión de chat para `ejecutar_turno_chat`: los
/// valores que no cambian entre turnos del REPL (agrupados para mantener la
/// firma del turno legible). `workspace` se presta del bucle; la
/// `conversacion_id` se re-crea al hacer `/nuevo`.
struct CtxTurnoChat<'a> {
    runtime: Arc<AgentRuntime>,
    persistencia: Arc<dyn AgentPersistence>,
    user_id: Uuid,
    conversacion_id: Uuid,
    workspace: &'a Option<std::path::PathBuf>,
    /// [318A-17 B3-F6] Checkpoints de la sesión (para `/undo` tras aprobar).
    historial: HistorialCompartido,
}

/// Ejecuta un mensaje completo de chat: lee el historial acumulado, lanza el
/// turno, imprime el resultado y resuelve aprobaciones + modo plan. Devuelve
/// `Siguiente` para que el bucle REPL decida (reenviar, prompt o salir).
async fn ejecutar_turno_chat(
    ctx: CtxTurnoChat<'_>,
    rx_lineas: &mut tokio::sync::mpsc::Receiver<Option<String>>,
    texto: &str,
    transcripcion: &mut Vec<ItemExport>,
) -> Result<Siguiente, String> {
    /* Historial acumulado de la conversación → el agente recuerda el hilo. */
    let historial = match ctx.persistencia.listar_mensajes(ctx.conversacion_id).await {
        Ok(mensajes) => historial_desde_persistencia(mensajes),
        Err(e) => {
            eprintln!("[chat] no se pudo leer el historial: {e}");
            return Ok(Siguiente::Prompt);
        }
    };

    /* El runtime persiste ambos mensajes vía puerto (`guardar_mensaje`);
     * aquí solo se reporta el resultado. Un fallo no acaba el chat: se
     * muestra y se vuelve al prompt para reintentar. `texto.clone()`:
     * el resolver de aprobaciones (F2) reintenta el mismo mensaje tras
     * decidir, así que el texto se conserva tras el turno. */
    match procesar_turno(
        ctx.runtime.clone(),
        ctx.user_id,
        ctx.conversacion_id,
        historial,
        texto.to_string(),
        imprimir_evento_turno,
    )
    .await
    {
        Ok(respuesta) => {
            /* [318A-17 B3-F5] La respuesta del asistente entra a la
             * transcripción con las herramientas ejecutadas (eventos del
             * turno). Se omite solo si no hubo texto ni tools. */
            if !respuesta.texto.is_empty() || !respuesta.herramientas.is_empty() {
                transcripcion.push(ItemExport::asistente(
                    respuesta.texto.clone(),
                    respuesta.herramientas.clone(),
                ));
            }
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
    /* Salir corta sin preguntar por el plan (mismo orden que el bucle
     * anterior). */
    let siguiente = match resolver_aprobaciones(&ctx.runtime, rx_lineas, texto).await {
        Siguiente::Salir => Siguiente::Salir,
        otra => {
            /* [318A-16 F5] Modo plan: al cerrar el turno se muestra el diff
             * acumulado y se pregunta aprobar/descartar (regla de una sola
             * aplicación). Fuera de modo plan no hay propuesta: no pregunta. */
            mostrar_plan_si_aplica(&ctx.runtime, ctx.workspace, &ctx.historial, rx_lineas).await?;
            otra
        }
    };
    Ok(siguiente)
}

/// Resultado del manejador de comandos `/` del REPL: salir, nueva
/// conversación, reemplazar el texto por un comando expandido o seguir con el
/// texto como mensaje.
enum Comando {
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
async fn manejar_comando(
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
                println!("[chat] no hay propuesta de plan (modo actual: {})", runtime.turno_config.modo);
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
            if let Some(expandido) = glory_harness_core::skill::expandir_comando(
                texto,
                comandos,
                workspace.as_deref(),
            ) {
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
fn exportar_sesion(
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

/// Imprime en stderr el progreso de un turno (tools, subagentes, telemetría).
/// Es el observador de `procesar_turno`: no captura estado, solo reporta.
fn imprimir_evento_turno(evento: AgenteEvento) {
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
        /* [Bloque 3, F1] `ask_user`: la pregunta se muestra y el turno
         * termina; el usuario responde como su siguiente mensaje. */
        AgenteEvento::Pregunta { texto, opciones, .. } => {
            eprintln!("  ? {texto}");
            if !opciones.is_empty() {
                for (i, opcion) in opciones.iter().enumerate() {
                    eprintln!("    {}) {opcion}", i + 1);
                }
            }
        }
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
}


/// Línea siguiente del hilo de stdin del REPL. `None` = Ctrl+C o EOF (fin de
/// sesión). Comparte la semántica de cancelación del bucle principal.
/// `pub(super)`: también la usa el modo plan (`ui/plan.rs`), que pregunta
/// aprobar/descartar en mitad del turno con la misma cancelación.
pub(super) async fn leer_linea(
    rx: &mut tokio::sync::mpsc::Receiver<Option<String>>,
) -> Option<String> {
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
