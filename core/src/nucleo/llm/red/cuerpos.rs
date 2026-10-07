//! Cuerpos JSON de petición al proveedor (parte de `super`): chat/stream y
//! dialecto Responses, más conversores chat→Responses.
//!
//! Sin cambio de comportamiento: movimiento puro desde `red.rs` (partición
//! 309A-3; el archivo superaba el límite + god-object).

use super::super::*;

/// [059A-S3] Cuerpo JSON de una petición de streaming (modelo, mensajes,
/// tools, max_tokens por proveedor y reasoning_effort solo donde el proveedor
/// lo acepta). Movimiento fiel del cuerpo que antes vivía en
/// `ejecutar_request_stream`.
pub(super) fn construir_cuerpo_stream(
    proveedor: &str,
    modelo: &str,
    mensajes: &[AiMessage],
    opciones: &AiChatOptions,
    tools: &[serde_json::Value],
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": modelo,
        "messages": mensajes,
        "temperature": opciones.temperature,
        "stream": true,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools.to_vec());
    }
    if proveedor == "groq" {
        body["max_completion_tokens"] = serde_json::json!(opciones.max_tokens);
    } else {
        body["max_tokens"] = serde_json::json!(opciones.max_tokens);
    }
    /* [318A-10 02-09-2026] Mismo criterio que ejecutar_request: solo se
     * envía `reasoning_effort` a proveedores que lo aceptan.
     * [318A-11 02-09-2026] Incluye `glory` (gloryapi local lo acepta,
     * verificado 02-09).
     * [20-09-2026] Incluye `opencode-go` (acepta effort minimal/low/medium/
     * high/xhigh según models.dev). */
    if let Some(esfuerzo) = &opciones.reasoning_effort {
        if proveedor == "deepseek"
            || proveedor == "groq"
            || proveedor == "cerebras"
            || proveedor == "glory"
            || proveedor == "opencode-go"
        {
            body["reasoning_effort"] = serde_json::json!(esfuerzo);
        }
    }
    body
}

/// [20-09-2026] Cuerpo JSON del dialecto Responses API (`/v1/responses` de
/// OpenCode Go, modelos muse-spark): `input` con items tipados en vez de
/// `messages`, `reasoning: {effort, summary}` en vez de `reasoning_effort`
/// suelto, y `max_output_tokens` en vez de `max_tokens`. Sin `temperature`:
/// los modelos de razonamiento fijan la suya (enviar 0.2 da 400).
/// `stream` lo decide el llamador (streaming del agente vs batch).
pub(crate) fn construir_cuerpo_responses(
    modelo: &str,
    mensajes: &[AiMessage],
    opciones: &AiChatOptions,
    tools: &[serde_json::Value],
    stream: bool,
) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": modelo,
        "input": mensajes_a_items_responses(mensajes),
        "max_output_tokens": opciones.max_tokens,
        "stream": stream,
    });
    if !tools.is_empty() {
        body["tools"] = serde_json::Value::Array(tools.iter().map(convertir_tool_responses).collect());
    }
    /* `summary: "auto"` = pensamiento visible en vivo
     * (`response.reasoning_summary_text.delta`); sin él el reasoning viaja
     * cifrado (`encrypted_content`, opaco) y la UI no mostraría nada. */
    if let Some(esfuerzo) = &opciones.reasoning_effort {
        body["reasoning"] =
            serde_json::json!({ "effort": esfuerzo, "summary": "auto" });
    }
    body
}

/// Convierte el historial OpenAI (`messages`) a items `input` de Responses:
/// texto directo; assistant con tool_calls → items `function_call`; tool →
/// `function_call_output`. El contenido multimodal (array) se aplana a texto
/// + imágenes (`input_text`/`input_image`).
fn mensajes_a_items_responses(mensajes: &[AiMessage]) -> Vec<serde_json::Value> {
    let mut items = Vec::new();
    for mensaje in mensajes {
        let rol = mensaje.role.as_str();
        if rol == "tool" {
            items.push(serde_json::json!({
                "type": "function_call_output",
                "call_id": mensaje.tool_call_id.as_deref().unwrap_or(""),
                "output": contenido_a_texto(&mensaje.content),
            }));
            continue;
        }
        if rol == "assistant" {
            if let Some(llamadas) = mensaje.tool_calls.as_deref() {
                for llamada in llamadas {
                    items.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": llamada.id,
                        "name": llamada.nombre,
                        "arguments": llamada.argumentos.to_string(),
                    }));
                }
            }
            let texto = contenido_a_texto(&mensaje.content);
            if !texto.trim().is_empty() {
                items.push(serde_json::json!({
                    "type": "message", "role": "assistant", "content": texto,
                }));
            }
            continue;
        }
        /* system/developer/user/human: rol directo; `developer` no existe en
         * chat pero Responses lo acepta (system se deja tal cual). */
        let contenido = contenido_a_item_responses(&mensaje.content);
        items.push(serde_json::json!({
            "type": "message", "role": rol, "content": contenido,
        }));
    }
    items
}

