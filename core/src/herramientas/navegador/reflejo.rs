//! [079A-1 F4] Tool navegador_reflejo (partida de herramientas/navegador.rs).

use crate::{
    error::{Error, Result},
    ports::NavegadorPort,
    tool::{AgentTool, AgentToolContext, AgentToolResult},
};
use async_trait::async_trait;
use serde_json::{json, Value};

use super::operaciones::{arg_str, op_capturar, op_dom, op_script};

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
            "abrir" | "navegar" => {
                let url = arg_str(&argumentos, "url", operacion)?;

                if operacion == "abrir" {
                    navegador.abrir(url).await?;

                    Ok(AgentToolResult::ok(
                        format!("Navegador abierto en {url}"),
                        "abrir navegador",
                    ))
                } else {
                    navegador.navegar(url).await?;

                    Ok(AgentToolResult::ok(format!("Navegado a {url}"), "navegar"))
                }
            }

            "capturar" => op_capturar(navegador).await,

            "js" | "cdp" => op_script(navegador, operacion, &argumentos).await,

            "click" | "rellenar" | "snapshot" => op_dom(navegador, operacion, &argumentos).await,

            "cerrar" => {
                navegador.cerrar().await?;

                Ok(AgentToolResult::ok("Navegador cerrado", "cerrar navegador"))
            }

            _ => Err(Error::Argumentos(format!(
                "operación '{operacion}' no soportada. Válidas: abrir, navegar, capturar, \

                 js, cdp, click, rellenar, snapshot, cerrar"
            ))),
        }
    }
}
