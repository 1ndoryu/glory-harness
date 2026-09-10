//! [F0] Servicio común del núcleo de Glory Harness, sin dependencias de Tauri.
//!
//! Contiene `SesionComun` y las operaciones fundamentales de sesión que
//! cualquier consumidor (CLI, Tauri, web) necesita: apertura, reconfiguración,
//! información y ejecución de turnos.
//!
//! El vault de archivos, los paneles y el emisor de eventos de ventana son
//! responsabilidad del consumidor y no viven aquí.

pub mod meta;
pub mod sesion_config;
pub mod sesion;

pub use meta::{
    aplicar_en_borrador, comando_desde_payload, elapsed_ms, resolver_meta, ComandoMeta, ErrorMeta,
    EstadoMeta, LogroMeta, MetaActiva, ResultadoMeta, MAX_LOGROS_META, MAX_META_CHARS,
};
pub use sesion::{Apertura, Error, OpcionesSesion, PreparacionTurno, ProveedorConteo, SesionComun};
