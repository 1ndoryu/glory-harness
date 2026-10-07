//! Envío HTTP y parseo de respuestas del proveedor (parte de `super`):
//! POST JSON con validación de status, fallback no-stream y aplanado del
//! dialecto Responses a los tipos comunes.
//!
//! Sin cambio de comportamiento: movimiento puro desde `red.rs` (partición
//! 309A-3; el archivo superaba el límite + god-object).

use super::super::modelo::{es_dialecto_responses, parsear_tool_calls};
use super::super::*;

/// ¿La respuesta del proveedor es un stream SSE real? Guía el fallback
/// no-stream de `ejecutar_request_stream`.
pub(super) fn respuesta_es_stream(respuesta: &reqwest::Response) -> bool {
    respuesta
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .contains("text/event-stream")
}

/// Envía la petición JSON y valida el status HTTP. Devuelve la respuesta solo
/// si fue exitosa; los errores del proveedor (4xx/5xx) se propagan con su
/// mensaje como `Error::Proveedor`.
/// [20-09-2026] Dialecto Responses (muse-spark): exige `x-opencode-session`
/// estable por conversación (sin él, 400 `MissingSessionID`) y User-Agent
/// propio (el gateway lo pide para clasificar el tráfico).
pub(super) async fn enviar_solicitud(
    cliente: &reqwest::Client,
    proveedor: &str,
    modelo: &str,
    api_key: &str,
    url: &str,
    body: &serde_json::Value,
    sesion: Option<&str>,
) -> Result<reqwest::Response, Error> {
    let mut request = cliente.post(url);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    if es_dialecto_responses(proveedor, modelo) {
        let sesion_id = match sesion {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        request = request
            .header("x-opencode-session", sesion_id)
            .header("User-Agent", "glory-harness/1.0");
    }
    let respuesta = request
        .json(body)
        .send()
        .await
        .map_err(|error| Error::Proveedor {
            detalle: format!("Error de red: {error}"),
            causa: None,
        })?;
    let status = respuesta.status();
    if !status.is_success() {
        let datos: serde_json::Value = respuesta.json().await.unwrap_or_default();
        let mensaje = datos
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Error del proveedor");
        return Err(Error::Proveedor {
            detalle: format!("{proveedor} {status}: {mensaje}"),
            causa: None,
        });
    }
    Ok(respuesta)
}

/// [059A-S3] Fallback no-stream: convierte una respuesta JSON directa en un
/// único `on_token` y construye el `AiStreamResult` (usage incluido). Si el
/// cliente canceló (`on_token` → false) se aborta con `Error::Cancelado` sin
/// devolver una respuesta parcial como éxito.
/// [069A-9 07-09-2026] Captura `X-Routed-Via` header (gloryapi) como
/// provider/modelo exacto, con prioridad sobre el campo `model` del body.
/// [129A-2] Vía no-stream = respuesta batch: NO llama a `on_razonamiento`
/// (no hay nada "en vivo" que emitir; el pensamiento viaja en el
/// `AiStreamResult` para el evento único de cierre, secuencia 129A-1 intacta).
pub(super) async fn resultado_no_stream(
    respuesta: reqwest::Response,
    proveedor: &str,
    modelo: &str,
    on_token: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<AiStreamResult, Error> {
    /* [069A-9 07-09-2026] gloryapi expone el proveedor exacto que eligió
     * su router auto en el header X-Routed-Via. Se extrae ANTES de consumir
     * el body (respuesta.json() toma ownership de `respuesta`). */
    let routed_via: Option<(String, String)> = respuesta
        .headers()
        .get("X-Routed-Via")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split_once('/'))
        .map(|(p, m)| (p.to_string(), m.to_string()));

    let datos: serde_json::Value = respuesta.json().await.map_err(|error| Error::Proveedor {
        detalle: format!("Respuesta no JSON del proveedor: {error}"),
        causa: None,
    })?;

    /* [20-09-2026] Dialecto Responses: la respuesta no-stream es el objeto
     * `response` completo (`output` con items, `usage` con input/output).
     * Se aplana al mismo AiStreamResult: texto + summaries + function_calls. */
    if es_dialecto_responses(proveedor, modelo) {
        return resultado_responses_no_stream(datos, proveedor, modelo, on_token);
    }

    let contenido = datos
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    /* [129A-1] Mismo punto de pérdida en no-stream: el pensamiento viaja en
     * `message.reasoning_content` y se ignoraba.
     * [20-09-2026] Fallback `message.reasoning`: mismo criterio que el stream. */
    let razonamiento = datos
        .pointer("/choices/0/message/reasoning_content")
        .and_then(serde_json::Value::as_str)
        .or_else(|| {
            datos
                .pointer("/choices/0/message/reasoning")
                .and_then(serde_json::Value::as_str)
        })
        .unwrap_or("")
        .to_string();
    if !on_token(&contenido) {
        return Err(Error::Cancelado);
    }
    /* [069A-7 06-09-2026] Capturar `model` real de la respuesta no-stream. */
    let modelo_real = datos
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(modelo);
    let (provider_final, modelo_final) = match routed_via {
        Some((p, m)) => (p, m),
        None => (proveedor.to_string(), modelo_real.to_string()),
    };
    Ok(AiStreamResult {
        contenido,
        razonamiento,
        tool_calls: Vec::new(),
        tokens_prompt: datos
            .pointer("/usage/prompt_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        tokens_complecion: datos
            .pointer("/usage/completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        finish_reason: datos
            .pointer("/choices/0/finish_reason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        provider: provider_final,
        modelo: modelo_final,
    })
}

/// [20-09-2026] Aplana un objeto `response` no-stream de Responses API al
/// `AiStreamResult` común: concatena `output_text` (+ `refusal`), summaries
/// de reasoning y `function_call` en formato chat para `parsear_tool_calls`.
/// `usage {input_tokens, output_tokens}` y `model` directos del objeto.
fn resultado_responses_no_stream(
    datos: serde_json::Value,
    proveedor: &str,
    modelo: &str,
    on_token: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<AiStreamResult, Error> {
    let mut contenido = String::new();
    let mut razonamiento = String::new();
    let mut tool_calls = Vec::new();
    if let Some(items) = datos.get("output").and_then(serde_json::Value::as_array) {
        for item in items {
            match item.get("type").and_then(serde_json::Value::as_str) {
                Some("message") => {
                    if let Some(partes) =
                        item.get("content").and_then(serde_json::Value::as_array)
                    {
                        for parte in partes {
                            let tipo = parte
                                .get("type")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("");
                            if tipo == "output_text" {
                                if let Some(texto) = parte
                                    .get("text")
                                    .and_then(serde_json::Value::as_str)
                                {
                                    contenido.push_str(texto);
                                }
                            } else if tipo == "refusal" {
                                if let Some(texto) = parte
                                    .get("refusal")
                                    .and_then(serde_json::Value::as_str)
                                {
                                    contenido.push_str(texto);
                                }
                            }
                        }
                    }
                }
                Some("reasoning") => {
                    if let Some(summaries) =
                        item.get("summary").and_then(serde_json::Value::as_array)
                    {
                        for resumen in summaries {
                            if let Some(texto) = resumen
                                .get("text")
                                .and_then(serde_json::Value::as_str)
                            {
                                razonamiento.push_str(texto);
                            }
                        }
                    }
                }
                Some("function_call") => {
                    tool_calls.push(serde_json::json!({
                        "id": item.get("call_id"),
                        "type": "function",
                        "function": {
                            "name": item.get("name"),
                            "arguments": item.get("arguments"),
                        },
                    }));
                }
                _ => {}
            }
        }
    }
    let estado = datos
        .get("status")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("completed");
    if estado == "failed" {
        return Err(Error::Proveedor {
            detalle: format!("{proveedor} response fallida"),
            causa: None,
        });
    }
    if !on_token(&contenido) {
        return Err(Error::Cancelado);
    }
    let modelo_real = datos
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or(modelo);
    Ok(AiStreamResult {
        contenido,
        razonamiento,
        tool_calls: parsear_tool_calls(tool_calls),
        tokens_prompt: datos
            .pointer("/usage/input_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        tokens_complecion: datos
            .pointer("/usage/output_tokens")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0) as u32,
        finish_reason: match estado {
            "completed" => "stop".to_string(),
            "incomplete" => "length".to_string(),
            other => other.to_string(),
        },
        provider: proveedor.to_string(),
        modelo: modelo_real.to_string(),
    })
}

/* [309A-3] POST JSON batch con validación de status (extraído de
 * `ejecutar_request` sin cambio de comportamiento). */
pub(super) async fn post_json(
    cliente: &reqwest::Client,
    proveedor: &str,
    url: &str,
    api_key: &str,
    dialecto_responses: bool,
    sesion_externa: Option<&str>,
    body: &serde_json::Value,
) -> Result<serde_json::Value, Error> {
    /* [27-08-2026] Glory API (free.empero.org) responde sin API key y
     * REJECTA un header Authorization vacío (400). Con key presente se
     * envía el header; sin key no se envía Authorization en absoluto.
     * [02-09-2026] Con gloryapi local la key SÍ es necesaria (unified key
     * de gloryapi); el header se envía si la env trae clave. */
    let mut request = cliente.post(url);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
    }
    /* [20-09-2026] Dialecto Responses: `x-opencode-session` obligatorio;
     * la sesión estable la pone el llamador vía `sesion_externa`. */
    if dialecto_responses {
        let sesion_id = match sesion_externa {
            Some(s) if !s.trim().is_empty() => s.trim().to_string(),
            _ => uuid::Uuid::new_v4().to_string(),
        };
        request = request
            .header("x-opencode-session", sesion_id)
            .header("User-Agent", "glory-harness/1.0");
    }
    let respuesta = request
        .json(body)
        .send()
        .await
        .map_err(|error| Error::Proveedor {
            detalle: format!("Error de red: {error}"),
            causa: None,
        })?;

    let status = respuesta.status();
    let datos: serde_json::Value =
        respuesta.json().await.map_err(|error| Error::Proveedor {
            detalle: format!("Respuesta no JSON del proveedor: {error}"),
            causa: None,
        })?;

    if !status.is_success() {
        let mensaje = datos
            .get("error")
            .and_then(|error| error.get("message"))
            .and_then(serde_json::Value::as_str)
            .or_else(|| datos.get("message").and_then(serde_json::Value::as_str))
            .unwrap_or("Error del proveedor");
        tracing::warn!(%proveedor, %status, %mensaje, detalle = %datos, "proveedor LLM rechazó el request");
        return Err(Error::Proveedor {
            detalle: format!("{proveedor} {status}: {mensaje}"),
            causa: None,
        });
    }
    /* [20-09-2026] Dialecto Responses: el objeto `response` se aplana
     * (texto + summaries); el estado no-`completed` cuenta como vacío. */
    Ok(datos)
}

/* [309A-3] Texto de la respuesta batch (extraído de `ejecutar_request`). */
pub(super) fn extraer_contenido(
    datos: &serde_json::Value,
    dialecto_responses: bool,
) -> Result<String, Error> {
    if dialecto_responses {
        let mut texto = String::new();
        if let Some(items) = datos.get("output").and_then(serde_json::Value::as_array)
        {
            for item in items {
                if item.get("type").and_then(serde_json::Value::as_str)
                    != Some("message")
                {
                    continue;
                }
                if let Some(partes) =
                    item.get("content").and_then(serde_json::Value::as_array)
                {
                    for parte in partes {
                        let tipo = parte
                            .get("type")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or("");
                        let fragmento = if tipo == "output_text" {
                            parte.get("text")
                        } else if tipo == "refusal" {
                            parte.get("refusal")
                        } else {
                            None
                        };
                        if let Some(fragmento) =
                            fragmento.and_then(serde_json::Value::as_str)
                        {
                            texto.push_str(fragmento);
                        }
                    }
                }
            }
        }
        let texto = texto.trim();
        if texto.is_empty() {
            return Err(Error::Proveedor {
                detalle: "Respuesta vacía del modelo".into(),
                causa: None,
            });
        }
        Ok(texto.to_string())
    } else {
        Ok(datos
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|contenido| !contenido.is_empty())
            .ok_or_else(|| Error::Proveedor {
                detalle: "Respuesta vacía del modelo".into(),
                causa: None,
            })?
            .to_string())
    }
}

/* [309A-3] Resultado batch común con usage por dialecto (extraído de
 * `ejecutar_request`). */
pub(super) fn armar_resultado_chat(
    proveedor: &str,
    modelo: &str,
    dialecto_responses: bool,
    contenido: String,
    datos: &serde_json::Value,
) -> AiChatResult {
    AiChatResult {
        contenido,
        tokens_prompt: if dialecto_responses {
            datos
                .pointer("/usage/input_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
        } else {
            datos
                .pointer("/usage/prompt_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
        },
        tokens_complecion: if dialecto_responses {
            datos
                .pointer("/usage/output_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
        } else {
            datos
                .pointer("/usage/completion_tokens")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0) as u32
        },
        finish_reason: if dialecto_responses {
            match datos.get("status").and_then(serde_json::Value::as_str) {
                Some("completed") => "stop".to_string(),
                Some("incomplete") => "length".to_string(),
                Some(otro) => otro.to_string(),
                None => String::new(),
            }
        } else {
            datos
                .pointer("/choices/0/finish_reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string()
        },
        provider: proveedor.to_string(),
        modelo: modelo.to_string(),
    }
}