/// Contenido de mensaje a texto plano (string directo o partes `text`).
fn contenido_a_texto(contenido: &serde_json::Value) -> String {
    match contenido {
        serde_json::Value::String(texto) => texto.clone(),
        serde_json::Value::Array(partes) => partes
            .iter()
            .filter_map(|parte| {
                parte
                    .get("text")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| {
                        parte
                            .get("input_text")
                            .and_then(|t| t.get("text"))
                            .and_then(serde_json::Value::as_str)
                    })
            })
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

/// Contenido a partes `input_*` de Responses (texto + imagen).
fn contenido_a_item_responses(contenido: &serde_json::Value) -> serde_json::Value {
    match contenido {
        serde_json::Value::String(_) => contenido.clone(),
        serde_json::Value::Array(partes) => serde_json::Value::Array(
            partes
                .iter()
                .filter_map(|parte| {
                    let tipo = parte.get("type").and_then(serde_json::Value::as_str)?;
                    match tipo {
                        "text" | "input_text" => parte.get("text").map(|texto| {
                            serde_json::json!({ "type": "input_text", "text": texto })
                        }),
                        "image_url" | "input_image" => {
                            let url = parte
                                .get("image_url")
                                .and_then(|u| u.get("url").and_then(serde_json::Value::as_str))
                                .or_else(|| {
                                    parte.get("image_url").and_then(serde_json::Value::as_str)
                                })?;
                            Some(serde_json::json!({ "type": "input_image", "image_url": url }))
                        }
                        _ => None,
                    }
                })
                .collect(),
        ),
        _ => serde_json::Value::String(String::new()),
    }
}

/// Tool chat `{"type":"function","function":{...}}` → Responses
/// `{"type":"function","name","description","parameters"}` (plana).
fn convertir_tool_responses(tool: &serde_json::Value) -> serde_json::Value {
    if let Some(funcion) = tool.get("function") {
        let mut plana = serde_json::json!({ "type": "function" });
        for campo in ["name", "description", "parameters", "strict"] {
            if let Some(valor) = funcion.get(campo) {
                plana[campo] = valor.clone();
            }
        }
        return plana;
    }
    tool.clone()
}

/* [309A-3] Cuerpo JSON de una petición batch `ejecutar_request` (extraído
 * sin cambio de comportamiento para bajar `ejecutar_request` del límite). */
pub(super) fn armar_body_request(
    proveedor: &str,
    modelo: &str,
    mensajes: &[AiMessage],
    opciones: &AiChatOptions,
    dialecto_responses: bool,
) -> serde_json::Value {
    /* Groq usa max_completion_tokens; el resto max_tokens (paridad PHP). */
    let mut body = if dialecto_responses {
        construir_cuerpo_responses(modelo, mensajes, opciones, &[], false)
    } else {
        serde_json::json!({
            "model": modelo,
            "messages": mensajes,
            "temperature": opciones.temperature,
        })
    };
    if proveedor == "groq" {
        body["max_completion_tokens"] = serde_json::json!(opciones.max_tokens);
    } else {
        body["max_tokens"] = serde_json::json!(opciones.max_tokens);
    }
    /* [318A-10 02-09-2026] Nivel de razonamiento elegido en el panel del
     * agente. Solo se envía si el usuario lo fijó y el proveedor lo acepta
     * (deepseek/groq/cerebras con modelos de razonamiento). No se envía a
     * proveedores tipo commandcode que pueden rechazar el campo.
     * [318A-11 02-09-2026] Se añade `glory`: gloryapi local SÍ acepta
     * `reasoning_effort` (verificado el 02-09: POST /v1/chat/completions
     * con reasoning_effort:"low" responde 200 con reasoning_content). */
    if let Some(esfuerzo) = &opciones.reasoning_effort {
        if proveedor == "deepseek"
            || proveedor == "groq"
            || proveedor == "cerebras"
            || proveedor == "glory"
        {
            body["reasoning_effort"] = serde_json::json!(esfuerzo);
        }
    }
    body
}