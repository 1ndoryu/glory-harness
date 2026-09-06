//! [059A-S4] Subcomandos del CLI: daemon, mcp_cli y run (chat/turnos). Se
//! re-exportan en `lib.rs` para no romper los paths `glory_harness::…`.

pub mod daemon;
pub mod mcp_cli;
pub mod memoria;
pub mod notificar;
pub mod run;
pub mod sesion;
pub mod web;
pub mod web_datos;
pub mod web_turnos;
