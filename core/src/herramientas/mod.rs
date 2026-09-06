//! [059A-S4] Herramientas del agente: registro `AgentToolRegistry`, tools de
//! archivos/web/comando y los dominios reutilizables (mcp, skill, todo,
//! tareas programadas, scheduler). Se re-exportan en la raíz del crate.

pub mod comando;
pub mod mcp;
pub mod repo_map;
pub mod scheduler;
pub mod skill;
pub mod tareas;
pub mod todo;
pub mod tool;
pub mod tools_archivo;
pub mod tools_web;
