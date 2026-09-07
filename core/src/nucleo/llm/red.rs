//! [059A-N S2] Split mecánico de `llm.rs`: red/HTTP: reintentos con backoff, streaming, hojear_stream y parseo de tool_calls. Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::modelo::{es_error_transitorio, parsear_tool_calls};
use super::*;

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
/// [069A-9 07-09-2026] Captura `X-Routed-Via` header (gloryapi) como
/// provider/modelo exacto, con prioridad sobre el campo `model` del body.
async fn resultado_no_stream(
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

    let contenido = datos
        .pointer("/choices/0/message/content")
        .and_then(serde_json::Value::as_str)
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

        /* [069A-9 07-09-2026] gloryapi expone el proveedor exacto que eligió
         * su router auto en el header X-Routed-Via (formato "platform/modelId",
         * ej. "andoryyu/deepseek-v4-flash"). Si está presente, se usa en lugar
         * del provider "glory" genérico y del model que el upstream reporte. */
        let routed_via: Option<(String, String)> = respuesta
            .headers()
            .get("X-Routed-Via")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split_once('/'))
            .map(|(p, m)| (p.to_string(), m.to_string()));

        let (contenido, tool_calls, tokens_prompt, tokens_complecion, finish_reason, modelo_real) =
            super::stream::hojear_stream(respuesta, on_token).await?;
        let tool_calls = parsear_tool_calls(tool_calls);

        /* [069A-9 07-09-2026] Prioridad: routed_via (gloryapi exacto) >
         * modelo_real del SSE > modelo solicitado. */
        let provider_final = routed_via
            .as_ref()
            .map(|(p, _)| p.clone())
            .unwrap_or_else(|| proveedor.to_string());
        let modelo_final = routed_via
            .as_ref()
            .map(|(_, m)| m.clone())
            .or_else(|| {
                if modelo_real.is_empty() {
                    None
                } else {
                    Some(modelo_real)
                }
            })
            .unwrap_or_else(|| modelo.to_string());

        Ok(AiStreamResult {
            contenido,
            tool_calls,
            tokens_prompt,
            tokens_complecion,
            finish_reason,
            provider: provider_final,
            modelo: modelo_final,
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
            proveedor, modelo, ..
        } = solicitud;
        let mut ultimo_error: Option<Error> = None;
        for intento in 0..=REINTENTOS_TRANSITORIOS {
            match self.ejecutar_request_stream(solicitud, on_token).await {
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
