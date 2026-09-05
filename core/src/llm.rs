//! Proxy LLM OpenAI-compatible con cadena de fallback, rotación de keys y
//! circuit breaker. Port agnóstico de `src/services/ai.rs` de task (plan
//! 318A-13, Fase 1b): **no** depende de `AiProviderKeys` ni de `AppError`;
//! usa tipos propios del núcleo ([`LlavesProveedor`], [`Error`]).
//!
//! El flujo: mensajes -> candidato solicitado (si el modelo es válido) ->
//! cadena de fallback -> rotación de keys del proveedor hasta una respuesta.

use crate::error::Error;
use serde::{Deserialize, Serialize};

/// Proveedores soportados: url de la API y modelos permitidos (allowlist).
/// Los nombres de env replican exactamente LLMProviderService.php (Coolify).
const PROVIDERS: &[(&str, &str, &[&str])] = &[
    (
        "groq",
        "https://api.groq.com/openai/v1/chat/completions",
        &[
            "groq/compound",
            "groq/compound-mini",
            "openai/gpt-oss-20b",
            "openai/gpt-oss-120b",
            "qwen/qwen3.6-27b",
        ],
    ),
    (
        "deepseek",
        "https://api.deepseek.com/chat/completions",
        &["deepseek-v4-flash"],
    ),
    /* [02-09-2026] Glory API = gloryapi LOCAL (http://127.0.0.1:3101). La URL
     * real se resuelve en tiempo de ejecución por url_proveedor(): env
     * GLORY_API_URL con default gloryapi local; la URL estática de aquí solo
     * alimenta la validación del allowlist de modelos. La ruta "auto" usa el
     * modelo `commandcode`, que mapea a deepseek/deepseek-v4-flash en gloryapi
     * (la vía que el usuario prefiere porque siempre funciona).
     * [318A-11 02-09-2026] Allowlist ampliada con los modelos REALES del
     * catálogo de gloryapi (verificado contra GET /v1/models el 02-09):
     * deepseek-v4-flash y sus variantes, deepseek-ai/deepseek-v4-flash-0731,
     * deepseek/deepseek-v4-flash, meta/muse-spark-1.2-contributor y
     * stealth/ox-alpha. Se conservan los alias legacy commandcode/glm-5.3-flash
     * (ruta auto) y `auto` como router explícito. Los IDs reales se pasan tal
     * cual a gloryapi en modelo_proveedor(). */
    (
        "glory",
        "http://127.0.0.1:3101/v1/chat/completions",
        &[
            "auto",
            "commandcode",
            "glm-5.3-flash",
            "deepseek-v4-flash",
            "deepseek-v4-flash-free",
            "deepseek-v4-flash:free",
            "deepseek-ai/deepseek-v4-flash-0731",
            "deepseek/deepseek-v4-flash",
            "meta/muse-spark-1.2-contributor",
            "stealth/ox-alpha",
        ],
    ),
    /* [02-09-2026] Command Code Provider API DIRECTA (sin gloryapi): la key
     * del Studio/CLI (COMMAND_CODE_API_KEY) autentica Bearer. Modelo gratuito
     * `poolside/laguna-s-2.1-free` (Laguna S 2.1 de Poolside, 100% OFF while
     * capacity lasts) — el prefijo `poolside/` es el ID real que el endpoint
     * Provider espera (verificado contra GET /provider/v1/models el 02-09).
     * Requiere al menos $1 de créditos en la cuenta para arrancar. */
    (
        "commandcode",
        "https://api.commandcode.ai/provider/v1/chat/completions",
        &["poolside/laguna-s-2.1-free"],
    ),
    (
        "cerebras",
        "https://api.cerebras.ai/v1/chat/completions",
        &["gemma-4-31b", "gpt-oss-120b"],
    ),
];

/// Catálogo público de proveedores (039A-1 F4): id + modelos del allowlist,
/// para que la UI liste proveedores/modelos reales sin duplicar la tabla.
/// Aditivo: no cambia validación ni cadena de fallback.
pub fn catalogo_proveedores() -> Vec<(&'static str, Vec<&'static str>)> {
    PROVIDERS
        .iter()
        .map(|(id, _, modelos)| (*id, modelos.to_vec()))
        .collect()
}

