//! Tareas programadas: planificación (`scheduler`) reutilizable sobre el
//! puerto `AgentPersistence`. Se re-exporta en la raíz del crate
//! (`crate::scheduler::…`), igual que antes desde `herramientas`.
//!
//! [069A-5 F4] Extraído de `herramientas/` (directorio-abarrotado: 11
//! ficheros) a dominio propio sin cambiar ningún path público.

pub mod scheduler;
