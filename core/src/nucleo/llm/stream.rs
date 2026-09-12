//! [079A-1 F3] Hojear SSE del proveedor (partido de red.rs).

use super::*;

/// [129A-2] Destinos en vivo de un stream: texto (`on_token`, con poder de
/// cancelación) y pensamiento (`on_razonamiento`) viajan en un solo valor
/// para que las firmas no crezcan con cada callback (clippy
/// `too_many_arguments`: 7 es el techo). Es `pub` porque viaja en la firma
/// pública de `enviar_chat_stream` (solo la construye el runtime).
pub struct SalidasVivo<'a> {
    pub token: &'a mut (dyn FnMut(&str) -> bool + Send),
    pub razonamiento: &'a mut (dyn FnMut(&str) + Send),
}

/* El bucle SSE aplanado en un helper: solo consume el stream, acumula
 * content/token usage/tool_calls/finish_reason y gestiona la cancelación
 * (on_token -> false). Devuelve la tupla cruda que ejecutar_request_stream
 * envuelve en AiStreamResult. `respuesta` se consume por valor (bytes_stream).
 * [129A-2] `on_razonamiento` recibe cada `delta.reasoning_content` EN VIVO
 * (sin valor de retorno: el pensamiento no cancela el stream); el acumulado
 * completo sigue volviendo en la tupla para el evento único de cierre. */
pub(crate) async fn hojear_stream(
    respuesta: reqwest::Response,
    salidas: SalidasVivo<'_>,
) -> Result<(String, String, Vec<serde_json::Value>, u32, u32, String, String), Error> {
    /* Los callbacks viajan juntos: se desestructuran para llamar sin
     * pelear con el borrow de `salidas`. */
    let SalidasVivo {
        token: on_token,
        razonamiento: on_razonamiento,
    } = salidas;
    let mut contenido = String::new();
    /* [129A-1] El pensamiento viaja en `delta.reasoning_content`: se acumula
     * aparte (nunca por `on_token`, que es solo texto de respuesta) y se
     * devuelve para emitirlo como evento único al completar. */
    let mut razonamiento = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut tokens_prompt = 0u32;
    let mut tokens_complecion = 0u32;
    let mut finish_reason = String::new();
    let mut modelo_real = String::new();

    let mut bytes = respuesta.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|error| Error::Proveedor {
            detalle: format!("Error leyendo el stream del proveedor: {error}"),
            causa: None,
        })?;
        let texto = String::from_utf8_lossy(&chunk);
        for linea in texto.lines() {
            let Some(evento) = extraer_evento_sse(linea) else {
                continue;
            };
            /* [069A-7 06-09-2026] Capturar `model` del SSE (glory API reporta
             * el modelo real que eligi├│ su router auto). Se toma del primer
             * chunk que lo incluya. */
            if modelo_real.is_empty() {
                if let Some(m) = evento.get("model").and_then(serde_json::Value::as_str) {
                    if !m.is_empty() {
                        modelo_real = m.to_string();
                    }
                }
            }
            if let Some(usage) = evento.get("usage") {
                tokens_prompt = usage
                    .get("prompt_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
                tokens_complecion = usage
                    .get("completion_tokens")
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(0) as u32;
            }
            if let Some(delta) = evento.pointer("/choices/0/delta") {
                if let Some(texto_delta) = delta.get("content").and_then(serde_json::Value::as_str)
                {
                    contenido.push_str(texto_delta);
                    /* Fase 4: cancelaci├│n real ÔÇö si el cliente cort├│ el SSE,
                     * dejar de consumir el stream del proveedor de inmediato. */
                    if !on_token(texto_delta) {
                        return Err(Error::Cancelado);
                    }
                }
                if let Some(pensado) = delta
                    .get("reasoning_content")
                    .and_then(serde_json::Value::as_str)
                {
                    /* [129A-2] En vivo para el summary abierto del front; el
                     * acumulado completo sigue saliendo en la tupla. */
                    razonamiento.push_str(pensado);
                    on_razonamiento(pensado);
                }
                if let Some(calls) = delta
                    .get("tool_calls")
                    .and_then(serde_json::Value::as_array)
                {
                    for call in calls {
                        fusionar_tool_call(&mut tool_calls, call);
                    }
                }
            }
            if let Some(fr) = evento
                .pointer("/choices/0/finish_reason")
                .and_then(serde_json::Value::as_str)
            {
                if !fr.is_empty() && fr != "null" {
                    finish_reason = fr.to_string();
                }
            }
        }
    }

    Ok((
        contenido,
        razonamiento,
        tool_calls,
        tokens_prompt,
        tokens_complecion,
        finish_reason,
        modelo_real,
    ))
}

/// [079A-1 F2] Parsea una l├¡nea SSE a evento (`None` si no es `data:` ├║til).
fn extraer_evento_sse(linea: &str) -> Option<serde_json::Value> {
    let linea = linea.trim();
    if !linea.starts_with("data:") {
        return None;
    }
    let data = linea.trim_start_matches("data:").trim();
    if data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(data).ok()
}

/// [079A-1 F2] Fusiona un fragmento `tool_calls` del delta en el acumulado
/// (los argumentos llegan troceados y se concatenan por ├¡ndice).
fn fusionar_tool_call(tool_calls: &mut Vec<serde_json::Value>, call: &serde_json::Value) {
    let index = call
        .get("index")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as usize;
    if tool_calls.len() <= index {
        tool_calls.resize(
            index + 1,
            serde_json::json!({ "function": { "name": "", "arguments": "" } }),
        );
    }
    if let Some(nombre) = call
        .pointer("/function/name")
        .and_then(serde_json::Value::as_str)
    {
        tool_calls[index]["function"]["name"] = serde_json::Value::String(nombre.to_string());
    }
    if let Some(args) = call
        .pointer("/function/arguments")
        .and_then(serde_json::Value::as_str)
    {
        let actual = tool_calls[index]["function"]["arguments"]
            .as_str()
            .unwrap_or("")
            .to_string();
        tool_calls[index]["function"]["arguments"] =
            serde_json::Value::String(format!("{actual}{args}"));
    }
    if let Some(id) = call.get("id").and_then(serde_json::Value::as_str) {
        tool_calls[index]["id"] = serde_json::Value::String(id.to_string());
    }
}
