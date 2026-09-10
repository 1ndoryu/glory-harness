/* [04-09-2026] Tool `ask_user` (Bloque 3, Fase 1): pregunta con opciones al
 * usuario en medio del turno (evidencia: claurst `tools/ask_user.rs`,
 * opencode `tool/question.ts`, grok `side-question.ts`).
 *
 * El runtime la intercepta por nombre (patrón `task` de 318A-15 F4): valida
 * argumentos, registra la pregunta pendiente en el registro (Arc compartido,
 * mismo patrón que las aprobaciones de 318A-16 F2), emite el evento SSE
 * `Pregunta` y TERMINA el turno — la respuesta del usuario llega como un
 * nuevo mensaje de usuario en el siguiente turno. El modelo nunca asume la
 * respuesta: el resultado de tool se lo indica explícitamente.
 *
 * La ejecución directa es un error (igual que `task`): la pregunta solo
 * existe dentro de un turno del runtime. */

use crate::error::{Error, Result};
use crate::evento::AgenteEvento;
use crate::llm::AiToolCall;
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

/// Pregunta pendiente al usuario. Se responde como nuevo mensaje de usuario
/// (el `id` permite correlacionar en la UI); `opciones` vacío = respuesta
/// libre.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreguntaPendiente {
    pub id: String,
    pub texto: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub opciones: Vec<String>,
}

impl PreguntaPendiente {
    #[must_use]
    pub fn nueva(id: impl Into<String>, texto: impl Into<String>, opciones: Vec<String>) -> Self {
        Self {
            id: id.into(),
            texto: texto.into(),
            opciones,
        }
    }
}

/// Tool `ask_user`: pregunta con opciones y respuesta acotada. El runtime la
/// intercepta en el bucle del turno (patrón `task`); la ejecución directa es
/// un error.
pub struct ToolAskUser;

#[async_trait]
impl AgentTool for ToolAskUser {
    fn id(&self) -> &'static str {
        "ask_user"
    }

    fn descripcion(&self) -> &'static str {
        "Haz una pregunta con opciones al usuario en medio del turno cuando necesites una decisión que solo él puede tomar. Devuelve la pregunta enviada; el turno termina y la respuesta del usuario llegará como su siguiente mensaje. No la uses para información que puedas buscar o inferir."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "texto": {
                    "type": "string",
                    "description": "La pregunta concreta, con contexto mínimo y opciones de decisión claras."
                },
                "opciones": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "(opcional) Opciones sugeridas de respuesta; vacío = respuesta libre."
                }
            },
            "required": ["texto"]
        })
    }

    fn efecto(&self) -> bool {
        false
    }

    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        _argumentos: Value,
    ) -> Result<AgentToolResult> {
        Err(Error::Validacion(
            "la tool 'ask_user' se ejecuta a través del runtime (evento Pregunta); no puede invocarse directamente"
                .into(),
        ))
    }
}

/// Registra la tool `ask_user` en el registro (el runtime la intercepta).
pub fn registrar_tool_ask_user(registry: &mut AgentToolRegistry) {
    registry.registrar(Box::new(ToolAskUser));
}

