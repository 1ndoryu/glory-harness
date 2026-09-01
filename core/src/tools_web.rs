/* [29-08-2026] Tools agnósticas de red (plan-agente-ia-plugin, Fase 1).
 * `web_search` es la única tool de red: usa el puerto `WebSearchProvider`
 * (el consumidor aporta el servicio con su proveedor y límites).
 * Portada a Glory Harness (plan 318A-13, Fase 1c): sin WebSearchService
 * concreto; error claro si el contexto no trae proveedor (nunca falso éxito). */

use crate::error::{Error, Result};
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ToolWebSearch;

#[async_trait]
impl AgentTool for ToolWebSearch {
    fn id(&self) -> &'static str {
        "web_search"
    }
    fn descripcion(&self) -> &'static str {
        "Busca información actual en internet y devuelve resultados resumidos."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {"type": "string", "description": "Consulta de búsqueda"}
            },
            "required": ["query"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let query = argumentos
            .get("query")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("query requerido".into()))?
            .to_string();
        let proveedor = ctx.web_search.ok_or_else(|| {
            Error::Validacion(
                "web_search no está disponible: el consumidor no aportó proveedor de búsqueda"
                    .into(),
            )
        })?;
        let resultados = proveedor.buscar(&query, 5).await?;
        let resumen = format!("{} resultados para '{}'", resultados.len(), query);
        let contenido = if resultados.is_empty() {
            "Sin resultados.".to_string()
        } else {
            resultados
                .iter()
                .take(5)
                .map(|r| format!("- {}: {}", r.titulo, r.url))
                .collect::<Vec<_>>()
                .join("\n")
        };
        Ok(AgentToolResult::ok(contenido, resumen))
    }
}

/// Registra las tools de red agnósticas en el registry.
pub fn registrar_tools_red(registry: &mut crate::tool::AgentToolRegistry) {
    registry.registrar(Box::new(ToolWebSearch));
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::contrato_tests::{PersistenciaMock, WebMock};
    use crate::ports::WebSearchProvider;
    use serde_json::json;

    /// Contexto de tool para tests: sin sandbox, sin proveedor LLM, sin
    /// extensión de dominio; la persistencia es el mock del contrato.
    fn ctx<'a>(
        persistencia: &'a PersistenciaMock,
        web_search: Option<&'a dyn WebSearchProvider>,
    ) -> AgentToolContext<'a> {
        AgentToolContext {
            user_id: Uuid::new_v4(),
            persistencia,
            web_search,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
        }
    }

    #[tokio::test]
    async fn falla_claro_sin_proveedor() {
        let persistencia = PersistenciaMock::default();
        let cxt = ctx(&persistencia, None);
        let err = ToolWebSearch
            .ejecutar(&cxt, json!({"query": "glory"}))
            .await
            .expect_err("sin proveedor debe fallar");
        assert!(
            err.to_string().contains("no está disponible"),
            "error claro: {err}"
        );
    }

    #[tokio::test]
    async fn busca_con_proveedor_del_contrato() {
        let persistencia = PersistenciaMock::default();
        let web = WebMock;
        let cxt = ctx(&persistencia, Some(&web));
        let resultado = ToolWebSearch
            .ejecutar(&cxt, json!({"query": "nakomi"}))
            .await
            .expect("con proveedor debe responder");
        assert!(resultado.ok);
        assert!(resultado.contenido.contains("nakomi"));
    }

    #[tokio::test]
    async fn rechaza_consulta_vacia() {
        let persistencia = PersistenciaMock::default();
        let cxt = ctx(&persistencia, None);
        let err = ToolWebSearch
            .ejecutar(&cxt, json!({}))
            .await
            .expect_err("query requerida se valida antes del proveedor");
        assert!(err.to_string().contains("query requerido"));
    }
}