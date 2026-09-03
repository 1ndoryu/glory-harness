/* [29-08-2026] Manejo de contexto del agente (plan-agente-ia-plugin, Fase 0).
 * Autocompactación por ocupación de ventana (estilo Hermes/opencode):
 * - Disparo al umbral configurado (default 50% de la ventana efectiva; piso
 *   75% en ventanas < 512K; 85% degenerado = compactación forzada).
 * - Cola reciente verbatim (~2.5% de la ventana, clamp [10K, 25K] tokens),
 *   alineada a límites de turno (nunca cortar un turno a la mitad).
 * - Head protegido (system + memoria) no se toca.
 * - Anti-thrash: si las 2 últimas compactaciones ahorraron < 10%, no compactar.
 * - La compactación NUNCA borra: marca `compactado` en BD y el historial
 *   completo sigue recuperable (sección 5.2.1 del plan). */

use crate::llm::AiMessage;
use serde::{Deserialize, Serialize};

/// Marcadores de capas del system prompt (318A-15 F1). El runtime ensambla el
/// prompt con `[ENTORNO]` (dinámico: fecha/workspace/git/modelo, recién
/// inyectado cada turno) y, si el consumidor aporta reglas, `[REGLAS]` (ranura
/// AGENTS.md/skills; vacía por defecto, sin encabezado huérfano). La
/// compactación trata como head protegido cualquier mensaje system que
/// contenga estos marcadores: nunca se resume ni se pierde.
pub const MARCA_ENTORNO: &str = "[ENTORNO]";
pub const CIERRE_ENTORNO: &str = "[/ENTORNO]";
pub const MARCA_REGLAS: &str = "[REGLAS]";
pub const CIERRE_REGLAS: &str = "[/REGLAS]";

/// ¿El mensaje es un system prompt por capas (lleva marcadores [ENTORNO] o
/// [REGLAS])? Usado por la compactación para protegerlo y por los tests.
#[must_use]
pub fn es_prompt_con_marcadores(mensaje: &AiMessage) -> bool {
    match &mensaje.content {
        serde_json::Value::String(texto) => {
            texto.contains(MARCA_ENTORNO) || texto.contains(MARCA_REGLAS)
        }
        _ => false,
    }
}

/// Estimación de tokens: chars/4 (aproximación estándar para texto mixto).
/// Suficiente para el presupuesto de v1; documentado como heurística.
#[must_use]
pub fn estimar_tokens(texto: &str) -> u32 {
    (texto.chars().count() as u32).div_ceil(4)
}

/// Configuración de la ventana de contexto y autocompactación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextoConfig {
    /// Tope duro de tokens de la ventana usada (ventana del modelo).
    pub max_ventana: u32,
    /// Reserva de salida (max_output_tokens del proveedor; 20K por defecto).
    pub reserva_salida: u32,
    /// Umbral de disparo como fracción de la ventana efectiva (0.5 por defecto).
    pub umbral: f32,
    /// Fracción de cola reciente verbatim (0.025 por defecto, estilo Hermes).
    pub cola_verbatim: f32,
    /// Piso de umbral para ventanas pequeñas (< 512K): 0.75.
    pub umbral_piso: f32,
    /// Umbral degenerado de compactación forzada: 0.85.
    pub umbral_degenerado: f32,
    /// [318A-15 F6] Umbral de disparo configurable por consumidor
    /// (`pct_compactar`, default 0.80): el disparo efectivo es
    /// `max(umbral_efectivo, pct_compactar)`. Propuesta del plan §8.4;
    /// ajustable con la telemetría de F0.
    #[serde(default = "default_pct_compactar")]
    pub pct_compactar: f32,
    /// [318A-15 F6] Fracción del techo reservada como ventana de seguridad
    /// (default 0.15): con `en_ejecucion_tool` activo la compactación se
    /// omite (no compactar durante un tool_call largo).
    #[serde(default = "default_ventana_seguridad")]
    pub ventana_seguridad: f32,
    /// [318A-15 F6] Variante A (LLM, mismo proveedor del turno): el runtime
    /// pide el resumen dirigido al proveedor y cae al fallback B determinista
    /// si falla o devuelve vacío. `false` (default) = fallback B siempre
    /// (los tests nunca requieren proveedor).
    #[serde(default)]
    pub resumir_con_llm: bool,
    /// [318A-15 F6] Tope de tokens de salida del resumen LLM (variante A).
    #[serde(default = "default_max_resumen_tokens")]
    pub max_resumen_tokens: u32,
}

