/* [29-08-2026] Framework de tools del agente (plan-agente-ia-plugin, Fase 0).
 * OCP: las tools se registran en `AgentToolRegistry`; el runtime solo conoce el
 * trait. El LLM solo ve el JSON Schema; el runtime solo ve `ejecutar`.
 *
 * Portado a Glory Harness (plan 318A-13, Fase 1c): el contexto ya no lleva
 * tipos concretos de task (`PgPool`, `WebSearchService`, `LlmProviderService`)
 * sino puertos del núcleo. Las tools de dominio del consumidor (crear_tarea,
 * crear_habito, ...) reciben sus servicios por `dominio` (slot opaco que el
 * consumidor downcastea); el núcleo nunca lo interpreta (DIP). */

use crate::error::{Error, Result};
use crate::ports::{AgentPersistence, ProviderPort, WebSearchProvider};
use crate::sandbox::SandboxArchivos;
use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Contexto que recibe cada tool al ejecutarse. El núcleo solo expone puertos
/// (persistencia, búsqueda web, proveedor LLM) y el sandbox de archivos; los
/// servicios de dominio del consumidor viajan en `dominio` (opaco al núcleo).
pub struct AgentToolContext<'a> {
    pub user_id: Uuid,
    /// Puerto de persistencia (turnos, mensajes, memoria, skills, tareas
    /// programadas). El runtime audita las acciones por aquí.
    pub persistencia: &'a dyn AgentPersistence,
    /// Búsqueda web. `None` si el consumidor no aporta proveedor: las tools
    /// que la necesiten fallan con error claro (nunca falso éxito).
    pub web_search: Option<&'a dyn WebSearchProvider>,
    /// Proveedor LLM (para tools que necesiten generar texto). `None` igual.
    pub ai_provider: Option<&'a dyn ProviderPort>,
    /// Sandbox de archivos (Fase 2). `None` en producción: las tools de
    /// archivo no existen (fail-closed, ni siquiera admin).
    pub sandbox_archivos: Option<Arc<SandboxArchivos>>,
    /// Slot de extensión para tools de dominio del consumidor: task inyecta
    /// aquí sus servicios (p. ej. `&PgPool` + repos), y sus tools hacen
    /// `downcast_ref`. El núcleo no interpreta este tipo.
    pub dominio: Option<&'a (dyn Any + Send + Sync)>,
}

/// Resultado de ejecutar una tool: texto legible para el LLM + estado.
#[derive(Debug, Clone)]
pub struct AgentToolResult {
    pub ok: bool,
    pub contenido: String,
    /// Resumen corto para auditoría (sin secretos, sin contenido largo).
    pub resumen: String,
    /// [31-08-2026] Fase 4: diff de líneas del cambio (file_write/file_patch)
    /// para mostrarlo en el front; `None` si no aplica.
    pub diff: Option<String>,
}

impl AgentToolResult {
    #[must_use]
    pub fn ok(contenido: impl Into<String>, resumen: impl Into<String>) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff: None,
        }
    }

    /// Resultado ok con diff de líneas (para tools que modifican archivos).
    #[must_use]
    pub fn ok_con_diff(
        contenido: impl Into<String>,
        resumen: impl Into<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff,
        }
    }

    #[must_use]
    pub fn error(contenido: impl Into<String>) -> Self {
        Self {
            ok: false,
            contenido: contenido.into(),
            resumen: "error".to_string(),
            diff: None,
        }
    }
}

/// Contrato de una tool del agente. `schema` es JSON Schema (objeto con
/// `properties`/`required`); el runtime valida los argumentos contra él antes
/// de ejecutar.
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn id(&self) -> &'static str;
    fn descripcion(&self) -> &'static str;
    fn schema(&self) -> Value;
    /// ¿Tiene efectos (escribe/borra)? Las tools con efecto en modo
    /// predeterminado requieren aprobación (diferenciado por la política de
    /// modos, sección 9.2). Por defecto false: la mayoría de las tools de
    /// dominio del v1 son de datos propios y se auditan, no se bloquean.
    fn efecto(&self) -> bool {
        false
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult>;
}

