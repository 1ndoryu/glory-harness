//! [F0] Servicio común del núcleo de Glory Harness, sin dependencias de Tauri.
//!
//! Contiene `SesionComun` y las operaciones fundamentales de sesión que
//! cualquier consumidor (CLI, Tauri, web) necesita: apertura, reconfiguración,
//! información y ejecución de turnos.
//!
//! El vault de archivos, los paneles y el emisor de eventos de ventana son
//! responsabilidad del consumidor y no viven aquí.

pub mod sesion;

pub use sesion::{Apertura, Error, OpcionesSesion, PreparacionTurno, ProveedorConteo, SesionComun};