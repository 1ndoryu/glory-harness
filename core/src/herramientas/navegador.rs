//! [069A-1 F5] Tool `navegador_reflejo` del agente: permite al modelo
//! navegar, capturar y manipular el navegador interno WebView2 child.
//!
//! Esta tool solo se registra cuando el consumidor aporta el puerto
//! [`NavegadorPort`]; sin él, la tool no existe (fail-closed: el modelo
//! ni la ve). Todas las operaciones emiten evento `ToolStart`/`ToolResult`
//! estándar; el front refleja las acciones en el panel navegador vía
//! [`AgenteEvento::ToolNavegador`] (emitido como evento adicional).

use crate::{
    error::{Error, Result},
    tool::{AgentTool, AgentToolContext, AgentToolResult},
};
use async_trait::async_trait;
use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Tool genérica que delega en `navegador_operacion`
// ---------------------------------------------------------------------------

/// Tool que expone operaciones del navegador interno al agente.
/// Usa un sub-campo `operacion` para distinguir la acción.
/// `navegador_abrir_url`, `navegador_ejecutar_js`, `navegador_capturar`, etc.
pub struct ToolNavegadorReflejo;

#[async_trait]
impl AgentTool for ToolNavegadorReflejo {
    fn id(&self) -> &str {
        "navegador_reflejo"
    }