/// Registro de tools: registrar_tool() en el arranque; listar_schemas() para el
/// request al LLM; ejecutar() con validación de schema.
pub struct AgentToolRegistry {
    tools: HashMap<&'static str, Box<dyn AgentTool>>,
    /// Sandbox compartido (Fase 2). Se fija una vez por runtime; el runtime lo
    /// inyecta en el contexto al ejecutar tools.
    sandbox_archivos: Option<Arc<SandboxArchivos>>,
}

impl Default for AgentToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            sandbox_archivos: None,
        }
    }

    pub fn registrar(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.insert(tool.id(), tool);
    }

    /// Fija el sandbox de archivos del runtime (solo AGENTE_MODO=local).
    pub fn registrar_sandbox(&mut self, sandbox: Arc<SandboxArchivos>) {
        self.sandbox_archivos = Some(sandbox);
    }

    #[must_use]
    pub fn sandbox(&self) -> Option<Arc<SandboxArchivos>> {
        self.sandbox_archivos.clone()
    }

    #[must_use]
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.tools.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Schemas en formato OpenAI `tools` para el request al LLM.
    #[must_use]
    pub fn schemas_openai(&self, solo_ids: Option<&[&str]>) -> Vec<Value> {
        let mut schemas: Vec<Value> = self
            .tools
            .iter()
            .filter(|(id, _)| solo_ids.map(|ids| ids.contains(id)).unwrap_or(true))
            .map(|(id, tool)| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": id,
                        "description": tool.descripcion(),
                        "parameters": tool.schema(),
                    }
                })
            })
            .collect();
        schemas.sort_by(|a, b| {
            a["function"]["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["function"]["name"].as_str().unwrap_or(""))
        });
        schemas
    }

    /// ¿La tool tiene efectos (escribe/borra)? Para la política de modos.
    #[must_use]
    pub fn tiene_efecto(&self, tool_id: &str) -> bool {
        self.tools.get(tool_id).map(|t| t.efecto()).unwrap_or(false)
    }

    pub async fn ejecutar(
        &self,
        tool_id: &str,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let tool = self
            .tools
            .get(tool_id)
            .ok_or_else(|| Error::ToolDesconocida(tool_id.to_string()))?;
        validar_contra_schema(tool.schema(), &argumentos)?;
        tool.ejecutar(ctx, argumentos).await
    }
}

/// Validación mínima de JSON Schema (object + properties + required). Suficiente
/// para el contrato declarativo de v1; se puede ampliar sin romper el trait.
fn validar_contra_schema(schema: Value, argumentos: &Value) -> Result<()> {
    if !argumentos.is_object() {
        return Err(Error::Argumentos(
            "Los argumentos de la tool deben ser un objeto JSON".into(),
        ));
    }
    if let Some(requeridos) = schema.get("required").and_then(Value::as_array) {
        for requerido in requeridos {
            if let Some(nombre) = requerido.as_str() {
                if argumentos.get(nombre).is_none() {
                    return Err(Error::Argumentos(format!(
                        "Falta el argumento requerido: {nombre}"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct ToolEcho;

    #[async_trait]
    impl AgentTool for ToolEcho {
        fn id(&self) -> &'static str {
            "echo"
        }
        fn descripcion(&self) -> &'static str {
            "Devuelve el texto recibido"
        }
        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"texto": {"type": "string"}},
                "required": ["texto"]
            })
        }
        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            argumentos: Value,
        ) -> Result<AgentToolResult> {
            let texto = argumentos["texto"].as_str().unwrap_or("").to_string();
            Ok(AgentToolResult::ok(texto.clone(), "echo"))
        }
    }

    #[tokio::test]
    async fn registra_y_lista_schemas() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(ToolEcho));
        assert_eq!(registry.ids(), vec!["echo"]);
        let schemas = registry.schemas_openai(None);
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["function"]["name"], "echo");
    }

    #[test]
    fn valida_argumentos_requeridos() {
        let schema = json!({
            "type": "object",
            "properties": {"texto": {"type": "string"}},
            "required": ["texto"]
        });
        let err = validar_contra_schema(schema.clone(), &json!({})).unwrap_err();
        assert!(err.to_string().contains("requerido"));
        // Con el argumento presente, pasa.
        assert!(validar_contra_schema(schema, &json!({ "texto": "hola" })).is_ok());
        // No-objeto rechazado.
        assert!(validar_contra_schema(json!({}), &json!([1, 2])).is_err());
    }

}