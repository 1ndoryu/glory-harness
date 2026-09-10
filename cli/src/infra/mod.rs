//! [059A-S4] Infraestructura del CLI: ejecución de comandos, fetch web,
//! persistencia y reglas. `persistencia_sqlite.rs` NO se mueve aquí (ajeno
//! 039A-3; se integrará al cerrar). Se re-exportan en `lib.rs`.

pub mod ejecutor;
pub mod fetch;
pub mod memoria_io;
pub mod persistencia;
pub mod reglas;
