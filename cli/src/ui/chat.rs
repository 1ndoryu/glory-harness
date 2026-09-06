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

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::historial::{historial_compartido, HistorialCompartido};
use glory_harness_core::ports::AgentPersistence;
use glory_harness_core::runtime::AgentRuntime;

use crate::run::{construir_harness_durable, OpcionesRun};
use crate::ui::exportar::ItemExport;
use crate::ui::plan::mostrar_plan_si_aplica;
use crate::ui::turno::{historial_desde_persistencia, procesar_turno};

use super::chat_comandos::{
    exportar_sesion, manejar_comando, resolver_aprobaciones, Comando, Siguiente,
};

/// Ejecuta el subcomando `chat`: abre la sesión interactiva y no devuelve
/// hasta que el usuario salga (`/salir`, Ctrl+C o EOF).
/// [069A-2] Harness durable (sqlite + `user_id` estable): la conversación se
/// crea como fila (`conversacion_crear`) y sobrevive a los procesos para
/// `session resume`; el título se toma del primer mensaje.
pub async fn chat(opciones: OpcionesRun) -> Result<(), String> {
    let harness = construir_harness_durable(&opciones).await?;
    // [069A-3] Avisos de escritorio solo si el operador los pidió.
    crate::notificar::aplicar_notificacion(&harness.runtime, opciones.notificar);
    let sqlite = harness
        .sqlite
        .clone()
        .ok_or_else(|| "chat durable sin tienda sqlite (inconsistencia interna)".to_string())?;
    let user_id = harness.user_id;
    let titulo = format!("sesión {}", chrono::Local::now().format("%d-%m %H:%M"));
    let conversacion_id = sqlite
        .conversacion_crear(user_id, &titulo)
        .map_err(|e| e.to_string())?;
    bucle_chat(harness, sqlite, conversacion_id, Vec::new(), false).await
}