impl Default for ContextoConfig {
    fn default() -> Self {
        Self {
            max_ventana: 128_000,
            reserva_salida: 20_000,
            umbral: 0.5,
            cola_verbatim: 0.025,
            umbral_piso: 0.75,
            umbral_degenerado: 0.85,
            pct_compactar: 0.80,
            ventana_seguridad: 0.15,
            resumir_con_llm: false,
            max_resumen_tokens: 1_000,
        }
    }
}

fn default_pct_compactar() -> f32 {
    0.80
}

fn default_ventana_seguridad() -> f32 {
    0.15
}

fn default_max_resumen_tokens() -> u32 {
    1_000
}

impl ContextoConfig {
    /// Ventana efectiva = ventana − reserva de salida (nunca por debajo de 1K).
    #[must_use]
    pub fn ventana_efectiva(&self) -> u32 {
        self.max_ventana.saturating_sub(self.reserva_salida).max(1_000)
    }

    /// Umbral efectivo: si la ventana < 512K se usa el piso (compactar antes),
    /// salvo que el usuario lo haya configurado explícitamente más alto.
    #[must_use]
    pub fn umbral_efectivo(&self) -> f32 {
        if self.max_ventana < 512_000 {
            self.umbral.max(self.umbral_piso)
        } else {
            self.umbral
        }
    }

    /// [318A-15 F6] Umbral de disparo efectivo: el consumidor configura
    /// `pct_compactar`; nunca se compacta por debajo del piso de ventanas
    /// pequeñas (`umbral_efectivo`).
    #[must_use]
    pub fn umbral_disparo(&self) -> f32 {
        self.umbral_efectivo().max(self.pct_compactar)
    }
}

/// Métricas de una compactación (evento `usage` del contrato SSE).
#[derive(Debug, Clone, Serialize)]
pub struct CompactionMetrics {
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub savings_pct: f32,
    pub occupancy_pct: f32,
    pub cola_verbatim_tokens: u32,
    /// [318A-15 F6] Tamaño en tokens del resumen del tramo (observabilidad).
    pub resumen_tokens: u32,
    /// [318A-15 F6] Tramos compactados acumulados (nº de compactaciones).
    pub tramos: u32,
}

/// Resultado de evaluar/compactar un historial.
pub struct CompactarResultado {
    pub mensajes: Vec<AiMessage>,
    pub compactado: bool,
    pub metricas: Option<CompactionMetrics>,
    pub tokens_estimados: u32,
}

pub struct AgentContextManager {
    config: ContextoConfig,
    /// Historial de ahorros de las últimas compactaciones (anti-thrash).
    ahorros_recientes: Vec<f32>,
    /// [318A-15 F0] Nº de compactaciones automáticas realizadas desde que el
    /// gestor existe (per-conversación cuando el runtime vive por
    /// conversación). Solo observa; no cambia la lógica de compactación.
    compactaciones: u32,
    /// [318A-15 F6] Longitud del historial en la última compactación:
    /// sin mensajes nuevos no se vuelve a compactar (item 6 del plan: no
    /// compactar dos veces seguidas con el mismo material).
    ultima_compactacion_len: usize,
}

impl AgentContextManager {
    #[must_use]
    pub fn new(config: ContextoConfig) -> Self {
        Self {
            config,
            ahorros_recientes: Vec::new(),
            compactaciones: 0,
            ultima_compactacion_len: 0,
        }
    }

    /// [318A-15 F0] Compactaciones acumuladas del gestor (telemetría).
    #[must_use]
    pub fn compactaciones(&self) -> u32 {
        self.compactaciones
    }

    #[must_use]
    pub fn config(&self) -> &ContextoConfig {
        &self.config
    }

