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
        "Busca información ACTUAL en internet y devuelve resultados resumidos (título + URL).\nFORMATO DE SALIDA: hasta 5 resultados, cada uno '- título: url'.\nLÍMITES: solo texto (sin descarga de páginas; eso es web_fetch); depende del proveedor del consumidor.\nCUÁNDO USARLA: datos recientes (noticias, docs, precios), verificar supuestos o cuando el contexto no alcanza. Para fechas o decisiones del workspace NO la uses.\nERRORES: sin proveedor configurado o sin resultados → mensaje claro, nunca éxito falso."
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

/// [Bloque 3, F1] `web_fetch`: descarga UNA url a texto limpio (distinta de
/// `web_search`, que devuelve resultados). Requiere el puerto `WebFetchProvider`
/// del consumidor; sin él falla con error claro (nunca falso éxito). El límite
/// de bytes lo impone el proveedor (el argumento opcional solo lo solicita).
pub struct ToolWebFetch;

#[async_trait]
impl AgentTool for ToolWebFetch {
    fn id(&self) -> &'static str {
        "web_fetch"
    }
    fn descripcion(&self) -> &'static str {
        "Descarga UNA página web (URL) y devuelve su texto legible acotado.\nFORMATO DE SALIDA: título + texto limpio (sin HTML/scripts), hasta ~20 KB por defecto.\nDIFERENCIA CON web_search: web_search devuelve RESULTADOS de búsqueda; web_fetch lee el CONTENIDO de una url concreta que ya conoces (doc oficial, issue, página).\nCUÁNDO USARLA: necesitas el detalle de una página específica citada en resultados o contexto.\nERRORES: url inválida, sin proveedor configurado o página no legible → mensaje claro, nunca éxito falso."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {"type": "string", "description": "URL absoluta a descargar"},
                "limite_bytes": {"type": "integer", "description": "Límite de texto devuelto (opcional, default 20000)"}
            },
            "required": ["url"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let url = argumentos
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("url requerido".into()))?
            .to_string();
        let limite = argumentos
            .get("limite_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(20_000)
            .min(200_000) as usize;
        let proveedor = ctx.web_fetch.ok_or_else(|| {
            Error::Validacion(
                "web_fetch no está disponible: el consumidor no aportó proveedor de descarga HTTP"
                    .into(),
            )
        })?;
        let contenido = proveedor.obtener(&url, limite).await?;
        let cuerpo = if contenido.texto.trim().is_empty() {
            "(página sin texto legible)".to_string()
        } else {
            contenido.texto
        };
        let cabecera = match &contenido.titulo {
            Some(t) if !t.trim().is_empty() => format!("# {}\n\n", t.trim()),
            _ => String::new(),
        };
        let resumen = format!("web_fetch {} ({} bytes)", contenido.url, contenido.bytes);
        Ok(AgentToolResult::ok(format!("{cabecera}{cuerpo}"), resumen))
    }
}

/// Registra las tools de red agnósticas en el registry.
pub fn registrar_tools_red(registry: &mut crate::tool::AgentToolRegistry) {
    registry.registrar(Box::new(ToolWebSearch));
    registry.registrar(Box::new(ToolWebFetch));
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    use crate::contrato_tests::{PersistenciaMock, WebMock};
    use crate::ports::{ContenidoWeb, WebFetchProvider, WebSearchProvider};

    /// Proveedor de descarga de prueba: devuelve contenido acotado al límite.
    struct FetchMock;

    #[async_trait]
    impl WebFetchProvider for FetchMock {
        async fn obtener(&self, url: &str, limite_bytes: usize) -> Result<ContenidoWeb> {
            let texto = format!("texto legible de {url}");
            let texto = texto.chars().take(limite_bytes).collect::<String>();
            Ok(ContenidoWeb {
                url: url.into(),
                titulo: Some("Página de prueba".into()),
                bytes: texto.len(),
                texto,
            })
        }
    }
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
            web_fetch: Some(&FetchMock),
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
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

    #[tokio::test]
    async fn web_fetch_falla_claro_sin_proveedor() {
        let persistencia = PersistenciaMock::default();
        let cxt = AgentToolContext {
            user_id: Uuid::new_v4(),
            persistencia: &persistencia,
            web_search: None,
            web_fetch: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        };
        let err = ToolWebFetch
            .ejecutar(&cxt, json!({"url": "https://ejemplo.test"}))
            .await
            .expect_err("sin proveedor debe fallar");
        assert!(
            err.to_string().contains("no está disponible"),
            "error claro: {err}"
        );
    }

    #[tokio::test]
    async fn web_fetch_descarga_con_proveedor() {
        let persistencia = PersistenciaMock::default();
        let cxt = ctx(&persistencia, None);
        let resultado = ToolWebFetch
            .ejecutar(&cxt, json!({"url": "https://ejemplo.test/pagina"}))
            .await
            .expect("con proveedor debe descargar");
        assert!(resultado.ok);
        assert!(resultado.contenido.contains("Página de prueba"));
        assert!(resultado.contenido.contains("texto legible"));
        assert!(resultado.resumen.contains("ejemplo.test"));
    }

    #[tokio::test]
    async fn web_fetch_valida_url_antes_del_proveedor() {
        let persistencia = PersistenciaMock::default();
        let cxt = ctx(&persistencia, None);
        let err = ToolWebFetch
            .ejecutar(&cxt, json!({}))
            .await
            .expect_err("url requerida");
        assert!(err.to_string().contains("url requerido"));
    }
}
