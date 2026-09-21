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
    /* [20-09-2026] Los eventos grandes llegan partidos entre chunks: se
     * reensamblan en `bufer` antes de parsear (sin esto se pierden en
     * silencio: el terminal con `usage` es el que más se parte). */
    let mut bufer = BuferSse::nuevo();
    loop {
        let trozo = bytes.next().await;
        let fin = trozo.is_none();
        let texto = match trozo {
            Some(Ok(chunk)) => String::from_utf8_lossy(&chunk).into_owned(),
            Some(Err(error)) => {
                return Err(Error::Proveedor {
                    detalle: format!("Error leyendo el stream del proveedor: {error}"),
                    causa: None,
                });
            }
            None => String::new(),
        };
        for evento in bufer.eventos(&texto, fin) {
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
                /* [20-09-2026] Fallback `delta.reasoning`: algunos gateways
                 * OpenAI-compatibles emiten el pensamiento en `reasoning` en
                 * vez de `reasoning_content`. Solo strings (no bloques). */
                let pensado = delta
                    .get("reasoning_content")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| delta.get("reasoning").and_then(serde_json::Value::as_str));
                if let Some(pensado) = pensado {
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
        if fin {
            break;
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
/* [20-09-2026] Lector SSE del dialecto Responses API (`response.*`,
 * OpenCode Go / muse-spark). Misma firma y tupla que `hojear_stream`:
 * - texto: `response.output_text.delta` -> on_token (cancela igual);
 * - pensamiento visible: `response.reasoning_summary_text.delta` ->
 *   on_razonamiento (lo que la UI muestra en el summary abierto);
 * - tool calls: `response.output_item.done` tipo `function_call` -> formato
 *   chat `{"id","type":"function","function":{...}}` que espera
 *   `parsear_tool_calls`;
 * - uso: `response.completed.response.usage {input_tokens, output_tokens}`;
 * - modelo: `response.created/completed.response.model`;
 * `incomplete` (max tokens) y `failed` se marcan en finish_reason en vez de
 * fallar: el runtime ya sabe cerrar turnos parciales. */
pub(crate) async fn hojear_responses_stream(
    respuesta: reqwest::Response,
    salidas: SalidasVivo<'_>,
) -> Result<(String, String, Vec<serde_json::Value>, u32, u32, String, String), Error> {
    let SalidasVivo {
        token: on_token,
        razonamiento: on_razonamiento,
    } = salidas;
    let mut contenido = String::new();
    let mut razonamiento = String::new();
    let mut tool_calls: Vec<serde_json::Value> = Vec::new();
    let mut tokens_prompt = 0u32;
    let mut tokens_complecion = 0u32;
    let mut finish_reason = String::new();
    let mut modelo_real = String::new();

    let mut bytes = respuesta.bytes_stream();
    use futures_util::StreamExt;
    /* [20-09-2026] Búfer igual que en `hojear_stream`: el terminal
     * (`completed` con `output` + `encrypted_content`) casi siempre llega
     * partido entre chunks. */
    let mut bufer = BuferSse::nuevo();
    loop {
        let trozo = bytes.next().await;
        let fin = trozo.is_none();
        let texto = match trozo {
            Some(Ok(chunk)) => String::from_utf8_lossy(&chunk).into_owned(),
            Some(Err(error)) => {
                return Err(Error::Proveedor {
                    detalle: format!("Error leyendo el stream del proveedor: {error}"),
                    causa: None,
                });
            }
            None => String::new(),
        };
        for evento in bufer.eventos(&texto, fin) {
            let tipo = evento
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match tipo {
                "response.created" | "response.in_progress" | "response.completed"
                | "response.failed" | "response.incomplete" => {
                    if let Some(resp) = evento.get("response") {
                        if modelo_real.is_empty() {
                            if let Some(m) =
                                resp.get("model").and_then(serde_json::Value::as_str)
                            {
                                if !m.is_empty() {
                                    modelo_real = m.to_string();
                                }
                            }
                        }
                        /* [20-09-2026] El uso viaja en el evento terminal...
                         * pero el terminal NO siempre es `completed`: con
                         * presupuesto agotado el gateway cierra con
                         * `incomplete` (sin `completed` posterior) y el uso
                         * viene ahí (`response.usage {input_tokens,
                         * output_tokens}`). Se toma de cualquier evento que
                         * lo traiga, no solo de `completed`. */
                        if let Some(usage) = resp.get("usage") {
                            if let Some(entrada) = usage
                                .get("input_tokens")
                                .and_then(serde_json::Value::as_u64)
                            {
                                tokens_prompt = entrada as u32;
                            }
                            if let Some(salida) = usage
                                .get("output_tokens")
                                .and_then(serde_json::Value::as_u64)
                            {
                                tokens_complecion = salida as u32;
                            }
                        }
                        if tipo == "response.completed" || tipo == "response.incomplete" {
                            let estado = resp
                                .get("status")
                                .and_then(serde_json::Value::as_str)
                                .unwrap_or("completed");
                            finish_reason = match estado {
                                "completed" => "stop".to_string(),
                                "incomplete" => "length".to_string(),
                                other => other.to_string(),
                            };
                        }
                        if tipo == "response.failed" {
                            finish_reason = "error".to_string();
                        }
                    }
                }
                "response.output_text.delta" => {
                    if let Some(texto_delta) =
                        evento.get("delta").and_then(serde_json::Value::as_str)
                    {
                        contenido.push_str(texto_delta);
                        if !on_token(texto_delta) {
                            return Err(Error::Cancelado);
                        }
                    }
                }
                "response.reasoning_summary_text.delta" => {
                    if let Some(pensado) =
                        evento.get("delta").and_then(serde_json::Value::as_str)
                    {
                        razonamiento.push_str(pensado);
                        on_razonamiento(pensado);
                    }
                }
                "response.output_item.done" => {
                    if let Some(item) = evento.get("item") {
                        if item.get("type").and_then(serde_json::Value::as_str)
                            == Some("function_call")
                        {
                            tool_calls.push(serde_json::json!({
                                "id": item.get("call_id"),
                                "type": "function",
                                "function": {
                                    "name": item.get("name"),
                                    "arguments": item.get("arguments"),
                                },
                            }));
                        }
                    }
                }
                _ => {}
            }
        }
        if fin {
            break;
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

/// [20-09-2026] Búfer de líneas SSE entre chunks: un evento `data: {...}`
/// puede partirse a mitad de JSON entre dos chunks (los eventos terminales
/// de Responses traen `output` + `encrypted_content` de varios KB y casi
/// siempre llegan partidos). Sin búfer, `from_str` falla y el evento se
/// pierde en silencio: así se perdían `usage` y `finish_reason` (footer
/// 0/0) aunque el texto llegaba bien. Uso compartido por los dos lectores.
struct BuferSse {
    resto: String,
}

impl BuferSse {
    fn nuevo() -> Self {
        Self {
            resto: String::new(),
        }
    }

    /// Eventos completos del chunk; el fragmento final sin `\n` queda en el
    /// búfer para el siguiente chunk. Con `fin == true` (stream cerrado)
    /// el resto también se parsea: aún puede ser el evento terminal.
    fn eventos(&mut self, texto: &str, fin: bool) -> Vec<serde_json::Value> {        self.resto.push_str(texto);
        let termina_en_linea = self.resto.ends_with('\n');
        /* Se saca el acumulado a un local: las líneas prestan de él mientras
         * `self.resto` queda libre para guardar el fragmento incompleto. */
        let cuerpo = std::mem::take(&mut self.resto);
        let mut lineas: Vec<&str> = cuerpo.lines().collect();
        if !fin && !termina_en_linea {
            if let Some(incompleta) = lineas.pop() {
                self.resto = incompleta.to_string();
            }
        }
        let mut eventos = Vec::new();
        for linea in lineas {
            if let Some(evento) = extraer_evento_sse(linea) {
                eventos.push(evento);
            }
        }
        eventos
    }
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

#[cfg(test)]
mod pruebas {
    use super::BuferSse;

    /// [20-09-2026] El terminal de Responses llega partido entre chunks: el
    /// búfer debe reensamblarlo o `usage` se pierde (footer 0/0).
    #[test]
    fn bufer_reensambla_evento_partido_entre_chunks() {
        let mut bufer = BuferSse::nuevo();
        let mitad = bufer.eventos("data: {\"type\":\"response.comp", false);
        assert!(mitad.is_empty(), "el fragmento no debe emitir nada");
        let eventos = bufer.eventos("leted\",\"response\":{\"usage\":{}}}\n", false);
        assert_eq!(eventos.len(), 1);
        assert_eq!(
            eventos[0].get("type").and_then(|v| v.as_str()),
            Some("response.completed")
        );
    }

    /// Con `fin` el resto sin `\n` también se parsea (cierre sin salto).
    #[test]
    fn bufer_vacia_resto_al_cerrar() {
        let mut bufer = BuferSse::nuevo();
        assert!(bufer.eventos("data: {\"type\":\"x\"}", false).is_empty());
        let eventos = bufer.eventos("", true);
        assert_eq!(eventos.len(), 1);
    }
}
