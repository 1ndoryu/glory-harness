//! [059A-S4] Contrato del núcleo: puertos (traits de frontera), errores,
//! eventos SSE, sandbox de archivos, plan de tarea, guardas de finalización,
//! pregunta a usuario, telemetría y diff. `contrato_tests` solo compila bajo
//! test. Se re-exportan en la raíz del crate.

#[cfg(test)]
pub mod contrato_tests;
pub mod diff;
pub mod error;
pub mod evento;
pub mod guardas;
pub mod ports;
pub mod pregunta;
pub mod sandbox;
pub mod telemetria;