/// Cadena de fallback cuando el candidato solicitado falla (PHP CHAT_FALLBACK_CHAIN).
/* [26-08-2026] Cadena actualizada a modelos reales de la cuenta (verificados
 * contra /models de cada proveedor el 26-08): groq/compound-mini responde
 * JSON en pocos tokens (ideal nutrición); gpt-oss son de razonamiento y
 * agotan el presupuesto corto dejando content vacío, por eso van detrás. */
const CHAT_FALLBACK_CHAIN: &[(&str, &str)] = &[
    /* [02-09-2026] Command Code Provider API directa con el modelo GRATIS
     * `poolside/laguna-s-2.1-free` va PRIMERO: cuesta $0 y es la vía
     * preferida para probar el agente sin pasar por gloryapi. Si no hay key
     * o falla, cae a glory (ruta auto -> DeepSeek Flash). */
    ("commandcode", "poolside/laguna-s-2.1-free"),
    /* [318A-10 02-09-2026] DeepSeek DIRECTO (api.deepseek.com) va SEGUNDO:
     * es la vía que "siempre funciona" (modelo `deepseek-v4-flash` verificado
     * contra GET /models con la key real de DEEPSEEK-API). Antes quedaba de
     * ÚLTIMO, tras 6 proveedores que fallan con 400/403/402, y además la key
     * no llegaba al backend por el bug del regex del script de reinicio. Con
     * key presente y posición temprana, cuando commandcode (gratis pero
     * inestable, da 503 puntuales) falla, el agente salta directo a la vía
     * fiable en lugar de encadenar 8 fallos. */
    ("deepseek", "deepseek-v4-flash"),
    /* [29-08-2026] Glory API/`commandcode` (ruta auto -> DeepSeek Flash) sin
     * clave va PRIMERO: es la vía que siempre funciona y el default del agente.
     * La nutrición no cambia: pasa un modelo groq válido, que `candidato_valido`
     * pone antes de esta cadena (la cadena solo rige cuando el candidato
     * solicitado es inválido/ausente). */
    ("glory", "commandcode"),
    ("glory", "glm-5.3-flash"),
    ("groq", "groq/compound-mini"),
    ("groq", "groq/compound"),
    ("cerebras", "gemma-4-31b"),
    ("groq", "openai/gpt-oss-20b"),
    ("groq", "openai/gpt-oss-120b"),
    ("groq", "qwen/qwen3.6-27b"),
];

/* [02-09-2026] Glory API = gloryapi LOCAL. URL configurable por env
 * (GLORY_API_URL, default http://127.0.0.1:3101/v1/chat/completions) para no
 * hardcodear loopback en producción: si el operador despliega contra una
 * gloryapi remota o mantiene free.empero.org, lo decide el entorno, no el
 * código. */
fn url_proveedor(proveedor: &str) -> String {
    if proveedor == "glory" {
        return std::env::var("GLORY_API_URL").unwrap_or_else(|_| {
            "http://127.0.0.1:3101/v1/chat/completions".to_string()
        });
    }
    PROVIDERS
        .iter()
        .find(|(id, _, _)| *id == proveedor)
        .map(|(_, url, _)| (*url).to_string())
        .unwrap_or_default()
}

/* [02-09-2026] Glory API local mapea el alias interno `commandcode` (y
 * `glm-5.3-flash`) al ID real del catálogo de gloryapi. El alias que el
 * usuario ve en la UI es `commandcode`; el request real usa el ID del
 * catálogo para que gloryapi lo enrute a DeepSeek V4 Flash.
 * [02-09-2026] El proveedor `commandcode` (Provider API directa) NO mapea:
 * sus modelos (`poolside/laguna-s-2.1-free`) son IDs reales y se pasan tal cual.
 * [318A-11 02-09-2026] `auto` (router explícito de gloryapi) también se
 * resuelve a deepseek/deepseek-v4-flash: es la vía que "siempre funciona"
 * (preferencia documentada del usuario). Los demás IDs reales del catálogo
 * (deepseek-v4-flash*, deepseek-ai/..., meta/muse-spark-1.2-contributor,
 * stealth/ox-alpha) se pasan tal cual a gloryapi: ya son IDs del catálogo. */
fn modelo_proveedor(proveedor: &str, modelo: &str) -> String {
    if proveedor == "glory" {
        match modelo {
            "commandcode" | "glm-5.3-flash" | "auto" => {
                "deepseek/deepseek-v4-flash".to_string()
            }
            otro => otro.to_string(),
        }
    } else {
        modelo.to_string()
    }
}

