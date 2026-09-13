//! [139A-8 F4/S1] Fachada de compatibilidad: el framework de tools se partió
//! en `contexto` ([`AgentToolContext`]), `resultado` ([`AgentToolResult`]) y
//! `registro` (contrato [`AgentTool`] + [`AgentToolRegistry`]).
//!
//! Este módulo solo re-exporta para no tocar los ~25 consumidores que usan
//! `crate::tool::X` / `glory_harness_core::tool::X`.

pub use super::contexto::AgentToolContext;
pub use super::registro::{AgentTool, AgentToolRegistry};
pub use super::resultado::AgentToolResult;
