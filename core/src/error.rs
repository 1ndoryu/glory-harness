//! Error propio del núcleo. Reemplaza `crate::errors::AppError` en el código
//! agnóstico: el consumidor traduce a su error HTTP en el borde.

use std::fmt;

/// Error del núcleo de Glory Harness.
#[derive(Debug, Clone)]
pub enum Error {
    /// Argumentos de tool inválidos (fallo de validación de JSON Schema).
    Argumentos(String),
    /// La tool no existe en el registro.
    ToolDesconocida(String),
    /// Error de proveedor LLM (red, auth, upstream). Lleva un detalle
    /// presentable y una causa interna opcional que **no** se expone al LLM.
    Proveedor { detalle: String, causa: Option<String> },
    /// Entrada inválida (payload mal formado, parámetros fuera de rango).
    Validacion(String),
    /// La operación de persistencia declarada por el puerto falló.
    Persistencia(String),
    /// El sandbox bloqueó la ruta (fuera de los directorios permitidos).
    Sandbox(String),
    /// Recurso no encontrado (archivo inexistente, fila ausente).
    NoEncontrado(String),
    /// Timeout o límite excedido (turnos, contexto, herramienta).
    Limite(String),
    /// Cancelación del turno (el consumidor cerró el SSE).
    Cancelado,
    /// Error interno inesperado (panic capturado, invariante rota).
    Interno(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Argumentos(msg) => write!(f, "argumentos de tool inválidos: {msg}"),
            Error::ToolDesconocida(id) => write!(f, "tool desconocida: {id}"),
            Error::Proveedor { detalle, causa: Some(causa) } => {
                write!(f, "{detalle} ({causa})")
            }
            Error::Proveedor { detalle, causa: None } => write!(f, "{detalle}"),
            Error::Validacion(msg) => write!(f, "entrada inválida: {msg}"),
            Error::Persistencia(msg) => write!(f, "error de persistencia: {msg}"),
            Error::Sandbox(msg) => write!(f, "ruta bloqueada por el sandbox: {msg}"),
            Error::NoEncontrado(msg) => write!(f, "recurso no encontrado: {msg}"),
            Error::Limite(msg) => write!(f, "límite excedido: {msg}"),
            Error::Cancelado => write!(f, "turno cancelado"),
            Error::Interno(msg) => write!(f, "error interno: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = core::result::Result<T, Error>;