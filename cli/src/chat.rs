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
use glory_harness_core::llm::{AiMessage, LlmProviderService, LlavesProveedor};
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::runtime::{AgentRuntime, PuertosHarness};
use glory_harness_core::tool::AgentToolRegistry;
use glory_harness_core::AgentPersistence;

use crate::persistencia::PersistenciaMemoria;
use crate::run::{quitar_prefijo_verbatim, turno_config_default, OpcionesRun};

/// Convierte los mensajes persistidos de una conversación en el historial que
/// el runtime espera (`AiMessage`). Es la fuente entre turnos del chat: el
/// agente recuerda el hilo porque cada turno recibe todo lo anterior.
fn historial_desde_persistencia(mensajes: Vec<MensajePersistido>) -> Vec<AiMessage> {
    mensajes
        .into_iter()
        .map(|m| AiMessage::texto(&m.rol, m.contenido))
        .collect()
}

/// Ejecuta el subcomando `chat`: abre la sesión interactiva y no devuelve
/// hasta que el usuario salga (`/salir`, Ctrl+C o EOF).
pub async fn chat(opciones: OpcionesRun) -> Result<(), String> {
    let persistencia = Arc::new(PersistenciaMemoria::nuevo());
    let user_id = Uuid::new_v4();
    persistencia.con_skills_base(user_id);

    let workspace = opciones
        .dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.canonicalize().unwrap_or(p))
        .map(quitar_prefijo_verbatim);

    /* Tools de archivo sobre el workspace (igual que `run`): el núcleo solo
     * las activa con AGENTE_MODO=local. */
    if std::env::var_os("AGENTE_MODO").is_none() {
        // edition 2021: set_var es seguro (sin unsafe).
        std::env::set_var("AGENTE_MODO", "local");
    }

    let llm = Arc::new(LlmProviderService::new(LlavesProveedor::from_env()));
    let persistencia_port: Arc<dyn AgentPersistence> = persistencia.clone();

    let mut config = turno_config_default(workspace.clone());
    if let Some(provider) = opciones.provider.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        config.provider = provider.to_string();
    }
    if let Some(modelo) = opciones.modelo.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        config.modelo = modelo.to_string();
    }

    let runtime = Arc::new(AgentRuntime::nuevo(
        AgentToolRegistry::new(),
        PuertosHarness {
            persistencia: persistencia_port,
            llm,
            web_search: None,
            dominio: None,
        },
        config.clone(),
    ));

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
    loop {
        print!("gh> ");
        let _ = std::io::stdout().flush();

        let linea = tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                println!();
                return Ok(());
            }
            opt = rx_lineas.recv() => match opt {
                Some(Some(l)) => l,
                _ => return Ok(()), // EOF o canal cerrado
            },
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

        let mut respuesta = String::new();
        let mut tools = Vec::new();
        while let Some(evento) = rx.recv().await {
            match evento {
                AgenteEvento::Token { texto: t } => respuesta.push_str(&t),
                AgenteEvento::ToolStart { tool, .. } => {
                    eprintln!("  ⏱ {tool}");
                    tools.push(tool);
                }
                AgenteEvento::RequiereAprobacion { tool, .. } => {
                    eprintln!("  ⚠ {tool} requiere aprobación (modo predeterminado)");
                }
                AgenteEvento::ToolResult {
                    tool,
                    ok: false,
                    resumen,
                    ..
                } => {
                    eprintln!("  ✗ {tool}: {resumen}");
                }
                AgenteEvento::Error { mensaje, .. } => {
                    eprintln!("  ✗ error: {mensaje}");
                }
                AgenteEvento::Done { .. } => break,
                _ => {}
            }
        }

        /* El runtime persiste ambos mensajes vía puerto (`guardar_mensaje`);
         * aquí solo se reporta el resultado. Un fallo no acaba el chat: se
         * muestra y se vuelve al prompt para reintentar. */
        let resultado = handle.await;
        match resultado {
            Ok(Ok(())) => {
                if !tools.is_empty() {
                    eprintln!("  tools: {}", tools.join(", "));
                }
                println!();
                println!("{respuesta}");
                println!();
            }
            Ok(Err(err)) => eprintln!("[chat] el turno falló: {err} (puedes reintentar)"),
            Err(err) => eprintln!("[chat] el turno abortó con pánico: {err}"),
        }
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