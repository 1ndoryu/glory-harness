//! [059A-S4] Política de permisos y aprobación: modelo de permisos por tool,
//! reglas v2 (categorías/patrones, fail-closed), peticiones de aprobación y
//! el clasificador de riesgo de comandos. Se re-exportan en la raíz del crate.

pub mod aprobacion;
pub mod bash_clasificar;
pub mod permiso;
pub mod regla;
