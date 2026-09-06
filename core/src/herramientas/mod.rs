//! [059A-S4] Herramientas del agente: registro `AgentToolRegistry`, tools de
//! archivos/web/comando y los dominios reutilizables (mcp, skill, todo,
//! tareas programadas). Se re-exportan en la raíz del crate.
//!
//! [069A-5 F4] `scheduler` vive en el dominio propio `tareas_programadas`.

pub mod comando;
pub mod mcp;
pub mod navegador;
pub mod repo_map;
pub mod skill;
pub mod tareas;
pub mod todo;
pub mod tool;
pub mod tools_archivo;
pub mod tools_web;
