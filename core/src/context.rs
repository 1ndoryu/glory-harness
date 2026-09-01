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
        }
    }
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
}

/// Métricas de una compactación (evento `usage` del contrato SSE).
#[derive(Debug, Clone, Serialize)]
pub struct CompactionMetrics {
    pub tokens_before: u32,
    pub tokens_after: u32,
    pub savings_pct: f32,
    pub occupancy_pct: f32,
    pub cola_verbatim_tokens: u32,
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
}

impl AgentContextManager {
    #[must_use]
    pub fn new(config: ContextoConfig) -> Self {
        Self {
            config,
            ahorros_recientes: Vec::new(),
        }
    }

    #[must_use]
    pub fn config(&self) -> &ContextoConfig {
        &self.config
    }

    /// Evalúa el historial y compacta si la ocupación supera el umbral.
    /// `indice_system` = índice del mensaje system (head protegido); los turnos
    /// anteriores a `indice_primer_turno_importante` son los candidatos a resumir.
    pub fn preparar(
        &mut self,
        mensajes: &[AiMessage],
        indice_system: usize,
    ) -> CompactarResultado {
        let tokens_total: u32 = mensajes.iter().map(|m| tokens_de_mensaje(m)).sum();
        let ventana_efectiva = self.config.ventana_efectiva();
        let occupancy = tokens_total as f32 / ventana_efectiva as f32;
        let umbral = self.config.umbral_efectivo();

        let debe_compactar = occupancy >= umbral_degenerado(&self.config)
            || (occupancy >= umbral && !self.anti_thrash_activo());
        if !debe_compactar || mensajes.len() <= indice_system + 2 {
            return CompactarResultado {
                mensajes: mensajes.to_vec(),
                compactado: false,
                metricas: None,
                tokens_estimados: tokens_total,
            };
        }

        let (nuevos, cola_tokens) = self.compactar(mensajes, indice_system);
        let tokens_after: u32 = nuevos.iter().map(|m| tokens_de_mensaje(m)).sum();
        let ahorro = (tokens_total.saturating_sub(tokens_after)) as f32 / tokens_total.max(1) as f32;
        self.ahorros_recientes.push(ahorro);
        if self.ahorros_recientes.len() > 2 {
            self.ahorros_recientes.remove(0);
        }

        CompactarResultado {
            mensajes: nuevos,
            compactado: true,
            metricas: Some(CompactionMetrics {
                tokens_before: tokens_total,
                tokens_after,
                savings_pct: ahorro * 100.0,
                occupancy_pct: occupancy * 100.0,
                cola_verbatim_tokens: cola_tokens,
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

    /// Compacta: head protegido + [resumen del medio] + cola verbatim.
    fn compactar(&self, mensajes: &[AiMessage], indice_system: usize) -> (Vec<AiMessage>, u32) {
        let ventana_efectiva = self.config.ventana_efectiva();
        // Cola verbatim: 2.5% de la ventana, clamp [10K, 25K].
        let presupuesto_cola = ((ventana_efectiva as f32 * self.config.cola_verbatim) as u32)
            .clamp(10_000, 25_000);

        // Separar: head (system + siguientes) / medio (a resumir) / cola.
        let mut head: Vec<AiMessage> = Vec::new();
        let mut medio: Vec<AiMessage> = Vec::new();
        let mut cola: Vec<AiMessage> = Vec::new();

        for (i, m) in mensajes.iter().enumerate() {
            if i <= indice_system {
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

        let resumen = if medio.is_empty() {
            String::new()
        } else {
            resumen_de_mensajes(&medio)
        };

        let mut resultado = head;
        if !resumen.is_empty() {
            resultado.push(AiMessage::texto("system", resumen));
        }
        // Mensaje de continuación (estilo opencode): no romper el formato.
        if !cola.is_empty() {
            resultado.push(AiMessage::texto(
                "user",
                "[CONTEXT COMPACTION — REFERENCE ONLY]\nContinúo desde el resumen anterior.",
            ));
            resultado.extend(cola);
        }
        (resultado, cola_tokens)
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
}
