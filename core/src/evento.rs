//! Contrato de eventos del turno (invariante H3 del plan 318A-13: el
//! frontend de task consume exactamente estos eventos vía SSE; el daemon y el
//! CLI del harness emiten el mismo contrato).
//!
//! Cada variante serializa 1:1 al SSE de task (contrato §6.4: «el handler de
//! task serializa ese enum a SSE; el frontend no cambia»). Los nombres de
//! campo coinciden byte a byte con `src/agent/runtime.rs::AgenteEvento` de
//! task (Token, ToolStart, ToolResult, RequiereAprobacion, Usage, Contexto,
//! ContextoDetalle, Error, Done).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stream de tokens de un turno: canal mpsc sin límite; `None`/cierre = fin.
pub type TokenStream = tokio::sync::mpsc::UnboundedReceiver<crate::ports::EventoTurno>;

/// Un evento emitido durante un turno del agente. Este es el contrato público
/// estable; el transporte (SSE de task, daemon loopback) serializa estos
/// eventos igual en todos los consumidores.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum AgenteEvento {
    /// Un fragmento de texto generado por el LLM.
    Token { texto: String },
    /// Inicio de una herramienta.
    ToolStart { tool: String, argumentos: serde_json::Value },
    /// Resultado de una herramienta (con diff opcional de líneas).
    ToolResult {
        tool: String,
        ok: bool,
        resumen: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// La tool requiere aprobación del usuario (política de permisos: modo
    /// predeterminado / override `ask`).
    RequiereAprobacion { tool: String, argumentos: serde_json::Value },
    /// [318A-15 F3] La tool fue denegada por política (`deny` silencioso por
    /// override o modo meta) o por negación del usuario en la UI. El runtime
    /// no reintenta la tool en ese turno: el modelo recibe el estado como
    /// resultado de tool y cambia de plan.
    PermisoDenegado {
        tool: String,
        /// Motivo presentable: "denegada_por_usuario" | "denegada_por_politica".
        motivo: String,
    },
    /// [318A-15 F4] Inicio de una sesión hija (subagente): la tool `task`
    /// del modelo padre delegó trabajo a un perfil efímero con presupuesto
    /// propio. La sesión hija nunca escribe en la conversación del padre.
    SubagenteInicio {
        perfil: String,
        instruccion: String,
    },
    /// [318A-15 F4] Fin de la sesión hija: resumen acotado devuelto al
    /// padre como resultado de la tool `task`. `parcial=true` cuando el
    /// presupuesto de pasos se agotó sin respuesta final del hijo (cierre
    /// estructurado "hecho / pendiente / siguiente paso" en vez de fallar).
    SubagenteFin {
        resumen: String,
        ok: bool,
        parcial: bool,
    },
    /// Uso parcial/final de tokens. `ocupacion_pct` lo emite el runtime tras
    /// cada compactación (barra de contexto del front); `None` en los demás.
    /// [02-09-2026] `provider`/`modelo` son el proveedor/modelo REAL que
    /// respondió (el fallback del core puede saltar a otro distinto del
    /// solicitado en `turno_config`); los emite `llm_llamada` tras resolver
    /// la cadena de candidatos. Campos opcionales: retrocompatibles para los
    /// consumidores (CLI/daemon/task) que aún no los leen.
    Usage {
        tokens_prompt: u32,
        tokens_complecion: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ocupacion_pct: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modelo: Option<String>,
    },
    /// [31-08-2026] Fase 3 (skills v1): cuántas skills activas se inyectaron
    /// como contexto en este turno (observabilidad real; el front lo ignora
    /// de forma segura).
    Contexto { skills: usize },
    /// Desglose de la ventana de contexto de la conversación: total usado por
    /// sección + reserva de salida + ventana máxima (barra con desglose del
    /// front, estilo Claude).
    ContextoDetalle {
        max_ventana: u32,
        reserva_salida: u32,
        system_instrucciones: u32,
        definiciones_tools: u32,
        mensajes: u32,
        resultados_tools: u32,
        total_entrada: u32,
        ocupacion_pct: f32,
    },
    /// Error del turno (mensaje presentable, sin detalles internos).
    /// `retryable` lo decide el consumidor (el handler de task marca
    /// reintentables Upstream/ServiceUnavailable/NotConfigured).
    Error { mensaje: String, retryable: bool },
    /// Fin del turno. `turno_id` es el id de auditoría que el consumidor
    /// creó antes del turno (el front lo usa para asociar la respuesta).
    Done { turno_id: Uuid },
}