/// Procesa una llamada interceptada a `ask_user` (patrón `task` de 318A-15
/// F4, pero en función libre para que el test sea determinista sin LLM):
/// valida argumentos → registra la pregunta pendiente → emite el evento
/// `Pregunta` → devuelve el resultado de tool que instruye al modelo a NO
/// asumir la respuesta. El turno termina (lo decide el runtime, no la tool).
pub async fn procesar_pregunta(
    registry: &AgentToolRegistry,
    call: &AiToolCall,
    tx: &Sender<AgenteEvento>,
) -> Result<AgentToolResult> {
    let texto = call
        .argumentos
        .get("texto")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .ok_or_else(|| Error::Validacion("ask_user: 'texto' es obligatorio".into()))?;
    let opciones: Vec<String> = call
        .argumentos
        .get("opciones")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let id = Uuid::new_v4().to_string();
    registry.registrar_pregunta(PreguntaPendiente::nueva(&id, texto, opciones.clone()));
    let _ = tx
        .send(AgenteEvento::Pregunta {
            id: id.clone(),
            texto: texto.to_owned(),
            opciones,
        })
        .await;
    Ok(AgentToolResult {
        ok: true,
        contenido: format!(
            "Pregunta enviada al usuario (id {id}): el turno termina y su respuesta llegará como el siguiente mensaje. NO asumas la respuesta."
        ),
        resumen: "pregunta_enviada".into(),
        diff: None,
        evento_extra: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_pide_texto_y_opciones_opcionales() {
        let schema = ToolAskUser.schema();
        let props = schema["properties"].as_object().expect("properties");
        assert!(props.contains_key("texto"), "texto obligatorio");
        assert!(props.contains_key("opciones"), "opciones opcional");
        let requeridos = schema["required"].as_array().expect("required");
        assert_eq!(requeridos, &vec![json!("texto")]);
    }

    #[test]
    fn ejecucion_directa_es_error() {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let error = rt.block_on(async {
            // Contexto mínimo (ningún puerto se toca: la tool falla antes).
            let ctx = AgentToolContext {
                ambito_memoria: crate::ports::AmbitoMemoria::Global,
                user_id: uuid::Uuid::new_v4(),
                persistencia: &crate::contrato_tests::PersistenciaMock::default(),
                web_search: None,
                web_fetch: None,
                ai_provider: None,
                sandbox_archivos: None,
                dominio: None,
                todo: None,
                plan: None,
                navegador: None,
            };
            ToolAskUser
                .ejecutar(&ctx, json!({"texto": "¿sí o no?"}))
                .await
        });
        let error = error.expect_err("debe fallar sin runtime");
        assert!(error.to_string().contains("a través del runtime"));
    }

    #[test]
    fn pregunta_pendiente_serde_ida_y_vuelta() {
        let pregunta =
            PreguntaPendiente::nueva("p-1", "¿Continúo?", vec!["sí".into(), "no".into()]);
        let json = serde_json::to_value(&pregunta).expect("serializa");
        assert_eq!(json["id"], "p-1");
        assert_eq!(json["texto"], "¿Continúo?");
        assert_eq!(json["opciones"].as_array().map(Vec::len), Some(2));
        let vuelta: PreguntaPendiente = serde_json::from_value(json).expect("deserializa");
        assert_eq!(vuelta.id, "p-1");
        assert_eq!(vuelta.opciones, vec!["sí", "no"]);
    }

    /* Intercepción del runtime (sin LLM): valida → registra → emite el
     * evento `Pregunta` → el resultado instruye al modelo a no asumir. */
    #[tokio::test]
    async fn intercepcion_valida_emite_y_registra() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(8);
        let mut registry = AgentToolRegistry::new();
        registrar_tool_ask_user(&mut registry);

        let call = AiToolCall {
            id: "call-1".into(),
            nombre: "ask_user".into(),
            argumentos: json!({"texto": "¿Continúo?", "opciones": ["sí", "no"]}),
        };
        let resultado = procesar_pregunta(&registry, &call, &tx)
            .await
            .expect("pregunta");
        assert!(resultado.ok);
        assert!(
            resultado.contenido.contains("NO asumas"),
            "el modelo debe saber que no debe asumir la respuesta"
        );
        let ev = rx.recv().await.expect("evento Pregunta");
        match ev {
            AgenteEvento::Pregunta {
                id,
                texto,
                opciones,
            } => {
                assert_eq!(texto, "¿Continúo?");
                assert_eq!(opciones, vec!["sí", "no"]);
                /* Queda registrada y consumible una sola vez por id. */
                assert_eq!(registry.preguntas_pendientes().len(), 1);
                assert!(registry.responder_pregunta(&id).is_ok());
                assert!(registry.preguntas_pendientes().is_empty());
            }
            other => panic!("evento inesperado: {other:?}"),
        }
    }

    #[tokio::test]
    async fn intercepcion_exige_texto() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let registry = AgentToolRegistry::new();
        let call = AiToolCall {
            id: "call-2".into(),
            nombre: "ask_user".into(),
            argumentos: json!({}),
        };
        let error = procesar_pregunta(&registry, &call, &tx)
            .await
            .expect_err("sin texto");
        assert!(error.to_string().contains("texto"));
        assert!(registry.preguntas_pendientes().is_empty());
    }

    /* Flujo del canal: registrar → pendiente visible → responder consume → ya
     * no está (mismo patrón que las aprobaciones F2, sin LLM: determinista). */
    #[test]
    fn canal_pregunta_registra_y_responde_una_vez() {
        let mut registry = AgentToolRegistry::new();
        registrar_tool_ask_user(&mut registry);
        assert!(registry.ids().contains(&"ask_user"));

        registry.registrar_pregunta(PreguntaPendiente::nueva(
            "p-1",
            "¿Aplico el cambio?",
            vec!["sí".into(), "no".into()],
        ));
        let pendientes = registry.preguntas_pendientes();
        assert_eq!(pendientes.len(), 1);
        assert_eq!(pendientes[0].id, "p-1");

        assert!(registry.responder_pregunta("p-1").is_ok());
        assert!(registry.preguntas_pendientes().is_empty());
        assert!(
            registry.responder_pregunta("p-1").is_err(),
            "responder dos veces debe fallar (id consumido)"
        );
    }
}
