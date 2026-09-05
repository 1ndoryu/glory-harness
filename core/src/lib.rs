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

/* [059A-S4] Organización por dominio (glory-sentinel directorio-abarrotado).
 * Cada grupo es un submódulo que re-declara sus hijos; el glob los
 * re-exporta en la raíz del crate, de modo que los paths internos
 * (`crate::tool::…`) y los de los consumidores (`glory_harness_core::tool`)
 * siguen resolviendo sin tocar ningún `use`. */
mod nucleo;
mod herramientas;
mod politica;
mod contrato;

pub use nucleo::*;
pub use herramientas::*;
pub use politica::*;
pub use contrato::*;

/// Frontera de puertos (traits) que define el núcleo y que el consumidor
/// implementa. Ver [`ports`].
pub use ports::{
    AgentPersistence, ContenidoWeb, EjecutorComando, ProgramadorTareas, ProviderPort, WebFetchProvider,
    WebSearchProvider,
};
/// Errores propios del núcleo (sin dependencia de `AppError` de task).
pub use error::{Error as HarnessError, Result as HarnessResult};

/// Versión del contrato de puertos y eventos.
pub const CONTRATO_VERSION: &str = "1.0.0";