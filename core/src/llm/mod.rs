//! Proxy LLM OpenAI-compatible con cadena de fallback, rotación de keys y
//! circuit breaker. Port agnóstico de `src/services/ai.rs` de task (plan
//! 318A-13, Fase 1b): **no** depende de `AiProviderKeys` ni de `AppError`;
//! usa tipos propios del núcleo ([`LlavesProveedor`], [`Error`]).
//!
//! El flujo: mensajes -> candidato solicitado (si el modelo es válido) ->
//! cadena de fallback -> rotación de keys del proveedor hasta una respuesta.

use crate::error::Error;
use serde::{Deserialize, Serialize};

mod modelo;
mod red;

pub use modelo::{AiChatOptions, AiChatResult, AiMessage, AiNutritionResult, AiStreamResult, AiToolCall, LlavesProveedor, candidatos_para, catalogo_proveedores};

use modelo::{PROMPT_NUTRICION, es_error_transitorio, mayuscula_primera, modelo_proveedor, resolver_candidatos, url_proveedor, validar_mensajes};

/// Estado del circuit breaker por proveedor (R7 del plan agente).
#[derive(Debug, Clone)]
struct CircuitoProveedor {
    fallos_consecutivos: u32,
    hasta: Option<std::time::Instant>,
}

/// Umbral de fallos consecutivos antes de abrir el circuito (cooldown 60s).
const CIRCUITO_UMBRAL: u32 = 3;
const CIRCUITO_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(60);

/* [318A-10 02-09-2026] Reintentos con backoff para errores TRANSITORIOS.
 * Antes cada proveedor/key se probaba una sola vez: un 503 puntual de
 * commandcode (overload de Laguna) o un timeout de red abandonaba la vía sin
 * reintentar y saltaba a la siguiente, encadenando fallos hasta el error final.
 * Ahora los errores transitorios (503/429/5xx/timeout/red) se reintentan con
 * backoff exponencial; los permanentes (400/401/403/402/404) se descartan al
 * instante porque reintentar no los arregla (auth/billing/schema). */

/// Servicio LLM con las keys del entorno. `Clone` es barato (reqwest::Client
/// comparte el pool internamente), así que vive directo en el estado del
/// consumidor. El circuit breaker (fallos consecutivos por proveedor) vive en
/// un `Mutex` compartido: `Clone` no duplica el estado.
#[derive(Debug, Clone)]
pub struct LlmProviderService {
    llaves: LlavesProveedor,
    client: reqwest::Client,
    circuito: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, CircuitoProveedor>>>,
}

/// Parámetros agrupados de un request de streaming (evita la firma larga;
/// [318A-13] clippy too_many_arguments).
#[derive(Clone, Copy)]
struct SolicitudStream<'a> {
    proveedor: &'a str,
    api_key: &'a str,
    modelo: &'a str,
    mensajes: &'a [AiMessage],
    opciones: &'a AiChatOptions,
    tools: &'a [serde_json::Value],
}


