//! [059A-S4] Núcleo del agente: contexto de conversación, esquemas de
//! subagente, runtime (bucle de turno) y servicio LLM. Los módulos se
//! re-exportan en la raíz del crate para no romper los paths internos ni a los
//! consumidores.

pub mod context;
pub mod plan;
pub mod subagente;
pub mod llm;
pub mod runtime;
