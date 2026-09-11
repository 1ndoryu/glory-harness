//! [059A-S4] Subcomandos del CLI: daemon, mcp_cli y run (chat/turnos). Se
//! re-exportan en `lib.rs` para no romper los paths `glory_harness::…`.

pub mod daemon;
pub mod mcp_cli;
pub mod memoria;
pub mod notificar;
pub mod run;
pub mod sesion;
/* [109A-5 F3] El servidor web vive ahora en `web/` (mod.rs + meta/sse/turnos),
 * igual que el hub de datos en `web_datos/`: `comandos/` tenía 11 archivos y
 * la regla `directorio-abarrotado` (máx. 10) manda agrupar por dominio. Los
 * paths públicos (`comandos::web::…`) no cambian. */
pub mod web;
pub mod web_datos;