impl LlmProviderService {
    pub fn new(llaves: LlavesProveedor) -> Self {
        /* Timeout de 45s por llamada al proveedor (paridad con wp_remote_post
         * del PHP); el TimeoutLayer global da el margen de la petición.
         *
         * [059A-S7] Excepción justificada de expect-produccion-rs: la firma
         * `new -> Self` está fijada por consumidores externos (PROYECTO TASKS
         * handlers/mod.rs llama `LlmProviderService::new` sin Result) y
         * `Client::builder().build()` solo falla por backend TLS/proxy mal
         * configurado a nivel máquina — infalible en runtime normal. Si algún
         * día el builder admite un fallo real, migrar `new` a `-> Result`. */
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(45))
            .build()
            // sentinel-disable-next-line expect-produccion-rs
            .expect("reqwest client builder is infallible");
        Self {
            llaves,
            client,
            circuito: std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// ¿Está el proveedor en cooldown por fallos consecutivos?
    fn proveedor_abierto(&self, proveedor: &str) -> bool {
        let estado = self
            .circuito
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match estado.get(proveedor) {
            Some(c) => c
                .hasta
                .map(|hasta| std::time::Instant::now() < hasta)
                .unwrap_or(false),
            None => false,
        }
    }

    /// Registra un fallo (abre el circuito tras N consecutivos) o un acierto
    /// (cierra el circuito y resetea el contador).
    fn registrar_fallo(&self, proveedor: &str) {
        let mut estado = self
            .circuito
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let entrada = estado.entry(proveedor.to_string()).or_insert(CircuitoProveedor {
            fallos_consecutivos: 0,
            hasta: None,
        });
        entrada.fallos_consecutivos += 1;
        if entrada.fallos_consecutivos >= CIRCUITO_UMBRAL {
            entrada.hasta = Some(std::time::Instant::now() + CIRCUITO_COOLDOWN);
            tracing::warn!(proveedor, fallos = entrada.fallos_consecutivos, cooldown_s = 60, "circuit breaker abierto");
        }
    }

    fn registrar_acierto(&self, proveedor: &str) {
        let mut estado = self
            .circuito
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(entrada) = estado.get_mut(proveedor) {
            entrada.fallos_consecutivos = 0;
            entrada.hasta = None;
        }
    }

    /* [318A-10 02-09-2026] Fallo PERMANENTE (400/401/403/402/404): NO abre el
     * circuito ni cuenta para el cooldown. Reintentar no arregla auth/billing/
     * schema, y contarlos solo provocaba que proveedores con 403 (groq) o 402
     * (cerebras) entraran en cooldown injustificadamente y bloquearan la vía
     * durante 60s aunque el problema fuera de la cuenta, no del servicio. */
    fn registrar_fallo_permanente(&self, proveedor: &str) {
        tracing::debug!(proveedor, "fallo permanente del proveedor (no abre circuito)");
    }

    pub async fn enviar_chat(
        &self,
        mensajes: Vec<AiMessage>,
        provider: &str,
        modelo: &str,
        opciones: AiChatOptions,
    ) -> Result<AiChatResult, Error> {
        let mensajes_validos = validar_mensajes(mensajes)?;
        let mut errores: Vec<String> = Vec::new();

        for (proveedor, modelo) in resolver_candidatos(provider, modelo) {
            /* [29-08-2026] Circuit breaker (R7): si el proveedor lleva N fallos
             * consecutivos, se aparta 60s y se prueba el siguiente de la cadena. */
            if self.proveedor_abierto(proveedor) {
                errores.push(format!(
                    "{proveedor}/{modelo}: proveedor en cooldown por fallos consecutivos"
                ));
                continue;
            }
            let keys = self.keys_para(proveedor);
            /* [27-08-2026] Glory API (free.empero.org) respondía sin API key.
             * [02-09-2026] Glory API ahora es gloryapi LOCAL (127.0.0.1:3101)
             * y SÍ requiere la unified key (GLORY_API_KEY). Se conserva el
             * intento sin key como fallback defensivo por si el operador
             * apunta GLORY_API_URL a un endpoint que no la exija. */
            if keys.is_empty() {
                if proveedor == "glory" {
                    match self
                        .ejecutar_request_con_reintentos(proveedor, "", modelo, &mensajes_validos, &opciones)
                        .await
                    {
                        Ok(resultado) => {
                            self.registrar_acierto(proveedor);
                            return Ok(resultado);
                        }
                        Err(error) => {
                            tracing::warn!(%error, proveedor, modelo, "glory sin key falló");
                            if es_error_transitorio(&error) {
                                self.registrar_fallo(proveedor);
                            } else {
                                self.registrar_fallo_permanente(proveedor);
                            }
                            errores.push(format!("{proveedor}/{modelo}: {error}"));
                        }
                    }
                } else {
                    errores.push(format!(
                        "No hay API key configurada para {proveedor} en el entorno"
                    ));
                }
                continue;
            }
            for key in keys {
                match self
                    .ejecutar_request_con_reintentos(proveedor, key, modelo, &mensajes_validos, &opciones)
                    .await
                {
                    Ok(resultado) => {
                        self.registrar_acierto(proveedor);
                        return Ok(resultado);
                    }
                    Err(error) => {
                        if es_error_transitorio(&error) {
                            self.registrar_fallo(proveedor);
                        } else {
                            self.registrar_fallo_permanente(proveedor);
                        }
                        errores.push(format!("{proveedor}/{modelo}: {error}"));
                    }
                }
            }
        }

        /* [26-08-2026] Reportar TODA la cadena de errores (sin duplicados por
         * proveedor/modelo), no solo el último: ocultar los fallos previos
         * impedía diagnosticar por qué cerebras/groq no respondían. */
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

    pub async fn estimar_nutricion(
        &self,
        descripcion: String,
        provider: &str,
        modelo: &str,
    ) -> Result<AiNutritionResult, Error> {
        let descripcion = descripcion.trim().to_string();
        if descripcion.is_empty() || descripcion.chars().count() > 1200 {
            return Err(Error::Validacion(
                "Descripción de comida inválida".into(),
            ));
        }

        let mensajes = vec![
            AiMessage::texto("system", PROMPT_NUTRICION),
            AiMessage::texto("user", descripcion.clone()),
        ];
        let respuesta = self
            .enviar_chat(
                mensajes,
                provider,
                modelo,
                AiChatOptions {
                    temperature: 0.1,
                    /* [26-08-2026] 180 era el contrato PHP, pero los modelos
                     * actuales (compound-mini, gpt-oss, qwen) razonan antes de
                     * responder: con presupuesto corto se cortan en thinking y
                     * devuelven content vacío o JSON truncado. 512 deja margen. */
                    max_tokens: 512,
                    reasoning_effort: None,
                },
            )
            .await?;

        /* El modelo puede devolver el JSON envuelto en backticks de markdown
         * y/o precedido de un bloque think.../think (modelos que razonan en
         * voz alta). Se limpia todo eso antes de parsear. */
        let contenido = respuesta.contenido.trim();
        /* [26-08-2026] Los modelos que razonan en voz alta (compound-mini,
         * qwen, gpt-oss) pueden envolver su razonamiento en think.../think
         * ANTES del JSON. Hay que remover el bloque COMPLETO (etiquetas y
         * contenido interior), no solo las etiquetas: si el texto del
         * razonamiento queda pegado al JSON, el parseo falla. */
        let mut sin_think = contenido.to_string();
        loop {
            let inicio = sin_think.find("<think");
            let fin = sin_think.find("</think");
            match (inicio, fin) {
                (Some(i), Some(f)) if f > i => {
                    let antes = &sin_think[..i];
                    let despues = &sin_think[f + "</think".len()..];
                    sin_think = format!("{antes}{despues}");
                }
                _ => break,
            }
        }
        let sin_think = sin_think.trim();
        let json = sin_think
            .strip_prefix("```json")
            .or_else(|| sin_think.strip_prefix("```"))
            .map(str::trim_start)
            .unwrap_or(sin_think)
            .trim_end_matches("```")
            .trim();

        let datos: serde_json::Value = serde_json::from_str(json).map_err(|_| {
            Error::Proveedor {
                detalle: format!(
                    "La IA no devolvió macros válidos. Reintenta con una descripción más concreta (JSON: {})",
                    contenido.chars().take(120).collect::<String>()
                ),
                causa: None,
            }
        })?;

        let numero = |clave: &str| -> Option<i64> {
            datos
                .get(clave)
                .and_then(serde_json::Value::as_f64)
                .map(|v| v.round() as i64)
        };
        let calorias = numero("calorias").ok_or_else(|| Error::Proveedor {
            detalle: "La IA no devolvió macros válidos. Reintenta con una descripción más concreta"
                .into(),
            causa: None,
        })?;

        Ok(AiNutritionResult {
            calorias,
            proteinas: numero("proteinas").unwrap_or(0),
            carbohidratos: numero("carbohidratos").unwrap_or(0),
            grasas: numero("grasas").unwrap_or(0),
            azucar: numero("azucar").unwrap_or(0),
            descripcion: mayuscula_primera(&descripcion),
            provider: respuesta.provider,
            modelo: respuesta.modelo,
        })
    }

}

impl LlmProviderService {
    fn keys_para(&self, proveedor: &str) -> &[String] {
        match proveedor {
            "cerebras" => &self.llaves.cerebras,
            "groq" => &self.llaves.groq,
            "deepseek" => &self.llaves.deepseek,
            "glory" => &self.llaves.glory,
            "commandcode" => &self.llaves.commandcode,
            _ => &[],
        }
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::modelo::parsear_tool_calls;

    #[test]
    fn parsear_tool_calls_descarta_malformadas_y_sintetiza_id() {
        // tool_call completa con id y nombre.
        let completa = serde_json::json!({
            "id": "call_abc",
            "type": "function",
            "function": { "name": "crear_tarea", "arguments": "{\"texto\":\"x\"}" }
        });
        // tool_call sin id (Laguna S 2.1 free): se sintetiza id.
        let sin_id = serde_json::json!({
            "type": "function",
            "function": { "name": "crear_tarea", "arguments": "{\"texto\":\"y\"}" }
        });
        // tool_call sin nombre (malformada): se descarta.
        let sin_nombre = serde_json::json!({
            "id": "call_xyz",
            "type": "function",
            "function": { "name": "", "arguments": "{}" }
        });
        let resultado = parsear_tool_calls(vec![completa, sin_id, sin_nombre]);
        assert_eq!(resultado.len(), 2, "la malformada sin nombre se descarta");
        assert_eq!(resultado[0].id, "call_abc");
        assert_eq!(resultado[0].nombre, "crear_tarea");
        assert_eq!(resultado[1].nombre, "crear_tarea");
        assert!(
            !resultado[1].id.is_empty(),
            "el id faltante se sintetiza (no vacío)"
        );
        assert_ne!(resultado[1].id, resultado[0].id, "ids sintetizados únicos");
    }

    #[test]
    fn parsear_tool_calls_argumentos_invalidos_son_objeto_vacio() {
        let con_args_rotos = serde_json::json!({
            "id": "call_1",
            "function": { "name": "buscar", "arguments": "no-json" }
        });
        let resultado = parsear_tool_calls(vec![con_args_rotos]);
        assert_eq!(resultado.len(), 1);
        assert_eq!(resultado[0].argumentos, serde_json::json!({}));
    }

    #[test]
    fn catalogo_proveedores_expone_allowlist_sin_duplicar_tabla() {
        // 039A-1 F4: la UI lista proveedores/modelos reales desde el núcleo.
        let catalogo = catalogo_proveedores();
        assert!(!catalogo.is_empty(), "el catálogo no está vacío");
        let ids: Vec<&str> = catalogo.iter().map(|(id, _)| *id).collect();
        for esperado in ["groq", "deepseek", "glory", "commandcode", "cerebras"] {
            assert!(ids.contains(&esperado), "falta proveedor {esperado}");
        }
        for (id, modelos) in &catalogo {
            assert!(!modelos.is_empty(), "el proveedor {id} no tiene modelos");
        }
    }

    #[test]
    fn es_error_transitorio_distingue_5xx_y_429_de_permanentes() {
        // "503 Service Unavailable" (Display de reqwest incluye la razón).
        let t503 = Error::Proveedor { detalle: "commandcode 503 Service Unavailable: overloaded".into(), causa: None };
        assert!(es_error_transitorio(&t503));
        let t500 = Error::Proveedor { detalle: "groq 500 Internal Server Error: x".into(), causa: None };
        assert!(es_error_transitorio(&t500));
        let t429 = Error::Proveedor { detalle: "groq 429 Too Many Requests: limit".into(), causa: None };
        assert!(es_error_transitorio(&t429));
        // Red / stream.
        let red = Error::Proveedor { detalle: "Error de red: timeout".into(), causa: None };
        assert!(es_error_transitorio(&red));
        let stream = Error::Proveedor { detalle: "Error leyendo el stream del proveedor: eof".into(), causa: None };
        assert!(es_error_transitorio(&stream));
        // Permanentes: 4xx de auth/billing/schema y errores sin status.
        let p400 = Error::Proveedor { detalle: "commandcode 400 Bad Request: tool_call_id".into(), causa: None };
        assert!(!es_error_transitorio(&p400));
        let p403 = Error::Proveedor { detalle: "groq 403 Forbidden: Forbidden".into(), causa: None };
        assert!(!es_error_transitorio(&p403));
        let p402 = Error::Proveedor { detalle: "cerebras 402 Payment Required: x".into(), causa: None };
        assert!(!es_error_transitorio(&p402));
        let sin_status = Error::Proveedor { detalle: "No se pudo contactar un modelo IA disponible".into(), causa: None };
        assert!(!es_error_transitorio(&sin_status));
    }

    #[test]
    fn validar_mensajes_conserva_par_assistant_tool_calls_y_tool() {
        let mensajes = vec![
            AiMessage::texto("system", "sistema"),
            AiMessage::texto("user", "crea una tarea"),
            AiMessage {
                role: "assistant".into(),
                content: serde_json::Value::Null,
                tool_calls: Some(vec![AiToolCall {
                    id: "call_1".into(),
                    nombre: "crear_tarea".into(),
                    argumentos: serde_json::json!({"texto": "x"}),
                }]),
                tool_call_id: None,
            },
            AiMessage {
                role: "tool".into(),
                content: serde_json::Value::String("ok".into()),
                tool_calls: None,
                tool_call_id: Some("call_1".into()),
            },
            AiMessage::texto("user", "termina"),
        ];
        let result = validar_mensajes(mensajes).expect("debe validar");
        // assistant(tool_calls) + tool se conservan, en orden.
        let idx_assistant = result.iter().position(|m| m.role == "assistant").unwrap();
        let idx_tool = result.iter().position(|m| m.role == "tool").unwrap();
        assert!(idx_assistant < idx_tool, "assistant precede a tool");
        let tool = &result[idx_tool];
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_1"));
    }

    #[test]
    fn validar_mensajes_elimina_tool_huerfano_y_repara_id_vacio() {
        // tool huérfano (sin assistant previo con tool_calls): se descarta.
        let huerfano = vec![
            AiMessage::texto("user", "hola"),
            AiMessage {
                role: "tool".into(),
                content: serde_json::Value::String("residuo".into()),
                tool_calls: None,
                tool_call_id: Some("call_99".into()),
            },
        ];
        let result = validar_mensajes(huerfano).expect("debe validar");
        assert!(
            !result.iter().any(|m| m.role == "tool"),
            "tool huérfano se elimina"
        );

        // tool con id vacío precedido por assistant con tool_calls: se repara.
        let reparar = vec![
            AiMessage::texto("user", "haz algo"),
            AiMessage {
                role: "assistant".into(),
                content: serde_json::Value::Null,
                tool_calls: Some(vec![AiToolCall {
                    id: "call_7".into(),
                    nombre: "crear_tarea".into(),
                    argumentos: serde_json::json!({}),
                }]),
                tool_call_id: None,
            },
            AiMessage {
                role: "tool".into(),
                content: serde_json::Value::String("hecho".into()),
                tool_calls: None,
                tool_call_id: Some(String::new()),
            },
        ];
        let result = validar_mensajes(reparar).expect("debe validar");
        let tool = result.iter().find(|m| m.role == "tool").unwrap();
        assert_eq!(tool.tool_call_id.as_deref(), Some("call_7"));
    }

    #[test]
    fn circuito_abre_tras_fallos_y_cierra_con_acierto() {
        let servicio = LlmProviderService::new(LlavesProveedor::default());
        // Sin fallos: abierto = false.
        assert!(!servicio.proveedor_abierto("groq"));
        // 2 fallos: aún cerrado (umbral 3).
        servicio.registrar_fallo("groq");
        servicio.registrar_fallo("groq");
        assert!(!servicio.proveedor_abierto("groq"));
        // 3er fallo: abre.
        servicio.registrar_fallo("groq");
        assert!(servicio.proveedor_abierto("groq"));
        // Un acierto cierra y resetea.
        servicio.registrar_acierto("groq");
        assert!(!servicio.proveedor_abierto("groq"));
    }

    #[test]
    fn circuitos_son_independientes_por_proveedor() {
        let servicio = LlmProviderService::new(LlavesProveedor::default());
        for _ in 0..3 {
            servicio.registrar_fallo("cerebras");
        }
        assert!(servicio.proveedor_abierto("cerebras"));
        assert!(!servicio.proveedor_abierto("groq"));
    }

    /// Paridad con el original de task: `commandcode` sin key no candidatea
    /// con key vacía; glory sí (intento sin key defensivo).
    #[test]
    fn resolver_candidatos_glory_sin_key_es_primero_tras_solicitado() {
        let candidatos = resolver_candidatos("glory", "commandcode");
        assert_eq!(candidatos[0], ("glory", "commandcode"));
        // La cadena no duplica el candidato solicitado.
        let pares_glory: Vec<_> = candidatos
            .iter()
            .filter(|(p, _)| *p == "glory")
            .collect();
        assert_eq!(pares_glory.len(), 2, "glory/commandcode + glory/glm-5.3-flash");
    }

    #[test]
    fn candidato_invalido_cae_a_la_cadena() {
        let candidatos = resolver_candidatos("groq", "modelo-inexistente");
        assert_eq!(candidatos[0], ("commandcode", "poolside/laguna-s-2.1-free"));
    }
}