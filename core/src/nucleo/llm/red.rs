//! [059A-N S2] Split mecánico de `llm.rs`: red/HTTP: reintentos con backoff, streaming, hojear_stream y parseo de tool_calls. Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::modelo::{
    es_dialecto_responses, es_error_transitorio, parsear_tool_calls, url_solicitud,
};
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

mod cuerpos;
mod respuestas;

pub(crate) use cuerpos::construir_cuerpo_responses;

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
        mut salidas: SalidasVivo<'_>,
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
                    .ejecutar_request_stream_con_reintentos(solicitud, &mut salidas)
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
        salidas: SalidasVivo<'_>,
    ) -> Result<AiStreamResult, Error> {
        let SolicitudStream {
            proveedor,
            api_key,
            modelo,
            mensajes,
            opciones,
            tools,
        } = solicitud;
        let url = url_solicitud(proveedor, modelo);
        let modelo = modelo_proveedor(proveedor, modelo);
        /* [20-09-2026] Dialecto Responses (muse-spark en OpenCode Go): otro
         * endpoint, otro cuerpo y otro parseo SSE. El resto del flujo
         * (reintentos, fallback, cancelación) no cambia. */
        let dialecto_responses = es_dialecto_responses(proveedor, &modelo);
        let body = if dialecto_responses {
            /* Reexportado arriba: lo usan este stream y los tests de `super`. */
            construir_cuerpo_responses(&modelo, mensajes, opciones, tools, true)
        } else {
            cuerpos::construir_cuerpo_stream(proveedor, &modelo, mensajes, opciones, tools)
        };
        let respuesta = respuestas::enviar_solicitud(
            &self.client,
            proveedor,
            &modelo,
            api_key,
            &url,
            &body,
            opciones.sesion_externa.as_deref(),
        )
        .await?;

        /* Fallback no-stream: si el proveedor no devuelve text/event-stream
         * (p. ej. un proxy que responde JSON directo), se hace la llamada
         * normal y se emite un único token. */
        if !respuestas::respuesta_es_stream(&respuesta) {
            return respuestas::resultado_no_stream(respuesta, proveedor, &modelo, salidas.token).await;
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

        let (contenido, razonamiento, tool_calls, tokens_prompt, tokens_complecion, finish_reason, modelo_real) =
            if dialecto_responses {
                super::stream::hojear_responses_stream(respuesta, salidas).await?
            } else {
                super::stream::hojear_stream(respuesta, salidas).await?
            };
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
            .or(if modelo_real.is_empty() {
                None
            } else {
                Some(modelo_real)
            })
            .unwrap_or_else(|| modelo.to_string());

        Ok(AiStreamResult {
            contenido,
            razonamiento,
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
        salidas: &mut SalidasVivo<'_>,
    ) -> Result<AiStreamResult, Error> {
        let SolicitudStream {
            proveedor, modelo, ..
        } = solicitud;
        let mut ultimo_error: Option<Error> = None;
        for intento in 0..=REINTENTOS_TRANSITORIOS {
            /* Por intento se re-prestan los callbacks (el bundle se mueve por
             * valor a cada intento; sin re-préstamo no compilaría el retry). */
            let por_intento = SalidasVivo {
                token: &mut *salidas.token,
                razonamiento: &mut *salidas.razonamiento,
            };
            match self.ejecutar_request_stream(solicitud, por_intento).await {
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
        let url = url_solicitud(proveedor, modelo);
        let modelo = modelo_proveedor(proveedor, modelo);
        /* [20-09-2026] Dialecto Responses (muse-spark): sin temperature ni
         * messages; `reasoning.effort+summary` en vez de `reasoning_effort`. */
        let dialecto_responses = es_dialecto_responses(proveedor, &modelo);

        /* Cuerpo en `cuerpos::armar_body_request` (309A-3). */
        let body =
            cuerpos::armar_body_request(proveedor, &modelo, mensajes, opciones, dialecto_responses);

        /* Envío + validación de status en `respuestas::post_json` (309A-3). */
        let datos = respuestas::post_json(
            &self.client,
            proveedor,
            &url,
            api_key,
            dialecto_responses,
            opciones.sesion_externa.as_deref(),
            &body,
        )
        .await?;

        /* Texto en `respuestas::extraer_contenido` (309A-3). */
        let contenido = respuestas::extraer_contenido(&datos, dialecto_responses)?;

        /* Resultado común en `respuestas::armar_resultado_chat` (309A-3). */
        Ok(respuestas::armar_resultado_chat(
            proveedor,
            &modelo,
            dialecto_responses,
            contenido,
            &datos,
        ))
    }
}
