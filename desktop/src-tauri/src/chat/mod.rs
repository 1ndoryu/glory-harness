//! Dominio «chat» del desktop: CRUD de conversaciones y ejecución del turno.
//!
//! [109A-6] Agrupados por dominio para bajar la densidad de `src/` (techo de 10
//! ficheros planos). `turno` sigue viendo `conversaciones` como `super::`, así
//! que sus llamadas internas no cambian.

pub(crate) mod conversaciones;
pub(crate) mod turno;