    /// Evalúa el historial y compacta si la ocupación supera el umbral.
    /// `indice_system` = índice del mensaje system (head protegido); los turnos
    /// anteriores a `indice_primer_turno_importante` son los candidatos a resumir.
    /// Equivale a `preparar_con(..., None, false)`: fallback determinista y sin
    /// ventana de seguridad (comportamiento histórico, todos los tests previos).
    pub fn preparar(
        &mut self,
        mensajes: &[AiMessage],
        indice_system: usize,
    ) -> CompactarResultado {
        self.preparar_con(mensajes, indice_system, None, false)
    }

    /// [318A-15 F6] `preparar` con las opciones de compactación dirigida:
    /// - `resumen_llm`: variante A (resumen del proveedor, misma plantilla
    ///   dirigida); `None` o vacío = fallback B determinista (instrucciones /
    ///   preferencias verbatim + último intercambio). Los tests nunca requieren
    ///   proveedor.
    /// - `en_ejecucion_tool`: con la ventana de seguridad configurada, no se
    ///   compacta mientras una tool está en curso (solo el umbral degenerado).
    pub fn preparar_con(
        &mut self,
        mensajes: &[AiMessage],
        indice_system: usize,
        resumen_llm: Option<String>,
        en_ejecucion_tool: bool,
    ) -> CompactarResultado {
        let tokens_total: u32 = mensajes.iter().map(tokens_de_mensaje).sum();
        let ventana_efectiva = self.config.ventana_efectiva();
        let occupancy = tokens_total as f32 / ventana_efectiva as f32;
        let umbral = self.config.umbral_disparo();

        /* [318A-15 F6] Ventana de seguridad: con una tool en curso (tool_call
         * largo o sesión hija) no se compacta salvo ocupación degenerada. */
        let en_tool = en_ejecucion_tool && occupancy < umbral_degenerado(&self.config);
        /* [318A-15 F6] No compactar dos veces seguidas con el mismo material:
         * sin mensajes nuevos desde la última compactación no hay nada que
         * ganar y solo se pierde fidelidad. */
        let sin_material_nuevo = mensajes.len() == self.ultima_compactacion_len;

        let debe_compactar = !en_tool
            && !sin_material_nuevo
            && (occupancy >= umbral_degenerado(&self.config)
                || (occupancy >= umbral && !self.anti_thrash_activo()));
        if !debe_compactar || mensajes.len() <= indice_system + 2 {
            return CompactarResultado {
                mensajes: mensajes.to_vec(),
                compactado: false,
                metricas: None,
                tokens_estimados: tokens_total,
            };
        }

        let (nuevos, cola_tokens, resumen_texto) =
            self.compactar(mensajes, indice_system, resumen_llm);
        let tokens_after: u32 = nuevos.iter().map(tokens_de_mensaje).sum();
        let ahorro = (tokens_total.saturating_sub(tokens_after)) as f32 / tokens_total.max(1) as f32;
        self.ahorros_recientes.push(ahorro);
        if self.ahorros_recientes.len() > 2 {
            self.ahorros_recientes.remove(0);
        }
        self.compactaciones += 1;
        self.ultima_compactacion_len = mensajes.len();

        CompactarResultado {
            mensajes: nuevos,
            compactado: true,
            metricas: Some(CompactionMetrics {
                tokens_before: tokens_total,
                tokens_after,
                savings_pct: ahorro * 100.0,
                occupancy_pct: occupancy * 100.0,
                cola_verbatim_tokens: cola_tokens,
                /* [318A-15 F6] Observabilidad (item 5): nº de tramos
                 * compactados acumulados y tamaño del resumen del tramo. */
                resumen_tokens: crate::context::estimar_tokens(&resumen_texto),
                tramos: self.compactaciones,
            }),
            tokens_estimados: tokens_after,
        }
    }

    /// Anti-thrash: si las 2 últimas compactaciones ahorraron < 10%, no
    /// compactar en el umbral normal (esperar al degenerado). Estilo Hermes.
    fn anti_thrash_activo(&self) -> bool {
        self.ahorros_recientes.len() >= 2
            && self
                .ahorros_recientes
                .iter()
                .rev()
                .take(2)
                .all(|ahorro| *ahorro < 0.10)
    }