/// Prompt de nutrición calibrado regional (mismo que el front para que el
/// admin reciba el mismo comportamiento que un usuario con key propia).
const PROMPT_NUTRICION: &str = "You are a certified nutritionist estimating macros for a home-cooked Latin American diet.
Rules:
- Use USDA FoodData Central values. For Venezuelan/Latin foods use accurate regional data.
- Assume food is COOKED unless explicitly stated raw. This is critical for rice, pasta, grains (cooked rice ≈ 130 kcal/100g, NOT 360 kcal/100g raw).
- If fried (frito), account for absorbed oil. If with skin (con cuero), include it.
- Be conservative: use home-portion sizes, not restaurant. When uncertain, pick the lower reasonable estimate.
- Never fabricate values. Use the closest known food if exact data is unavailable.

Informal measurements (common in casual Spanish input):
- \"puño\" (handful) ≈ 75-85g of cooked grains/rice/pasta
- \"tajada\" (slice of fried ripe plantain) ≈ 35-45g per slice (~50-60 kcal each)
- \"media arepa\" = half an arepa. A standard homemade arepa (corn, no filling) ≈ 120-150 kcal, so half ≈ 60-75 kcal.
- \"cucharada\" (tablespoon) ≈ 15ml/15g. \"Cucharadita\" (teaspoon) ≈ 5ml.
- \"pedazo\"/\"trozo\" (piece) = a modest single portion unless context says otherwise.
- \"plato\" (plate) = a normal home serving, not heaped.
- If no quantity is specified, use ONE standard home serving.

Calibration references (use these as anchors):
- 1 arepa de maíz sin relleno: ~130 kcal
- 1 huevo entero: ~72 kcal
- 1 tajada de plátano maduro frito: ~55 kcal
- 100g arroz blanco cocido: ~130 kcal
- 1 puño arroz cocido (~80g): ~104 kcal

Respond ONLY with valid JSON, no markdown, no explanation.
JSON format:
{\"calorias\":<kcal>,\"proteinas\":<g>,\"carbohidratos\":<g>,\"grasas\":<g>,\"azucar\":<g>}";

/// Mensaje del chat (contrato del front). `content` puede ser string (texto)
/// o array (multimodal, p. ej. vision) — igual que en PHP validarMensajes().
/// `tool_calls`/`tool_call_id` son del agente (contrato OpenAI para tools);
/// el front los omite (default).
#[derive(Debug, Clone, Deserialize, Serialize, utoipa::ToSchema)]
pub struct AiMessage {
    pub role: String,
    pub content: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<AiToolCall>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl AiMessage {
    #[must_use]
    pub fn texto(role: &str, contenido: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            content: serde_json::Value::String(contenido.into()),
            tool_calls: None,
            tool_call_id: None,
        }
    }
}

/// Tool call propuesta por el modelo (agente): id + nombre + argumentos JSON.
/// Serializa al formato OpenAI de `tool_calls` en un mensaje assistant
/// (`function.name` + `function.arguments` como string JSON), que es lo que
/// exige el proveedor al reenviar el historial con tools.
#[derive(Debug, Clone, Deserialize)]
pub struct AiToolCall {
    pub id: String,
    pub nombre: String,
    pub argumentos: serde_json::Value,
}

impl Serialize for AiToolCall {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("AiToolCall", 3)?;
        state.serialize_field("id", &self.id)?;
        state.serialize_field("type", "function")?;
        state.serialize_field(
            "function",
            &serde_json::json!({
                "name": self.nombre,
                "arguments": self.argumentos.to_string(),
            }),
        )?;
        state.end()
    }
}

/// Resultado de una llamada con streaming y tool calls (agente).
#[derive(Debug, Clone)]
pub struct AiStreamResult {
    pub contenido: String,
    pub tool_calls: Vec<AiToolCall>,
    pub tokens_prompt: u32,
    pub tokens_complecion: u32,
    pub finish_reason: String,
    pub provider: String,
    pub modelo: String,
}

