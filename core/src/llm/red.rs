//! [059A-N S2] Split mecánico de `llm.rs`: red/HTTP: reintentos con backoff, streaming, hojear_stream y parseo de tool_calls. Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::*;
use super::modelo::{es_error_transitorio, parsear_tool_calls};

const REINTENTOS_TRANSITORIOS: u32 = 2;
const BACKOFF_BASE_MS: u64 = 500;


/// Espera de backoff exponencial entre reintentos (0.5s, 1.5s). Devuelve
/// inmediatamente si el intento es el primero (sin espera previa).
async fn esperar_backoff(intento: u32) {
    if intento == 0 {
        return;
    }
    let ms = BACKOFF_BASE_MS * 2u64.pow(intento);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

/// [059A-S3] Cuerpo JSON de una petición de streaming (modelo, mensajes,
/// tools, max_tokens por proveedor y reasoning_effort solo donde el proveedor
/// lo acepta). Movimiento fiel del cuerpo que antes vivía en
/// `ejecutar_request_stream`.
fn construir_cuerpo_stream(
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
     * verificado 02-09). */
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

/// ¿La respuesta del proveedor es un stream SSE real? Guía el fallback
/// no-stream de `ejecutar_request_stream`.
fn respuesta_es_stream(respuesta: &reqwest::Response) -> bool {
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
async fn enviar_solicitud(
    cliente: &reqwest::Client,
    proveedor: &str,
    api_key: &str,
    url: &str,
    body: &serde_json::Value,
) -> Result<reqwest::Response, Error> {
    let mut request = cliente.post(url);
    if !api_key.is_empty() {
        request = request.bearer_auth(api_key);
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
async fn resultado_no_stream(
    respuesta: reqwest::Response,
    proveedor: &str,
    modelo: &str,
    on_token: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<AiStreamResult, Error> {
    let datos: serde_json::Value = respuesta.json().await.map_err(|error| {
        Error::Proveedor {
            detalle: format!("Respuesta no JSON del proveedor: {error}"),
            causa: None,
        }
    })?;
    let contenido = datos
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    if !on_token(&contenido) {
        return Err(Error::Cancelado);
    }
    Ok(AiStreamResult {
        contenido,
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
        provider: proveedor.to_string(),
        modelo: modelo.to_string(),
    })
}

impl LlmProviderService {
    /// [29-08-2026] Streaming SSE hacia el proveedor (agente, plan Fase 0/1).
    /// Emite cada token vía `on_token` (para el contrato SSE del agente) y
    /// acumula tool_calls en streaming. `tools` es la lista de schemas OpenAI
    /// (vacía = llamada sin tools). Fallback: si el proveedor no soporta
    /// streaming (o falla al abrir el stream), se hace una llamada no-stream y
    /// se emite un único `on_token` con la respuesta completa.
    pub async fn enviar_chat_stream(
        &self,
        mensajes: Vec<AiMessage>,
        provider: &str,
        modelo: &str,
        opciones: AiChatOptions,
        tools: Vec<serde_json::Value>,
        on_token: &mut (dyn FnMut(&str) -> bool + Send),
    ) -> Result<AiStreamResult, Error> {
        let mensajes_validos = validar_mensajes(mensajes)?;
        let mut errores: Vec<String> = Vec::new();

        for (proveedor, modelo) in resolver_candidatos(provider, modelo) {
            /* [29-08-2026] Circuit breaker (R7): mismo criterio que enviar_chat. */
            if self.proveedor_abierto(proveedor) {
                errores.push(format!(
                    "{proveedor}/{modelo}: proveedor en cooldown por fallos consecutivos"
                ));
                continue;
            }
            let keys = self.keys_para(proveedor);
            if keys.is_empty() && proveedor != "glory" {
                errores.push(format!(
                    "No hay API key configurada para {proveedor} en el entorno"
                ));
                continue;
            }
            let keys: Vec<String> = if keys.is_empty() {
                vec![String::new()]
            } else {
                keys.to_vec()
            };
            for key in &keys {
                let solicitud = SolicitudStream {
                    proveedor,
                    api_key: key,
                    modelo,
                    mensajes: &mensajes_validos,
                    opciones: &opciones,
                    tools: &tools,
                };
                match self
                    .ejecutar_request_stream_con_reintentos(solicitud, on_token)
                    .await
                {
                    Ok(resultado) => {
                        self.registrar_acierto(proveedor);
                        return Ok(resultado);
                    }
                    /* Cancelación del cliente: no es fallo del proveedor y no
                     * hay que probar el siguiente — abortar el stream. */
                    Err(Error::Cancelado) => return Err(Error::Cancelado),
                    Err(error) => {
                        if es_error_transitorio(&error) {
                            self.registrar_fallo(proveedor);
                        } else {
                            self.registrar_fallo_permanente(proveedor);
                        }
                        tracing::warn!(%error, proveedor, modelo, "stream del proveedor falló");
                        errores.push(format!("{proveedor}/{modelo}: {error}"));
                    }
                }
            }
        }

        let mut unicos: Vec<String> = Vec::new();
        for e in &errores {
            if !unicos.contains(e) {
                unicos.push(e.clone());
            }
        }
        let detalle = if unicos.is_empty() {
            "sin errores de proveedor".to_string()
        } else {
            unicos.join(" | ")
        };
        Err(Error::Proveedor {
            detalle: format!("No se pudo contactar un modelo IA disponible: {detalle}"),
            causa: None,
        })
    }

    /// Request con stream=true; parsea líneas SSE `data: {...}` acumulando
    /// content y tool_calls. Fallback interno a no-stream si el proveedor
    /// responde sin SSE (algunos proxies devuelven JSON directo).
    async fn ejecutar_request_stream(
        &self,
        solicitud: SolicitudStream<'_>,
        on_token: &mut (dyn FnMut(&str) -> bool + Send),
    ) -> Result<AiStreamResult, Error> {
        let SolicitudStream {
            proveedor,
            api_key,
            modelo,
            mensajes,
            opciones,
            tools,
        } = solicitud;
        let url = url_proveedor(proveedor);
        let modelo = modelo_proveedor(proveedor, modelo);
        let body = construir_cuerpo_stream(proveedor, &modelo, mensajes, opciones, tools);

        let respuesta = enviar_solicitud(&self.client, proveedor, api_key, &url, &body).await?;

        /* Fallback no-stream: si el proveedor no devuelve text/event-stream
         * (p. ej. un proxy que responde JSON directo), se hace la llamada
         * normal y se emite un único token. */
        if !respuesta_es_stream(&respuesta) {
            return resultado_no_stream(respuesta, proveedor, &modelo, on_token).await;
        }

        let (contenido, tool_calls, tokens_prompt, tokens_complecion, finish_reason) =
            hojear_stream(respuesta, on_token).await?;
        let tool_calls = parsear_tool_calls(tool_calls);

        Ok(AiStreamResult {
            contenido,
            tool_calls,
            tokens_prompt,
            tokens_complecion,
            finish_reason,
            provider: proveedor.to_string(),
            modelo: modelo.to_string(),
        })
    }

    /* [318A-10 02-09-2026] Envoltorio con reintentos: solo reintenta errores
     * transitorios (503/429/5xx/timeout/red) con backoff exponencial. Los
     * errores permanentes (4xx de auth/billing/schema) fallan al primer
     * intento para no añadir latencia inútil. */
    pub(crate) async fn ejecutar_request_con_reintentos(
        &self,
        proveedor: &str,
        api_key: &str,
        modelo: &str,
        mensajes: &[AiMessage],
        opciones: &AiChatOptions,
    ) -> Result<AiChatResult, Error> {
        let mut ultimo_error: Option<Error> = None;
        for intento in 0..=REINTENTOS_TRANSITORIOS {
            match self
                .ejecutar_request(proveedor, api_key, modelo, mensajes, opciones)
                .await
            {
                Ok(resultado) => return Ok(resultado),
                Err(error) if es_error_transitorio(&error) && intento < REINTENTOS_TRANSITORIOS => {
                    tracing::warn!(
                        %error,
                        proveedor,
                        modelo,
                        intento,
                        "error transitorio del proveedor, reintentando"
                    );
                    ultimo_error = Some(error);
                    esperar_backoff(intento).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(ultimo_error.unwrap_or_else(|| Error::Proveedor {
            detalle: "Error transitorio sin detalle".into(),
            causa: None,
        }))
    }

    /// Igual que `ejecutar_request_con_reintentos` pero para streaming (agente).
    /// La cancelación del cliente (Error::Cancelado) se propaga sin reintento.
    async fn ejecutar_request_stream_con_reintentos(
        &self,
        solicitud: SolicitudStream<'_>,
        on_token: &mut (dyn FnMut(&str) -> bool + Send),
    ) -> Result<AiStreamResult, Error> {
        let SolicitudStream {
            proveedor,
            modelo,
            ..
        } = solicitud;
        let mut ultimo_error: Option<Error> = None;
        for intento in 0..=REINTENTOS_TRANSITORIOS {
            match self
                .ejecutar_request_stream(solicitud, on_token)
                .await
            {
                Ok(resultado) => return Ok(resultado),
                Err(Error::Cancelado) => return Err(Error::Cancelado),
                Err(error) if es_error_transitorio(&error) && intento < REINTENTOS_TRANSITORIOS => {
                    tracing::warn!(
                        %error,
                        proveedor,
                        modelo,
                        intento,
                        "error transitorio del proveedor, reintentando"
                    );
                    ultimo_error = Some(error);
                    esperar_backoff(intento).await;
                }
                Err(error) => return Err(error),
            }
        }
        Err(ultimo_error.unwrap_or_else(|| Error::Proveedor {
            detalle: "Error transitorio sin detalle".into(),
            causa: None,
        }))
    }

}

impl LlmProviderService {
    async fn ejecutar_request(
        &self,
        proveedor: &str,
        api_key: &str,
        modelo: &str,
        mensajes: &[AiMessage],
        opciones: &AiChatOptions,
    ) -> Result<AiChatResult, Error> {
        let url = url_proveedor(proveedor);
        let modelo = modelo_proveedor(proveedor, modelo);

        /* Groq usa max_completion_tokens; el resto max_tokens (paridad PHP). */
        let mut body = serde_json::json!({
            "model": modelo,
            "messages": mensajes,
            "temperature": opciones.temperature,
        });
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

        /* [27-08-2026] Glory API (free.empero.org) responde sin API key y
         * REJECTA un header Authorization vacío (400). Con key presente se
         * envía el header; sin key no se envía Authorization en absoluto.
         * [02-09-2026] Con gloryapi local la key SÍ es necesaria (unified key
         * de gloryapi); el header se envía si la env trae clave. */
        let mut request = self.client.post(url);
        if !api_key.is_empty() {
            request = request.bearer_auth(api_key);
        }
        let respuesta = request
            .json(&body)
            .send()
            .await
            .map_err(|error| Error::Proveedor {
                detalle: format!("Error de red: {error}"),
                causa: None,
            })?;

        let status = respuesta.status();
        let datos: serde_json::Value = respuesta.json().await.map_err(|error| {
            Error::Proveedor {
                detalle: format!("Respuesta no JSON del proveedor: {error}"),
                causa: None,
            }
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

        let contenido = datos
            .pointer("/choices/0/message/content")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|contenido| !contenido.is_empty())
            .ok_or_else(|| Error::Proveedor {
                detalle: "Respuesta vacía del modelo".into(),
                causa: None,
            })?
            .to_string();

        Ok(AiChatResult {
            contenido,
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
            provider: proveedor.to_string(),
            modelo: modelo.to_string(),
        })
    }
}

/* El bucle SSE aplanado en un helper: solo consume el stream, acumula
 * content/token usage/tool_calls/finish_reason y gestiona la cancelación
 * (on_token -> false). Devuelve la tupla cruda que ejecutar_request_stream
 * envuelve en AiStreamResult. `respuesta` se consume por valor (bytes_stream). */
async fn hojear_stream(
    respuesta: reqwest::Response,
    on_token: &mut (dyn FnMut(&str) -> bool + Send),
) -> Result<(String, Vec<serde_json::Value>, u32, u32, String), Error> {
    let mut contenido = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut tokens_prompt = 0u32;
    let mut tokens_complecion = 0u32;
    let mut finish_reason = String::new();

    let mut bytes = respuesta.bytes_stream();
    use futures_util::StreamExt;
    while let Some(chunk) = bytes.next().await {
        let chunk = chunk.map_err(|error| Error::Proveedor {
            detalle: format!("Error leyendo el stream del proveedor: {error}"),
            causa: None,
        })?;
        let texto = String::from_utf8_lossy(&chunk);
        for linea in texto.lines() {
            let linea = linea.trim();
            if !linea.starts_with("data:") {
                continue;
            }
            let data = linea.trim_start_matches("data:").trim();
            if data == "[DONE]" {
                continue;
            }
            let Ok(evento) = serde_json::from_str::<serde_json::Value>(data) else {
                continue;
            };
            if let Some(usage) = evento.get("usage") {
                tokens_prompt = usage.get("prompt_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
                tokens_complecion = usage.get("completion_tokens").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
            }
            if let Some(delta) = evento.pointer("/choices/0/delta") {
                if let Some(texto_delta) = delta.get("content").and_then(serde_json::Value::as_str) {
                    contenido.push_str(texto_delta);
                    /* Fase 4: cancelación real — si el cliente cortó el SSE,
                     * dejar de consumir el stream del proveedor de inmediato. */
                    if !on_token(texto_delta) {
                        return Err(Error::Cancelado);
                    }
                }
                if let Some(calls) = delta.get("tool_calls").and_then(serde_json::Value::as_array) {
                    for call in calls {
                        let index = call.get("index").and_then(serde_json::Value::as_u64).unwrap_or(0) as usize;
                        if tool_calls.len() <= index {
                            tool_calls.resize(index + 1, serde_json::json!({ "function": { "name": "", "arguments": "" } }));
                        }
                        if let Some(nombre) = call.pointer("/function/name").and_then(serde_json::Value::as_str) {
                            tool_calls[index]["function"]["name"] = serde_json::Value::String(nombre.to_string());
                        }
                        if let Some(args) = call.pointer("/function/arguments").and_then(serde_json::Value::as_str) {
                            let actual = tool_calls[index]["function"]["arguments"].as_str().unwrap_or("").to_string();
                            tool_calls[index]["function"]["arguments"] =
                                serde_json::Value::String(format!("{actual}{args}"));
                        }
                        if let Some(id) = call.get("id").and_then(serde_json::Value::as_str) {
                            tool_calls[index]["id"] = serde_json::Value::String(id.to_string());
                        }
                    }
                }
            }
            if let Some(fr) = evento.pointer("/choices/0/finish_reason").and_then(serde_json::Value::as_str) {
                if !fr.is_empty() && fr != "null" {
                    finish_reason = fr.to_string();
                }
            }
        }
    }

    Ok((contenido, tool_calls, tokens_prompt, tokens_complecion, finish_reason))
}
