//! [079A-1 F4] Operaciones del navegador (partidas de herramientas/navegador.rs).

use crate::{
    error::{Error, Result},
    ports::NavegadorPort,
    tool::AgentToolResult,
};
use serde_json::Value;

/// [079A-1 F2] Extrae un argumento string (auxiliar de `ejecutar` para el
/// límite de 100 líneas efectivas del gate).
pub(crate) fn arg_str<'a>(argumentos: &'a Value, clave: &str, operacion: &str) -> Result<&'a str> {
    argumentos
        .get(clave)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Argumentos(format!("{clave} requerido para {operacion}")))
}

/// [079A-1 F2] Familia `js`/`cdp`: ejecuta script o comando CDP.
pub(crate) async fn op_script(
    navegador: &dyn NavegadorPort,
    operacion: &str,
    argumentos: &Value,
) -> Result<AgentToolResult> {
    if operacion == "js" {
        let codigo = arg_str(argumentos, "codigo", operacion)?;

        let resultado = navegador.js(codigo).await?;

        Ok(AgentToolResult::ok(resultado, "ejecutar JavaScript"))
    } else {
        let metodo = arg_str(argumentos, "metodo", operacion)?;

        let parametros = arg_str(argumentos, "parametros", operacion)?;

        let resultado = navegador.cdp(metodo, parametros).await?;

        Ok(AgentToolResult::ok(resultado, "CDP"))
    }
}

/// [079A-1 F2] Familia DOM (`click`/`rellenar`/`snapshot`).
pub(crate) async fn op_dom(
    navegador: &dyn NavegadorPort,
    operacion: &str,
    argumentos: &Value,
) -> Result<AgentToolResult> {
    let selector = arg_str(argumentos, "selector", operacion)?;

    match operacion {
        "click" => {
            navegador.click(selector).await?;

            Ok(AgentToolResult::ok(format!("Click en {selector}"), "click"))
        }

        "rellenar" => {
            let valor = arg_str(argumentos, "valor", operacion)?;

            navegador.rellenar(selector, valor).await?;

            Ok(AgentToolResult::ok(
                format!("Campo {selector} rellenado"),
                "rellenar formulario",
            ))
        }

        _ => {
            let resultado = navegador.snapshot(selector).await?;

            Ok(AgentToolResult::ok(resultado, "snapshot DOM"))
        }
    }
}

/// [079A-1 F2] `capturar`: captura + evento ToolNavegador con la imagen
/// base64 para que el front la muestre en el panel.
pub(crate) async fn op_capturar(navegador: &dyn NavegadorPort) -> Result<AgentToolResult> {
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
}