#[derive(Debug, Clone)]
pub struct AiChatOptions {
    pub temperature: f32,
    pub max_tokens: u32,
    /// [318A-10 02-09-2026] Nivel de razonamiento del modelo (contrato OpenAI
    /// `reasoning_effort`): `low` | `medium` | `high`. `None` = no se envía
    /// (el proveedor usa su default). Lo decide el usuario en el panel del
    /// agente; solo se envía a proveedores que lo aceptan (deepseek, groq,
    /// cerebras con modelos de razonamiento).
    pub reasoning_effort: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AiChatResult {
    pub contenido: String,
    pub tokens_prompt: u32,
    pub tokens_complecion: u32,
    pub finish_reason: String,
    pub provider: String,
    pub modelo: String,
}

#[derive(Debug, Clone)]
pub struct AiNutritionResult {
    pub calorias: i64,
    pub proteinas: i64,
    pub carbohidratos: i64,
    pub grasas: i64,
    pub azucar: i64,
    pub descripcion: String,
    pub provider: String,
    pub modelo: String,
}

/// Keys de los proveedores en el formato del núcleo (reemplaza
/// `crate::config::AiProviderKeys` de task). Varias claves por proveedor =
/// rotación (se prueban en orden hasta que una responde).
#[derive(Debug, Clone, Default)]
pub struct LlavesProveedor {
    pub cerebras: Vec<String>,
    pub groq: Vec<String>,
    pub deepseek: Vec<String>,
    pub glory: Vec<String>,
    /// [02-09-2026] Command Code Provider API directa (api.commandcode.ai).
    /// Env: COMMAND_CODE_API_KEY (la misma key del Studio/CLI).
    pub commandcode: Vec<String>,
}

impl LlavesProveedor {
    /// Carga las keys desde variables de entorno (mismos nombres que task:
    /// GROQ_API/GROQ_API_1..3, DEEPSEEK_API/DEEPSEEK-API/DEEPSEEK_API_KEY,
    /// GLORY_API_KEY/GLORY_API/EMPERO_API_KEY, COMMAND_CODE_API_KEY,
    /// CEREBRAS_API_KEY).
    #[must_use]
    pub fn from_env() -> Self {
        fn env_list(names: &[&str]) -> Vec<String> {
            names
                .iter()
                .filter_map(|name| std::env::var(name).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .collect()
        }
        Self {
            cerebras: env_list(&["CEREBRAS_API_KEY"]),
            groq: env_list(&["GROQ_API", "GROQ_API_1", "GROQ_API_2", "GROQ_API_3"]),
            deepseek: env_list(&["DEEPSEEK_API", "DEEPSEEK-API", "DEEPSEEK_API_KEY"]),
            glory: env_list(&["GLORY_API_KEY", "GLORY_API", "EMPERO_API_KEY"]),
            commandcode: env_list(&["COMMAND_CODE_API_KEY"]),
        }
    }
}

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
const REINTENTOS_TRANSITORIOS: u32 = 2;
const BACKOFF_BASE_MS: u64 = 500;

/// ¿El error del proveedor es transitorio (reintentar tiene sentido)?
/// `Error::Proveedor` con prefijo de red (reqwest send/read) o con un código
/// HTTP 5xx/429; el resto (4xx de auth/billing/schema) es permanente.
fn es_error_transitorio(error: &Error) -> bool {
    let Error::Proveedor { detalle, causa: _ } = error else {
        return false;
    };
    if detalle.starts_with("Error de red:") || detalle.contains("Error leyendo el stream") {
        return true;
    }
    /* El detalle tiene la forma "{proveedor} {status}: {mensaje}" donde
     * {status} es el Display de reqwest StatusCode, p. ej. "503 Service
     * Unavailable" (incluye la razón). Extraemos el primer token numérico
     * tras el proveedor (el código de 3 dígitos). */
    let despues_proveedor = detalle
        .find(' ')
        .and_then(|i| detalle.get(i + 1..))
        .unwrap_or("");
    let status_txt = despues_proveedor
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim();
    let Ok(status) = status_txt.parse::<u16>() else {
        return false;
    };
    status == 429 || (500..=599).contains(&status)
}

/// Espera de backoff exponencial entre reintentos (0.5s, 1.5s). Devuelve
/// inmediatamente si el intento es el primero (sin espera previa).
async fn esperar_backoff(intento: u32) {
    if intento == 0 {
        return;
    }
    let ms = BACKOFF_BASE_MS * 2u64.pow(intento);
    tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
}

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
    #[must_use]
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

