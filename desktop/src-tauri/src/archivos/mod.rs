//! Dominio «archivos» del desktop: lectura para el visor, filesystem del
//! workspace activo y vault de respaldos.
//!
//! [109A-6] `src/` estaba en el techo de 10 ficheros planos de
//! `directorio-abarrotado`, así que los módulos se agrupan por dominio. Los
//! submódulos son `pub(crate)` y `main.rs` los reexpone con `use`, de forma que
//! las rutas ya escritas (`vault::VaultArchivos`, `filesystem::workspace_info`,
//! `archivo::leer_archivo`) no cambian: solo cambia de dónde se declaran.

pub(crate) mod archivo;
pub(crate) mod filesystem;
pub(crate) mod vault;
