//! [059A-N S2] Split mecánico de `llm.rs`: catálogo de proveedores, tipos del contrato, validación de mensajes y resolución de candidatos. Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::*;

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
pub(crate) fn url_proveedor(proveedor: &str) -> String {
    if proveedor == "glory" {
        return std::env::var("GLORY_API_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:3101/v1/chat/completions".to_string());
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
 * stealth/ox-alpha) se pasan tal cual a gloryapi: ya son IDs del catálogo.
 * [069A-7 06-09-2026] `auto` ya NO se mapea aquí: pasa literal como "auto"
 * a glory API para que su router decida el modelo real. */
pub(crate) fn modelo_proveedor(proveedor: &str, modelo: &str) -> String {
    if proveedor == "glory" {
        match modelo {
            "commandcode" | "glm-5.3-flash" => "deepseek/deepseek-v4-flash".to_string(),
            otro => otro.to_string(),
        }
    } else {
        modelo.to_string()
    }
}

/// Prompt de nutrición calibrado regional (mismo que el front para que el
/// admin reciba el mismo comportamiento que un usuario con key propia).
pub(crate) const PROMPT_NUTRICION: &str = "You are a certified nutritionist estimating macros for a home-cooked Latin American diet.
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

/// Valida roles y contenido, recorta a los últimos 25 mensajes y limita el
/// contenido de texto a 12000 caracteres (paridad con PHP validarMensajes()).
pub(crate) fn validar_mensajes(mensajes: Vec<AiMessage>) -> Result<Vec<AiMessage>, Error> {
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
                && mensaje.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty())
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
            && mensaje.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty());
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
pub(crate) fn resolver_candidatos(
    provider: &str,
    modelo: &str,
) -> Vec<(&'static str, &'static str)> {
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

pub(crate) fn mayuscula_primera(texto: &str) -> String {
    let mut chars = texto.chars();
    match chars.next() {
        Some(primera) => primera.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/* [059A-S3] Relocalización desde `red.rs` (límite de 500 ef del archivo):
 * el parseo de tool_calls y la clasificación de errores transitorios son lógica
 * pura sobre el contrato; el transporte HTTP queda en `red.rs`. Movimiento fiel,
 * sin cambios de lógica. */
/// ¿El error del proveedor es transitorio (reintentar tiene sentido)?
/// `Error::Proveedor` con prefijo de red (reqwest send/read) o con un código
/// HTTP 5xx/429; el resto (4xx de auth/billing/schema) es permanente.
pub(crate) fn es_error_transitorio(error: &Error) -> bool {
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
pub(crate) fn parsear_tool_calls(tool_calls: Vec<serde_json::Value>) -> Vec<AiToolCall> {
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
            Some(AiToolCall {
                id,
                nombre,
                argumentos,
            })
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