        let content_type = respuesta
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        /* Fallback no-stream: si el proveedor no devuelve text/event-stream
         * (p. ej. un proxy que responde JSON directo), se hace la llamada
         * normal y se emite un único token. */
        if !content_type.contains("text/event-stream") {
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
            /* Fase 4: si el cliente canceló (on_token → false), se aborta y
             * no se devuelve una respuesta parcial como resultado exitoso. */
            if !on_token(&contenido) {
                return Err(Error::Cancelado);
            }
            return Ok(AiStreamResult {
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
            });
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
    async fn ejecutar_request_con_reintentos(
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

/// Valida roles y contenido, recorta a los últimos 25 mensajes y limita el
/// contenido de texto a 12000 caracteres (paridad con PHP validarMensajes()).
fn validar_mensajes(mensajes: Vec<AiMessage>) -> Result<Vec<AiMessage>, Error> {
    let validos: Vec<AiMessage> = mensajes
        .into_iter()
        .rev()
        .take(25)
        /* [27-08-2026] El rev().take(25) anterior dejaba el historial EN ORDEN
         * INVERSO: el último mensaje enviado al proveedor era el más antiguo
         * (system), y groq/glory rechazaban con "last message role must be
         * 'user'". Se vuelve a invertir para restaurar el orden cronológico. */
        .rev()
        .filter(|mensaje| {
            /* [29-08-2026] El agente usa role `tool` (resultado de tool con
             * tool_call_id); sin él el modelo no ve el resultado y repite la
             * llamada. Los mensajes de tool van junto a su assistant previo. */
            if !matches!(
                mensaje.role.as_str(),
                "system" | "user" | "assistant" | "tool"
            ) {
                return false;
            }
            /* [01-09-2026] Fix 318A-11: el `assistant` con `tool_calls` lleva
             * content a Null (contrato OpenAI: assistant con tool_calls +
             * tool con tool_call_id). El filtro anterior lo descartaba por
             * `_ => false`, dejando `tool` huérfanos y el proveedor
             * (commandcode/deepseek) respondía 400 "Messages with role 'tool'
             * must be a response to a preceding message with 'tool_calls'". */
            if mensaje.role == "assistant"
                && mensaje
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tc| !tc.is_empty())
            {
                return true;
            }
            match &mensaje.content {
                serde_json::Value::String(texto) => !texto.trim().is_empty(),
                serde_json::Value::Array(items) => !items.is_empty(),
                _ => false,
            }
        })
        .map(|mut mensaje| {
            if let serde_json::Value::String(texto) = &mensaje.content {
                let recortado: String = texto.chars().take(12_000).collect();
                mensaje.content = serde_json::Value::String(recortado);
            }
            mensaje
        })
        .collect();

    /* [01-09-2026] Fix 318A-11: saneo defensivo. Si el recorte a 25 rompe un
     * par assistant(tool_calls)/tool, descartamos los `tool` huérfanos en vez
     * de enviarlos (el proveedor los rechaza con 400).
     * [318A-10 02-09-2026] Además se repara el `tool` que precede a un
     * assistant con tool_calls pero sin `tool_call_id` (o con id vacío): se le
     * copia el primer id de la tool_call del assistant previo. Así el par
     * cumple el contrato OpenAI aunque el historial venga de un turno roto. */
    let mut sanitizados: Vec<AiMessage> = Vec::with_capacity(validos.len());
    let mut precedido_por_tool_calls = false;
    let mut id_tool_call_previo: Option<String> = None;
    for mut mensaje in validos {
        if mensaje.role == "tool" && !precedido_por_tool_calls {
            continue;
        }
        if mensaje.role == "tool" {
            let id_ok = mensaje
                .tool_call_id
                .as_deref()
                .is_some_and(|id| !id.trim().is_empty());
            if !id_ok {
                if let Some(id) = id_tool_call_previo.clone() {
                    mensaje.tool_call_id = Some(id);
                } else {
                    /* tool sin id y sin assistant previo con id: huérfano de
                     * facto, mejor descartarlo que mandar un 400. */
                    continue;
                }
            }
        }
        precedido_por_tool_calls = mensaje.role == "assistant"
            && mensaje
                .tool_calls
                .as_ref()
                .is_some_and(|tc| !tc.is_empty());
        if precedido_por_tool_calls {
            id_tool_call_previo = mensaje
                .tool_calls
                .as_ref()
                .and_then(|tc| tc.first())
                .map(|call| call.id.clone())
                .filter(|id| !id.trim().is_empty());
        }
        sanitizados.push(mensaje);
    }

    if sanitizados.is_empty() {
        return Err(Error::Validacion(
            "No hay mensajes válidos para enviar a la IA".into(),
        ));
    }
    Ok(sanitizados)
}

/// Candidatos a probar: el solicitado (si el modelo es válido para el
/// proveedor) primero, luego la cadena de fallback, sin duplicados.
fn resolver_candidatos(provider: &str, modelo: &str) -> Vec<(&'static str, &'static str)> {
    let mut candidatos: Vec<(&'static str, &'static str)> = Vec::new();
    if let Some((proveedor, modelo)) = candidato_valido(provider, modelo) {
        candidatos.push((proveedor, modelo));
    }
    for (proveedor, modelo) in CHAT_FALLBACK_CHAIN {
        if !candidatos
            .iter()
            .any(|(p, m)| *p == *proveedor && *m == *modelo)
        {
            candidatos.push((*proveedor, *modelo));
        }
    }
    candidatos
}

/// Devuelve el candidato solicitado solo si el proveedor y el modelo pasan la
/// allowlist. Si la configuración del front es inválida, se usa la cadena.
fn candidato_valido(provider: &str, modelo: &str) -> Option<(&'static str, &'static str)> {
    let proveedor = provider.trim().to_ascii_lowercase();
    let (id, _, modelos) = PROVIDERS.iter().find(|(id, _, _)| *id == proveedor)?;
    let modelo_trim = modelo.trim();
    if modelo_trim.is_empty() || !modelos.iter().copied().any(|m| m == modelo_trim) {
        return None;
    }
    /* El modelo validado es estático (allowlist), así que se devuelve el
     * &'static str del slice original. */
    let modelo_estatico = modelos.iter().copied().find(|m| *m == modelo_trim)?;
    Some((*id, modelo_estatico))
}

/// Función pura expuesta para pruebas de paridad con el servicio original de
/// task: el consumidor puede comparar los candidatos resueltos.
#[must_use]
pub fn candidatos_para(provider: &str, modelo: &str) -> Vec<(&'static str, &'static str)> {
    resolver_candidatos(provider, modelo)
}

fn mayuscula_primera(texto: &str) -> String {
    let mut chars = texto.chars();
    match chars.next() {
        Some(primera) => primera.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
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

/* Convierte las tool_calls crudas del SSE a la estructura tipada del dominio.
 * Función pura extraída del método stream para acortarlo (funcion-larga-rs).
 * [318A-10 02-09-2026] Sanidad defensiva: Laguna S 2.1 free (commandcode)
 * devuelve tool_calls en streaming con `id` y `function.name` VACÍOS. Antes
 * eso llegaba al runtime como AiToolCall{id:"", nombre:"", ...} → la tool
 * fallaba con "Tool desconocida: " y, al reenviar el par assistant/tool, el
 * `tool_call_id` vacío hacía que el proveedor respondiera 400 "Tool message
 * must have tool_call_id". Ahora:
 * - si `function.name` falta o es vacío → se descarta la tool_call (malformada);
 * - si `id` falta o es vacío → se sintetiza uno estable para que el par
 *   assistant(tool_calls)/tool conserve un tool_call_id válido. */
fn parsear_tool_calls(tool_calls: Vec<serde_json::Value>) -> Vec<AiToolCall> {
    tool_calls
        .into_iter()
        .filter_map(|call| {
            let nombre = call
                .pointer("/function/name")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|nombre| !nombre.is_empty())
                .map(str::to_owned)?;
            let id = call
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(str::to_owned)
                .unwrap_or_else(|| format!("call_{nombre}_{:x}", rand_fallback()));
            let argumentos: serde_json::Value = call
                .pointer("/function/arguments")
                .and_then(serde_json::Value::as_str)
                .and_then(|args| serde_json::from_str(args).ok())
                .unwrap_or_else(|| serde_json::json!({}));
            Some(AiToolCall { id, nombre, argumentos })
        })
        .collect()
}

/// Fuente de entropía para sintetizar `tool_call_id` (sin dependencia nueva):
/// mezcla un contador volátil con el reloj. Suficiente para un id estable
/// dentro del turno; el proveedor solo exige que NO sea vacío y que el par
/// assistant/tool lo repita.
fn rand_fallback() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0);
    (nanos as u64) ^ (nanos as u64).rotate_left(17)
}

#[cfg(test)]
mod tests {
    use super::*;

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