    fn descripcion(&self) -> &str {
        "Controla el navegador interno (webview hija) para navegar, capturar, hacer \
         clic, rellenar formularios y ejecutar JavaScript.\n\n\
         OPERACIONES:\n\
         - `abrir`: Abre el navegador en una URL (ancho/alto opcionales).\n\
         - `navegar`: Navega la webview a una URL.\n\
         - `capturar`: Toma una captura PNG de la webview (devuelve Base64).\n\
         - `js`: Ejecuta JavaScript en la webview y devuelve el resultado.\n\
         - `cdp`: Invoca un método del DevTools Protocol.\n\
         - `click`: Hace clic en el primer elemento que coincide con un selector CSS.\n\
         - `rellenar`: Rellena un campo de formulario (selector + valor).\n\
         - `snapshot`: Toma un snapshot parcial del DOM.\n\
         - `cerrar`: Cierra el navegador.\n\n\
         LIMITACIONES: sin navegador abierto o sin puerto configurado → error claro,\n\
         nunca éxito falso. El tamaño de código JS está limitado a 128 KB.\n\
         La captura puede fallar en plataformas sin WebView2."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operacion": {
                    "type": "string",
                    "enum": ["abrir", "navegar", "capturar", "js", "cdp", "click", "rellenar", "snapshot", "cerrar"],
                    "description": "Operación a ejecutar en el navegador"
                },
                "url": {
                    "type": "string",
                    "description": "URL destino (para abrir/navegar)"
                },
                "selector": {
                    "type": "string",
                    "description": "Selector CSS (para click/rellenar/snapshot)"
                },
                "codigo": {
                    "type": "string",
                    "description": "Código JavaScript (para js)"
                },
                "metodo": {
                    "type": "string",
                    "description": "Método CDP (para cdp, ej: 'Runtime.evaluate')"
                },
                "parametros": {
                    "type": "string",
                    "description": "Parámetros JSON del método CDP (para cdp)"
                },
                "valor": {
                    "type": "string",
                    "description": "Valor a escribir (para rellenar)"
                },
                "ancho": {
                    "type": "integer",
                    "description": "Ancho en px (opcional, para abrir; default 800)"
                },
                "alto": {
                    "type": "integer",
                    "description": "Alto en px (opcional, para abrir; default 600)"
                }
            },
            "required": ["operacion"],
            "dependent_schemas": {
                "abrir": { "required": ["url"] },
                "navegar": { "required": ["url"] },
                "js": { "required": ["codigo"] },
                "cdp": { "required": ["metodo", "parametros"] },
                "click": { "required": ["selector"] },
                "rellenar": { "required": ["selector", "valor"] },
                "snapshot": { "required": ["selector"] },
                "capturar": {},
                "cerrar": {}
            }
        })
    }

    fn efecto(&self) -> bool {
        true
    }

    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let navegador = ctx.navegador.ok_or_else(|| {
            Error::Validacion(
                "navegador_reflejo no está disponible: sin navegador interno configurado".into(),
            )
        })?;

        let operacion = argumentos
            .get("operacion")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("operacion requerido".into()))?;

        match operacion {
            "abrir" => {
                let url = argumentos
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("url requerido para abrir".into()))?;
                navegador.abrir(url).await?;
                Ok(AgentToolResult::ok(
                    format!("Navegador abierto en {url}"),
                    "abrir navegador",
                ))
            },
            "navegar" => {
                let url = argumentos
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("url requerido para navegar".into()))?;
                navegador.navegar(url).await?;
                Ok(AgentToolResult::ok(
                    format!("Navegado a {url}"),
                    "navegar",
                ))
            },
                        "capturar" => {
                let base64_str = navegador.capturar().await?;
                // [069A-1 F6] Emitir evento ToolNavegador con la imagen
                // base64 para que el front la muestre en el panel.
                let tam = base64_str.len();
                let preview = if tam > 200 {
                    format!("{}... ({} bytes total)", &base64_str[..200], tam)
                } else {
                    base64_str.clone()
                };
                let evento_extra = crate::evento::AgenteEvento::ToolNavegador {
                    accion: "capturar".into(),
                    ok: true,
                    url: None,
                    selector: None,
                    captura_base64: Some(base64_str),
                    descripcion: format!("captura PNG ({tam} bytes base64)"),
                };
                Ok(AgentToolResult::ok_con_evento(
                    format!("Captura tomada: {preview}"),
                    format!("captura PNG ({tam} bytes base64)"),
                    evento_extra,
                ))
            },
            "js" => {
                let codigo = argumentos
                    .get("codigo")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("codigo requerido para js".into()))?;
                let resultado = navegador.js(codigo).await?;
                Ok(AgentToolResult::ok(
                    resultado,
                    "ejecutar JavaScript",
                ))
            },
            "cdp" => {
                let metodo = argumentos
                    .get("metodo")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("metodo requerido para cdp".into()))?;
                let parametros = argumentos
                    .get("parametros")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("parametros requerido para cdp".into()))?;
                let resultado = navegador.cdp(metodo, parametros).await?;
                Ok(AgentToolResult::ok(
                    resultado,
                    "CDP",
                ))
            },
            "click" => {
                let selector = argumentos
                    .get("selector")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("selector requerido para click".into()))?;
                navegador.click(selector).await?;
                Ok(AgentToolResult::ok(
                    format!("Click en {selector}"),
                    "click",
                ))
            },
            "rellenar" => {
                let selector = argumentos
                    .get("selector")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("selector requerido para rellenar".into()))?;
                let valor = argumentos
                    .get("valor")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("valor requerido para rellenar".into()))?;
                navegador.rellenar(selector, valor).await?;
                Ok(AgentToolResult::ok(
                    format!("Campo {selector} rellenado"),
                    "rellenar formulario",
                ))
            },
            "snapshot" => {
                let selector = argumentos
                    .get("selector")
                    .and_then(Value::as_str)
                    .ok_or_else(|| Error::Argumentos("selector requerido para snapshot".into()))?;
                let resultado = navegador.snapshot(selector).await?;
                Ok(AgentToolResult::ok(
                    resultado,
                    "snapshot DOM",
                ))
            },
            "cerrar" => {
                navegador.cerrar().await?;
                Ok(AgentToolResult::ok(
                    "Navegador cerrado",
                    "cerrar navegador",
                ))
            },
            _ => Err(Error::Argumentos(format!(
                "operación '{operacion}' no soportada. Válidas: abrir, navegar, capturar, \
                 js, cdp, click, rellenar, snapshot, cerrar"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::AgentToolRegistry;
    use crate::ports::NavegadorPort;
    use async_trait::async_trait;

    struct StubNavegador {
        abierto: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl NavegadorPort for StubNavegador {
        async fn abrir(&self, _url: &str) -> Result<()> {
            *self.abierto.lock().unwrap() = true;
            Ok(())
        }
        async fn navegar(&self, _url: &str) -> Result<()> { Ok(()) }
        async fn capturar(&self) -> Result<String> { Ok("stub_captura_base64".into()) }
        async fn js(&self, _codigo: &str) -> Result<String> { Ok("stub_resultado".into()) }
        async fn cdp(&self, _metodo: &str, _parametros: &str) -> Result<String> { Ok("{}".into()) }
        async fn click(&self, _selector: &str) -> Result<()> { Ok(()) }
        async fn rellenar(&self, _selector: &str, _valor: &str) -> Result<()> { Ok(()) }
        async fn snapshot(&self, _selector: &str) -> Result<String> { Ok("<html/>".into()) }
        async fn cerrar(&self) -> Result<()> {
            *self.abierto.lock().unwrap() = false;
            Ok(())
        }
    }

    fn ctx_con_navegador() -> AgentToolContext<'static> {
        AgentToolContext {
            user_id: uuid::Uuid::nil(),
            persistencia: &crate::PersistenciaMemoria::nuevo(),
            web_search: None,
            web_fetch: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: Some(&StubNavegador { abierto: std::sync::Mutex::new(false) }),
        }
    }

    fn ctx_sin_navegador() -> AgentToolContext<'static> {
        AgentToolContext {
            user_id: uuid::Uuid::nil(),
            persistencia: &crate::PersistenciaMemoria::nuevo(),
            web_search: None,
            web_fetch: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        }
    }

    #[tokio::test]
    async fn t01_abrir_exitoso() {
        let tool = ToolNavegadorReflejo;
        let args = json!({"operacion": "abrir", "url": "https://example.com"});
        let r = tool.ejecutar(&ctx_con_navegador(), args).await.unwrap();
        assert!(r.ok);
        assert!(r.contenido.contains("example.com"));
    }

    #[tokio::test]
    async fn t02_falla_sin_navegador() {
        let tool = ToolNavegadorReflejo;
        let args = json!({"operacion": "abrir", "url": "https://example.com"});
        let e = tool.ejecutar(&ctx_sin_navegador(), args).await;
        assert!(e.is_err());
        let msg = e.unwrap_err().to_string();
        assert!(msg.contains("no está disponible") || msg.contains("disponible"));
    }

    #[tokio::test]
    async fn t03_operacion_invalida() {
        let tool = ToolNavegadorReflejo;
        let args = json!({"operacion": "volar"});
        let e = tool.ejecutar(&ctx_con_navegador(), args).await;
        assert!(e.is_err());
    }

    #[tokio::test]
    async fn t04_capturar_con_navegador() {
        let tool = ToolNavegadorReflejo;
        let args = json!({"operacion": "capturar"});
        let r = tool.ejecutar(&ctx_con_navegador(), args).await.unwrap();
        assert!(r.ok);
        assert!(r.contenido.contains("stub_captura"));
    }

    #[tokio::test]
    async fn t05_cierra_y_reabre_stub() {
        let nav = StubNavegador { abierto: std::sync::Mutex::new(false) };
        nav.abrir("https://ej.com").await.unwrap();
        assert!(*nav.abierto.lock().unwrap());
        nav.cerrar().await.unwrap();
        assert!(!*nav.abierto.lock().unwrap());
    }

    #[tokio::test]
    async fn t06_id_y_schema() {
        let tool = ToolNavegadorReflejo;
        assert_eq!(tool.id(), "navegador_reflejo");
        let s = tool.schema();
        assert!(s.get("properties").and_then(|p| p.get("operacion")).is_some());
    }
}