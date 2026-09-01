//! Contrato de eventos del turno (invariante H3 del plan 318A-13: el
//! frontend de task consume exactamente estos eventos vía SSE; el daemon y el
//! CLI del harness emiten el mismo contrato).

use serde::{Deserialize, Serialize};

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
    /// La tool requiere aprobación del usuario (modo predeterminado).
    RequiereAprobacion { tool: String, argumentos: serde_json::Value },
    /// Uso parcial/final de tokens.
    Usage { prompt_tokens: u32, completion_tokens: u32, total_tokens: u32 },
    /// Resumen del contexto inyectado (memoria, skills, notas…).
    Contexto { resumen: String },
    /// Detalle del contexto inyectado (para la UI de auditoría).
    ContextoDetalle { secciones: Vec<SeccionContexto> },
    /// Error del turno (mensaje presentable, sin detalles internos).
    Error { mensaje: String },
    /// Fin del turno (motivo: ok | error | cancelado | max_tokens…).
    Done { motivo: String },
}

/// Sección del contexto inyectado al prompt (auditoría).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeccionContexto {
    pub nombre: String,
    pub contenido: String,
}

/// Stream de tokens de un turno: canal mpsc sin límite; `None`/cierre = fin.
pub type TokenStream = tokio::sync::mpsc::UnboundedReceiver<crate::ports::EventoTurno>;