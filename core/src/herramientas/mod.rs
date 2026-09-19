//! [059A-S4] Herramientas del agente: registro `AgentToolRegistry`, tools de
//! archivos/web/comando y los dominios reutilizables (mcp, skill, todo,
//! tareas programadas). Se re-exportan en la raíz del crate.
//!
//! [069A-5 F4] `scheduler` vive en el dominio propio `tareas_programadas`.

pub mod archivo;
pub mod contexto;
pub mod mcp;
pub mod navegador;
pub mod planificacion;
pub mod registro;
pub mod resultado;
pub mod skill;
pub mod tool;

pub use archivo::{content_search, tools_archivo};
pub use planificacion::{comando, repo_map, tareas, todo, tools_web};
