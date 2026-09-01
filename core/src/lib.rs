//! # glory-harness-core
//!
//! Núcleo de IA agnóstico extraíble (plan 318A-13). Este crate **no conoce** a
//! task ni a ningún consumidor: no importa `AppState`, `PgPool` ni tablas
//! `agente_*`; la persistencia entra por el puerto [`AgentPersistence`], la
//! búsqueda web por [`WebSearchProvider`] y los proveedores LLM por
//! [`ProviderPort`].
//!
//! La frontera se documenta en `README.md` (sección «Frontera»); los eventos
//! del turno en [`crate::evento::AgenteEvento`].

#![forbid(unsafe_code)]

pub mod ports;
pub mod evento;
pub mod error;
pub mod llm;
pub mod diff;
pub mod context;
pub mod sandbox;
pub mod tool;
pub mod tools_archivo;
pub mod tools_web;
pub mod scheduler;
#[cfg(test)]
pub mod contrato_tests;

/// Frontera de puertos (traits) que define el núcleo y que el consumidor
/// implementa. Ver [`ports`].
pub use ports::{AgentPersistence, ProviderPort, WebSearchProvider};
/// Errores propios del núcleo (sin dependencia de `AppError` de task).
pub use error::{Error as HarnessError, Result as HarnessResult};

/// Versión del contrato de puertos y eventos.
pub const CONTRATO_VERSION: &str = "1.0.0";