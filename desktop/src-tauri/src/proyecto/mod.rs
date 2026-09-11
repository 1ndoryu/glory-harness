//! Dominio «proyecto» del desktop: workspaces, estado Git local y memorias del
//! proyecto activo.
//!
//! [109A-6] Agrupados por dominio para bajar la densidad de `src/`. Cada
//! submódulo mantiene su contrato de comandos: `main.rs` los sigue invocando
//! con el mismo nombre corto gracias al `use` de reexportación.

pub(crate) mod git;
pub(crate) mod memoria;
pub(crate) mod workspaces;