/// [069A-2] Abre el REPL sobre una conversación existente (`session resume`):
/// recompone la transcripción de export desde sus mensajes y continúa el hilo
/// (el historial del turno se lee de la tienda en cada mensaje, como siempre).
pub async fn chat_resume(opciones: OpcionesRun, conversacion_id: Uuid) -> Result<(), String> {
    let harness = construir_harness_durable(&opciones).await?;
    // [069A-3] Como en `chat` (mismo REPL sobre otra conversación).
    crate::notificar::aplicar_notificacion(&harness.runtime, opciones.notificar);
    let sqlite = harness
        .sqlite
        .clone()
        .ok_or_else(|| "chat durable sin tienda sqlite (inconsistencia interna)".to_string())?;
    let user_id = harness.user_id;
    // Guarda de ownership: la conversación debe existir y ser del usuario
    // estable (una ajena se rechaza sin revelar nada más).
    let propia = sqlite
        .conversaciones_listar(user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .any(|c| c.id == conversacion_id);
    if !propia {
        return Err(format!("la conversación {conversacion_id} no existe"));
    }
    let mensajes = harness
        .persistencia
        .listar_mensajes(conversacion_id)
        .await
        .map_err(|e| e.to_string())?;
    let transcripcion = transcripcion_desde_mensajes(&mensajes);
    println!(
        "[chat] conversación retomada ({} mensajes en el hilo)",
        mensajes.len()
    );
    bucle_chat(harness, sqlite, conversacion_id, transcripcion, true).await
}

/// [069A-2] Reconstruye la transcripción de export desde mensajes persistidos
/// (pura y testeable): usuario → entrada de usuario; asistente → respuesta sin
/// detalle de tools (los mensajes guardan texto, no eventos).
pub fn transcripcion_desde_mensajes(
    mensajes: &[glory_harness_core::ports::MensajePersistido],
) -> Vec<ItemExport> {
    mensajes
        .iter()
        .map(|m| {
            if m.rol == "user" {
                ItemExport::usuario(m.contenido.clone())
            } else {
                ItemExport::asistente(m.contenido.clone(), Vec::new())
            }
        })
        .collect()
}

/// [069A-2] Bucle REPL extraído de `chat` para compartirlo con `session
/// resume`: arranca sobre `conversacion_id` con su transcripción ya cargada.
/// `titulada` indica si la fila ya tiene título definitivo (el resume no
/// retitula; una sesión nueva toma el primer mensaje como título).
async fn bucle_chat(
    harness: crate::run::HarnessCli,
    sqlite: std::sync::Arc<crate::persistencia_sqlite::PersistenciaSqlite>,
    mut conversacion_id: Uuid,
    mut transcripcion: Vec<ItemExport>,
    mut titulada: bool,
) -> Result<(), String> {
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
        .map(|ws| {
            glory_harness_core::skill::descubrir_comandos(&ws.join(".glory").join("comandos"))
        })
        .unwrap_or_default();

    anunciar_sesion(&workspace, &config);

    /* [318A-16 F2] Texto que reintentar tras resolver aprobaciones (tres
     * vías). El REPL reenvía el último mensaje para que el agente ejecute lo
     * aprobado SIN que el usuario escriba dos veces. */
    let mut rx_lineas = lanzar_lector_lineas();
    let mut reintento: Option<String> = None;
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
                nueva_conversacion(
                    &sqlite,
                    user_id,
                    &mut conversacion_id,
                    &mut transcripcion,
                    &mut titulada,
                );
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
                eprintln!(
                    "[comando] plantilla expandida ({} caracteres)",
                    expandido.len()
                );
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
            titular_si_nueva(&sqlite, conversacion_id, user_id, &texto, &mut titulada);
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

/// [069A-5] Banner del REPL extraído de `bucle_chat` (deuda funcion-larga).
fn anunciar_sesion(
    workspace: &Option<std::path::PathBuf>,
    config: &glory_harness_core::runtime::TurnoConfig,
) {
    let raiz = workspace
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    println!(
        "glory-harness chat — modelo {}/{} · workspace {raiz}",
        config.provider, config.modelo
    );
    println!("escribe un mensaje, o /ayuda para los comandos");
}

/// [069A-5] Titulado de `/nuevo` extraído de `bucle_chat` (deuda
/// funcion-larga): la fila nueva toma el primer mensaje como título (60
/// caracteres); el resume conserva el suyo. No bloquea el turno.
fn titular_si_nueva(
    sqlite: &std::sync::Arc<crate::persistencia_sqlite::PersistenciaSqlite>,
    conversacion_id: Uuid,
    user_id: Uuid,
    texto: &str,
    titulada: &mut bool,
) {
    if *titulada {
        return;
    }
    *titulada = true;
    let titulo: String = texto.chars().take(60).collect();
    if let Err(e) = sqlite.conversacion_renombrar(conversacion_id, user_id, &titulo) {
        eprintln!("[chat] no se pudo titular la conversación: {e}");
    }
}

/// [069A-5] `/nuevo` del REPL extraído de `bucle_chat` (deuda funcion-larga):
/// crea la fila nueva (la anterior queda en la BD para `session resume`) y
/// resetea el estado de sesión; el título llega con el primer mensaje. Un
/// fallo no tumba el REPL: se sigue en la conversación actual.
fn nueva_conversacion(
    sqlite: &std::sync::Arc<crate::persistencia_sqlite::PersistenciaSqlite>,
    user_id: Uuid,
    conversacion_id: &mut Uuid,
    transcripcion: &mut Vec<ItemExport>,
    titulada: &mut bool,
) {
    match sqlite.conversacion_crear(user_id, "sesión") {
        Ok(id) => {
            *conversacion_id = id;
            *titulada = false;
            transcripcion.clear();
            println!("[chat] conversación nueva (el agente ya no recuerda lo anterior)");
        }
        Err(e) => eprintln!("[chat] no se pudo crear la conversación: {e}"),
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
    /* [069A-4] Memoria a largo plazo antepuesta al hilo (respeta los flags
     * del turno; un fallo avisa y el turno sigue sin recuerdos). */
    let historial = crate::memoria::anteponer_memoria(
        historial,
        crate::memoria::bloque_memoria_para_turno(
            &ctx.persistencia,
            ctx.user_id,
            texto,
            ctx.runtime.turno_config.incluir_memoria,
            ctx.runtime.turno_config.incluir_skills,
        )
        .await,
    );

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
            /* [069A-4] Sync post-turno (mejor esfuerzo con aviso). */
            crate::memoria::sincronizar_memoria_tras_turno(
                &ctx.persistencia,
                ctx.user_id,
                &respuesta.texto,
                texto,
                "turno:chat",
            )
            .await;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn mensaje(rol: &str, contenido: &str) -> glory_harness_core::ports::MensajePersistido {
        glory_harness_core::ports::MensajePersistido {
            id: Uuid::new_v4(),
            conversacion_id: Uuid::new_v4(),
            rol: rol.to_string(),
            contenido: contenido.to_string(),
            creado_en: chrono::Utc::now(),
        }
    }

    #[test]
    fn recompone_transcripcion_para_resume() {
        let mensajes = vec![
            mensaje("user", "hola"),
            mensaje("assistant", "buenas"),
            mensaje("user", "sigue"),
        ];
        let t = transcripcion_desde_mensajes(&mensajes);
        assert_eq!(t.len(), 3);
        // La forma exacta la valida exportar: basta orden y contenido.
        let md = crate::exportar::render_markdown(Uuid::new_v4(), &t, chrono::Utc::now());
        assert!(md.contains("hola") && md.contains("buenas") && md.contains("sigue"));
    }

    #[test]
    fn conversacion_vacia_da_transcripcion_vacia() {
        assert!(transcripcion_desde_mensajes(&[]).is_empty());
    }
}
