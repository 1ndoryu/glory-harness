//! [089A-8] Tools de archivo: ficheros (`tools_archivo`: leer, escribir,
//! parche, búsqueda por nombre) y búsqueda indexada por contenido
//! (`content_search`, motor tgrep-core compilado). Se re-exportan en
//! `herramientas` para conservar las rutas `crate::tools_archivo::*` y
//! `crate::content_search::*`.

pub mod content_search;
pub mod tools_archivo;
