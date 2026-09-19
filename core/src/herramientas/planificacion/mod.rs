//! Herramientas de planificación y ejecución del agente (089A-16).
//!
//! Agrupa los dominios que convierten intención en acción: lista de tareas
//! (`todo`), tareas programadas (`tareas`), ejecución de comandos (`comando`),
//! mapa del repositorio (`repo_map`) y búsqueda web (`tools_web`). Se
//! re-exportan en [`crate::herramientas`] para preservar las rutas
//! (`crate::tareas`, `crate::todo`, …).

pub mod comando;
pub mod repo_map;
pub mod tareas;
pub mod todo;
pub mod tools_web;