    /// Compacta: head protegido + [resumen dirigido fechado] + cola verbatim.
    /// Devuelve también el texto del resumen para las métricas de F6.
    fn compactar(
        &self,
        mensajes: &[AiMessage],
        indice_system: usize,
        resumen_llm: Option<String>,
    ) -> (Vec<AiMessage>, u32, String) {
        let ventana_efectiva = self.config.ventana_efectiva();
        // Cola verbatim: 2.5% de la ventana, clamp [10K, 25K].
        let presupuesto_cola = ((ventana_efectiva as f32 * self.config.cola_verbatim) as u32)
            .clamp(10_000, 25_000);

        // Separar: head (system + siguientes) / medio (a resumir) / cola.
        let mut head: Vec<AiMessage> = Vec::new();
        let mut medio: Vec<AiMessage> = Vec::new();
        let mut cola: Vec<AiMessage> = Vec::new();

        /* [318A-15 F1] Head protegido: además del system del índice dado, todo
         * mensaje system con marcadores [ENTORNO]/[REGLAS] se conserva verbatim
         * (nunca se resume ni cae al medio). El runtime lo reinyecta fresco en
         * cada turno, pero si un consumidor persistió uno anterior, tampoco se
         * pierde ni se corrompe con el resumen. */
        for (i, m) in mensajes.iter().enumerate() {
            if i <= indice_system || (m.role == "system" && es_prompt_con_marcadores(m)) {
                head.push(m.clone());
            } else {
                medio.push(m.clone());
            }
        }

        // Construir la cola desde el final hasta llenar el presupuesto,
        // alineada a límites de turno: un "turno" = par (user, assistant|tool).
        let mut cola_tokens = 0u32;
        for m in medio.iter().rev() {
            let t = tokens_de_mensaje(m);
            // Nunca cortar un turno a la mitad: si sumar este mensaje supera el
            // presupuesto, detenerse (el turno completo se queda en el medio).
            if cola_tokens + t > presupuesto_cola && !cola.is_empty() {
                break;
            }
            cola_tokens += t;
            cola.push(m.clone());
        }
        cola.reverse();
        // Quitar de `medio` lo que pasó a la cola.
        let corte = medio.len() - cola.len();
        medio.truncate(corte);

        /* [318A-15 F6] Tramo fechado: el resumen del medio lleva la fecha de la
         * compactación (cada tramo deja su propio resumen system; nunca "todo
         * lo anterior" sin referencia temporal). Si el consumidor aportó un
         * resumen LLM (variante A) se usa; si no o si llegó vacío, fallback B
         * determinista — los tests nunca dependen del proveedor. */
        let resumen = if medio.is_empty() {
            String::new()
        } else {
            resumen_dirigido(&medio, &crate::runtime::fecha_hoy(), resumen_llm)
        };

        let mut resultado = head;
        if !resumen.is_empty() {
            resultado.push(AiMessage::texto("system", resumen.clone()));
        }
        // Mensaje de continuación (estilo opencode): no romper el formato.
        if !cola.is_empty() {
            resultado.push(AiMessage::texto(
                "user",
                "[CONTEXT COMPACTION — REFERENCE ONLY]\nContinúo desde el resumen anterior.",
            ));
            resultado.extend(cola);
        }
        (resultado, cola_tokens, resumen)
    }
}

fn umbral_degenerado(config: &ContextoConfig) -> f32 {
    config.umbral_degenerado
}

#[must_use]
pub fn tokens_de_mensaje(mensaje: &AiMessage) -> u32 {
    match &mensaje.content {
        serde_json::Value::String(texto) => estimar_tokens(texto),
        serde_json::Value::Array(items) => {
            items
                .iter()
                .map(|item| {
                    item.get("text")
                        .and_then(serde_json::Value::as_str)
                        .map_or(0, estimar_tokens)
                })
                .sum()
        }
        _ => 0,
    }
}

