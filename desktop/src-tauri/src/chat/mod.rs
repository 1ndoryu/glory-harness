//! Dominio «chat» del desktop: CRUD de conversaciones y ejecución del turno.
//!
//! [109A-6] Agrupados por dominio para bajar la densidad de `src/` (techo de 10
//! ficheros planos). `turno` sigue viendo `conversaciones` como `super::`, así
//! que sus llamadas internas no cambian.
//! [Partición limite-lineas] `conversaciones` se partió en `crud`, `consolas`
//! y `rewind`; `conversaciones` re-exporta los comandos para no cambiar las
//! rutas registradas en `main.rs`.

pub(crate) mod consolas;
pub(crate) mod conversaciones;
pub(crate) mod crud;
pub(crate) mod rewind;
pub(crate) mod turno;
