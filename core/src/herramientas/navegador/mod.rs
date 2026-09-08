//! [079A-1 F4] Dominio navegador (partido de herramientas/navegador.rs).
//!
//! [069A-1 F5] Tool `navegador_reflejo` del agente: permite al modelo
//! navegar, capturar y manipular el navegador interno WebView2 child.
//!
//! Esta tool solo se registra cuando el consumidor aporta el puerto
//! [`NavegadorPort`]; sin él, la tool no existe (fail-closed: el modelo
//! ni la ve). Todas las operaciones emiten evento `ToolStart`/`ToolResult`
//! estándar; el front refleja las acciones en el panel navegador vía
//! [`AgenteEvento::ToolNavegador`] (emitido como evento adicional).
//!
//! Re-exporta la tool para que `herramientas::navegador::ToolNavegadorReflejo`
//! siga resolviendo sin cambios.

mod operaciones;
#[cfg(test)]
mod pruebas;
mod reflejo;

pub(crate) use reflejo::ToolNavegadorReflejo;