/// [318A-15 F6] Plantilla del resumen dirigido (variante A): secciones fijas
/// `[DECISIONES]`/`[PENDIENTES]`/`[PREFERENCIAS]`/`[RESTRICCIONES]` y consigna
/// anti-alucinación ("no añadas nada que no esté en la conversación"). Es el
/// prompt que recibe el proveedor en la variante A y el molde en el que el
/// consumidor vuelca su resumen; el fallback B no la usa porque su contenido
/// es verbatim de la conversación (no puede inventar).
#[must_use]
pub fn plantilla_resumen_dirigido() -> String {
    "Resume la conversación anterior SOLO con lo que aparece en ella. No añadas\
 nada que no esté en la conversación (ni hechos, ni intenciones, ni preferencias\
 inventadas). Rellena cada sección con lo que exista y deja vacía la que no\
 aplique:\n[DECISIONES]\n[PENDIENTES]\n[PREFERENCIAS]\n[RESTRICCIONES]"
        .to_string()
}

/// [318A-15 F6] Resumen dirigido de un tramo de conversación, fechado.
/// - `resumen_llm` con contenido → variante A: se enmarca con la plantilla
///   dirigida y la fecha del tramo.
/// - `None` o vacío (el LLM falló) → fallback B determinista: instrucciones y
///   preferencias verbatim + último intercambio verbatim.
#[must_use]
pub fn resumen_dirigido(mensajes: &[AiMessage], fecha: &str, resumen_llm: Option<String>) -> String {
    if let Some(llm) = resumen_llm {
        let llm = llm.trim();
        if !llm.is_empty() {
            return format!(
                "## RESUMEN {fecha} (dirigido)\n{}\n{llm}\n--- END OF CONTEXT SUMMARY ---",
                plantilla_resumen_dirigido()
            );
        }
    }
    fallback_determinista(mensajes, fecha)
}

/// [318A-15 F6] Fallback B determinista: conserva instrucciones/preferencias
/// verbatim (mensajes de usuario del tramo) + el último intercambio verbatim.
/// Determinista por construcción: sin LLM no puede alucinar.
fn fallback_determinista(mensajes: &[AiMessage], fecha: &str) -> String {
    let texto = |m: &AiMessage| match &m.content {
        serde_json::Value::String(t) => t.clone(),
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(serde_json::Value::as_str))
            .collect::<Vec<_>>()
            .join(" "),
        _ => String::new(),
    };
    let recortar = |s: &str| -> String { s.chars().take(800).collect() };

    let instrucciones: Vec<String> = mensajes
        .iter()
        .filter(|m| m.role == "user")
        .filter_map(|m| {
            let t = texto(m);
            if t.trim().is_empty() {
                None
            } else {
                Some(format!("- {}", recortar(&t)))
            }
        })
        .collect();
    let ultimo_user = mensajes.iter().rev().find(|m| m.role == "user").map(texto);
    let ultimo_assistant = mensajes
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(texto);

    let mut cuerpo = if instrucciones.is_empty() {
        "(sin instrucciones de usuario en el tramo)\n".to_string()
    } else {
        format!("{}\n", instrucciones.join("\n"))
    };
    if let (Some(u), Some(a)) = (ultimo_user, ultimo_assistant) {
        cuerpo.push_str(&format!(
            "Último intercambio (verbatim):\n[user] {}\n[assistant] {}",
            recortar(u.trim()),
            recortar(a.trim())
        ));
    }
    format!(
        "## RESUMEN {fecha} — fallback determinista (sin LLM)\n{cuerpo}--- END OF CONTEXT SUMMARY ---"
    )
}

