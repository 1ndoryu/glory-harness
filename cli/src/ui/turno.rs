//! [318A-17 B3-F5] Maquinaria de turno compartida entre el REPL lineal
//! (`chat`) y la TUI (`chat --tui`): conversión del historial persistido,
//! ejecución de un turno sobre el contrato `AgenteEvento` y resultado
//! tipado. Extraída de `chat.rs` para que ambas superficies tengan una sola
//! fuente sin duplicar el bucle ni el observador de eventos.
//!
//! Es un cliente más sobre la API del núcleo: no toca el runtime.

use std::sync::Arc;

use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::AiMessage;
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::runtime::AgentRuntime;

use crate::ui::exportar::HerramientaEjecutada;

/// Convierte los mensajes persistidos de una conversación en el historial que
/// el runtime espera (`AiMessage`). Es la fuente entre turnos del chat: el
/// agente recuerda el hilo porque cada turno recibe todo lo anterior.
/// Compartido con la TUI y la app de escritorio: misma fuente, otra UI.
pub fn historial_desde_persistencia(mensajes: Vec<MensajePersistido>) -> Vec<AiMessage> {
    mensajes
        .into_iter()
        .map(|m| AiMessage::texto(&m.rol, m.contenido))
        .collect()
}

/// Resultado de un turno de chat: texto del asistente + tools ejecutadas.
/// Compartido entre el REPL lineal y la TUI (`chat --tui`). Los errores se
/// muestran en vivo por el callback de eventos, no se acumulan aquí.
pub struct TurnoResultado {
    pub texto: String,
    pub tools: Vec<String>,
    /// [318A-17 B3-F5] Resultados de las herramientas del turno (eventos
    /// `ToolResult`), para la transcripción que alimenta `/export`.
    pub herramientas: Vec<HerramientaEjecutada>,
}

/// Ejecuta un turno sobre la conversación y consume el contrato `AgenteEvento`
/// con un callback por evento (cada UI decide cómo pintarlo: el REPL imprime
/// en vivo; la TUI lo acumula en sus paneles). El runtime persiste ambos
/// mensajes vía puerto; aquí solo se recolecta el resultado. Un fallo se
/// devuelve como `Err` y no acaba la sesión: el llamador puede reintentar.
pub async fn procesar_turno(
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
    let mut herramientas = Vec::new();
    while let Some(evento) = rx.recv().await {
        match &evento {
            AgenteEvento::Token { texto: t } => texto_respuesta.push_str(t),
            AgenteEvento::ToolStart { tool, .. } => tools.push(tool.clone()),
            /* [318A-17 B3-F5] Captura de eventos del turno para `/export`:
             * cada tool cerrada entra con su estado (ok/error), resumen y
             * diff opcional, igual que verá el Markdown. */
            AgenteEvento::ToolResult {
                tool,
                ok,
                resumen,
                diff,
            } => herramientas.push(HerramientaEjecutada {
                tool: tool.clone(),
                ok: *ok,
                resumen: resumen.clone(),
                diff: diff.clone(),
            }),
            AgenteEvento::Done { .. } => break,
            _ => {}
        }
        on_evento(evento);
    }

    match handle.await {
        Ok(Ok(())) => Ok(TurnoResultado {
            texto: texto_respuesta,
            tools,
            herramientas,
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
        assert_eq!(
            historial[0].content,
            serde_json::Value::String("primero".into())
        );
        assert_eq!(historial[1].role, "assistant");
        assert_eq!(
            historial[1].content,
            serde_json::Value::String("respuesta".into())
        );
        assert_eq!(
            historial[2].content,
            serde_json::Value::String("segundo".into())
        );
    }

    #[test]
    fn historial_vacio_es_vacio() {
        assert!(historial_desde_persistencia(vec![]).is_empty());
    }
}