/// Genera el bloque de resumen estructurado del medio compactado.
/// [318A-7] `pub(crate)` para reutilizarla en el endpoint de compactación manual
/// (el resumen que se guarda en BD al marcar mensajes como compactados).
pub fn resumen_de_mensajes(mensajes: &[AiMessage]) -> String {
    let mut partes: Vec<String> = Vec::new();
    for m in mensajes {
        let texto = match &m.content {
            serde_json::Value::String(t) => t.clone(),
            serde_json::Value::Array(items) => items
                .iter()
                .filter_map(|i| i.get("text").and_then(serde_json::Value::as_str))
                .collect::<Vec<_>>()
                .join(" "),
            _ => String::new(),
        };
        if texto.trim().is_empty() {
            continue;
        }
        // Recortar cada mensaje a 400 chars para que el resumen no reviente.
        let recortado: String = texto.chars().take(400).collect();
        partes.push(format!("[{}] {}", m.role, recortado));
    }
    let cuerpo = if partes.is_empty() {
        "Historial anterior sin contenido relevante.".to_string()
    } else {
        partes.join("\n")
    };
    format!(
        "## RESUMEN DE LA CONVERSACIÓN ANTERIOR\n{cuerpo}\n--- END OF CONTEXT SUMMARY ---"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mensaje(rol: &str, texto: &str) -> AiMessage {
        AiMessage::texto(rol, texto)
    }

    fn historial_largo(n_turnos: usize) -> Vec<AiMessage> {
        let mut msgs = vec![mensaje("system", "Eres un asistente.")];
        for i in 0..n_turnos {
            msgs.push(mensaje("user", &format!("Pregunta {i}: {}", "x".repeat(400))));
            msgs.push(mensaje("assistant", &format!("Respuesta {i}: {}", "y".repeat(400))));
        }
        msgs
    }

    #[test]
    fn no_compacta_bajo_el_umbral() {
        let mut cm = AgentContextManager::new(ContextoConfig::default());
        let msgs = historial_largo(5);
        let r = cm.preparar(&msgs, 0);
        assert!(!r.compactado);
        assert_eq!(r.mensajes.len(), msgs.len());
    }

    #[test]
    fn compacta_al_superar_el_umbral_y_conserva_cola() {
        /* Ventana pequeña: 200 turnos ≈ 40K tokens superan el piso 75% de la
         * ventana efectiva (14K de 18K), así que compacta. */
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let msgs = historial_largo(200);
        let r = cm.preparar(&msgs, 0);
        assert!(r.compactado, "debería compactar con 200 turnos");
        let metricas = r.metricas.as_ref().expect("métricas presentes");
        assert!(metricas.tokens_after < metricas.tokens_before);
        assert!(metricas.savings_pct > 0.0);
        // Head (system) protegido + resumen + mensaje de continuación + cola.
        assert_eq!(r.mensajes[0].role, "system");
        assert!(r.mensajes.len() < msgs.len());
        // La cola verbatim conserva el último user verbatim.
        let ultimo = r.mensajes.last().expect("cola no vacía");
        assert_eq!(ultimo.role, "assistant");
    }

    #[test]
    fn anti_thrash_evita_compactar_sin_ahorro() {
        let mut cm = AgentContextManager::new(ContextoConfig {
            umbral: 0.0, // compactar siempre
            ..ContextoConfig::default()
        });
        // Mensajes que no ahorran nada (ya compactados).
        let msgs = vec![mensaje("system", "s"), mensaje("user", "u"), mensaje("assistant", "a")];
        let _ = cm.preparar(&msgs, 0);
        let _ = cm.preparar(&msgs, 0);
        // Tercera: el ahorro es 0 (<10%) → anti-thrash activo; como msgs es
        // pequeño (len <= system+2), no compacta por tamaño de todas formas.
        let r = cm.preparar(&msgs, 0);
        assert!(!r.compactado);
    }

    #[test]
    fn nunca_corta_un_turno_a_la_mitad() {
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            cola_verbatim: 0.01, // presupuesto cola = 180, menor que un turno
            umbral: 0.0,
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let msgs = historial_largo(10);
        let r = cm.preparar(&msgs, 0);
        if r.compactado {
            // El último mensaje de la cola debe ser un assistant (par completo).
            let ultimo = r.mensajes.last().expect("cola");
            assert_eq!(ultimo.role, "assistant");
        }
    }

    /* [318A-15 F1] Un mensaje system con marcadores [ENTORNO]/[REGLAS] es head
     * protegido aunque no esté en el índice del system (p. ej. un prompt de un
     * turno anterior persistido por el consumidor): nunca se resume ni se pierde. */

    #[test]
    fn system_con_marcadores_se_protege_de_la_compactacion() {
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            cola_verbatim: 0.005,
            umbral: 0.0, // compactar siempre que haya material que resumir
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let mut msgs = vec![mensaje("system", "Eres un asistente.")];
        /* Suficientes turnos para superar el piso 75% de la ventana efectiva
         * (18K de 18K en ventanas < 512K, ver umbral_efectivo). */
        for i in 0..200 {
            msgs.push(mensaje("user", &format!("Pregunta {i}: {}", "x".repeat(400))));
            msgs.push(mensaje("assistant", &format!("Respuesta {i}: {}", "y".repeat(400))));
        }
        /* Un system con entorno (como el que el runtime ensambla cada turno)
         * colocado después de los turnos, con marca y contenido único. */
        let entorno = format!("{MARCA_ENTORNO}\nFecha: 2026-09-03\nWorkspace: C:/ruta/única\n{CIERRE_ENTORNO}");
        msgs.push(mensaje("system", &entorno));
        msgs.push(mensaje("user", "Pregunta final"));

        let r = cm.preparar(&msgs, 0);
        assert!(r.compactado, "debe compactar el medio");
        assert!(r.mensajes.len() < msgs.len(), "el medio se resumió");
        /* El system con marcadores sobrevive verbatim (nunca al resumen). */
        let sobrevive = r.mensajes.iter().any(|m| {
            m.role == "system"
                && matches!(&m.content, serde_json::Value::String(s) if s.contains("C:/ruta/única"))
        });
        assert!(sobrevive, "el entorno marcado se conserva verbatim");
        /* Y la marca no aparece embebida en el resumen del medio. */
        let resumen = r
            .mensajes
            .iter()
            .find(|m| matches!(&m.content, serde_json::Value::String(s) if s.starts_with("## RESUMEN")))
            .map(|m| match &m.content {
                serde_json::Value::String(s) => s.clone(),
                _ => String::new(),
            });
        if let Some(resumen) = resumen {
            assert!(!resumen.contains(MARCA_ENTORNO), "el resumen no duplica el entorno");
        }
    }

    /* ===== [318A-15 F6] Compactación dirigida ===== */

    #[test]
    fn f6_umbral_disparo_configurable_por_consumidor() {
        /* Ventana grande (>= 512K): sin piso, el disparo es max(umbral, pct). */
        let grande = ContextoConfig {
            max_ventana: 512_000,
            umbral: 0.5,
            ..ContextoConfig::default()
        };
        assert_eq!(grande.umbral_disparo(), 0.8, "pct_compactar default 0.80");
        let con_override = ContextoConfig {
            max_ventana: 512_000,
            umbral: 0.5,
            pct_compactar: 0.6,
            ..ContextoConfig::default()
        };
        assert_eq!(con_override.umbral_disparo(), 0.6);
        /* Ventana pequeña: el piso (0.75) nunca se rebaja con el pct. */
        let pequena = ContextoConfig {
            max_ventana: 20_000,
            pct_compactar: 0.5,
            ..ContextoConfig::default()
        };
        assert_eq!(pequena.umbral_disparo(), 0.75);
    }

    #[test]
    fn f6_ventana_seguridad_omite_compactar_durante_tool() {
        /* 70 turnos ≈ 0.803 de la ventana efectiva (18K): entre el disparo
         * (0.80) y el degenerado (0.85). Con tool en curso no se compacta; sin
         * ella sí. */
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            umbral: 0.0,
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let msgs = historial_largo(70);
        let en_tool = cm.preparar_con(&msgs, 0, None, true);
        assert!(!en_tool.compactado, "ventana de seguridad: no compactar en tool_call");
        let sin_tool = cm.preparar_con(&msgs, 0, None, false);
        assert!(sin_tool.compactado, "sin tool en curso sí compacta");
    }

    #[test]
    fn f6_tramos_fechados_y_no_recompacta_sin_material_nuevo() {
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let msgs = historial_largo(200);

        let r1 = cm.preparar_con(&msgs, 0, None, false);
        assert!(r1.compactado);
        /* El resumen del tramo es un system fechado con el fallback B. */
        let resumen = r1
            .mensajes
            .iter()
            .find(|m| matches!(&m.content, serde_json::Value::String(s) if s.starts_with("## RESUMEN")))
            .expect("resumen fechado presente");
        let texto = match &resumen.content {
            serde_json::Value::String(s) => s.clone(),
            _ => String::new(),
        };
        assert!(texto.contains(&crate::runtime::fecha_hoy()), "tramo fechado");
        assert!(texto.contains("fallback determinista"));
        assert_eq!(r1.metricas.as_ref().expect("métricas").tramos, 1);
        assert!(r1.metricas.as_ref().expect("métricas").resumen_tokens > 0);

        /* La misma entrada no se compacta dos veces seguidas. */
        let r2 = cm.preparar_con(&msgs, 0, None, false);
        assert!(!r2.compactado, "sin mensajes nuevos no se recompacta");

        /* Material nuevo → segundo tramo. */
        let mut msgs2 = msgs;
        msgs2.push(mensaje("user", "Pregunta extra"));
        msgs2.push(mensaje("assistant", "Respuesta extra"));
        let r3 = cm.preparar_con(&msgs2, 0, None, false);
        assert!(r3.compactado, "material nuevo sí compacta");
        assert_eq!(r3.metricas.as_ref().expect("métricas").tramos, 2);
    }

    #[test]
    fn f6_fallback_determinista_si_el_llm_falla() {
        let msgs = [
            mensaje("user", "Decisión: usar PostgreSQL"),
            mensaje("assistant", "Perfecto, anotado."),
        ];
        let con_vacio = resumen_dirigido(&msgs, "2026-09-03", Some(String::new()));
        let sin_llm = resumen_dirigido(&msgs, "2026-09-03", None);
        assert_eq!(con_vacio, sin_llm, "resumen LLM vacío cae al fallback B");
        assert!(sin_llm.contains("Decisión: usar PostgreSQL"), "instrucción verbatim");
        assert!(sin_llm.contains("Último intercambio"), "último intercambio verbatim");
        assert!(sin_llm.contains("2026-09-03"), "tramo fechado");
    }

    #[test]
    fn f6_variante_a_enmarca_el_resumen_llm_con_plantilla() {
        let msgs = [mensaje("user", "Instrucción: usar PostgreSQL")];
        let v = resumen_dirigido(
            &msgs,
            "2026-09-03",
            Some("[DECISIONES]\n- usar MySQL".to_string()),
        );
        for seccion in ["[DECISIONES]", "[PENDIENTES]", "[PREFERENCIAS]", "[RESTRICCIONES]"] {
            assert!(v.contains(seccion), "plantilla con {seccion}");
        }
        assert!(v.contains("no esté en la conversación"), "consigna anti-alucinación");
        assert!(v.contains("usar MySQL"), "el resumen LLM viaja");
        assert!(!v.contains("Instrucción:"), "el cuerpo verbatim no se cuela");
    }

    #[test]
    fn f6_e2e_fixture_decision_solo_en_resumen() {
        /* Conversación larga con una decisión en el medio: tras compactar, la
         * decisión sobrevive únicamente a través del resumen (fallback B la
         * conserva verbatim) y el último intercambio queda verbatim en la cola
         * — el "modelo refiere una decisión que solo está en el resumen". */
        let config = ContextoConfig {
            max_ventana: 20_000,
            reserva_salida: 2_000,
            ..ContextoConfig::default()
        };
        let mut cm = AgentContextManager::new(config);
        let mut msgs = vec![mensaje("system", "Eres un asistente.")];
        for i in 0..150 {
            msgs.push(mensaje("user", &format!("Pregunta {i}: {}", "x".repeat(400))));
            msgs.push(mensaje("assistant", &format!("Respuesta {i}: {}", "y".repeat(400))));
        }
        msgs.push(mensaje("user", "Decisión: usar PostgreSQL en el proyecto"));
        msgs.push(mensaje("assistant", "Perfecto, quedará registrado."));
        for i in 151..200 {
            msgs.push(mensaje("user", &format!("Pregunta {i}: {}", "x".repeat(400))));
            msgs.push(mensaje("assistant", &format!("Respuesta {i}: {}", "y".repeat(400))));
        }

        let r = cm.preparar_con(&msgs, 0, None, false);
        assert!(r.compactado);
        assert_eq!(r.mensajes[0].role, "system", "head protegido intacto");
        let todo = r
            .mensajes
            .iter()
            .map(|m| match &m.content {
                serde_json::Value::String(s) => s.clone(),
                _ => String::new(),
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(todo.contains("usar PostgreSQL"), "la decisión sobrevive en el resumen");
        assert!(
            r.mensajes.last().is_some_and(|m| m.role == "assistant"),
            "la cola termina en un par completo"
        );
    }
}
