//! [Bloque 3, F4] Hooks de ciclo de vida del agente (evidencia: claurst
//! `docs/hooks.md` + `spec/07_hooks.md`).
//!
//! El núcleo emite eventos en los puntos del ciclo de vida (turno, tool,
//! subagente, compactación, permiso, sesión) y los hooks configurados se
//! disparan SIN acoplarse a un runner concreto: `DispatcherHooks` delega en
//! [`RunnerHook`], de modo que los tests inyectan un runner que graba (nunca
//! lanzan procesos) y el CLI/producción usa [`RunnerComandoHttp`].
//!
//! Semántica (espejo claurst):
//! - Un hook es `command` (proceso local con timeout) u `http` (POST JSON).
//!   Los tipos `prompt`/`agent` quedan diferidos (decisión del plan).
//! - Matcher por evento + patrón opcional de tool con `*` (comodín).
//! - Un hook de `command` que termina con exit code 2 BLOQUEA la acción en
//!   curso, pero solo en los eventos que pueden bloquear ([`EventoHook::
//!   puede_bloquear`]); el resto son informativos (sus fallos se registran y
//!   el turno continúa — nunca rompen la ejecución).
//! - El payload viaja como JSON (stdin del proceso / cuerpo del POST).
//! - Sin hooks configurados el dispatcher es un no-op barato: el runtime no
//!   cambia su comportamiento (los hooks son observación/política opcional).

// [059A-S5] Dominio hooks dividido por responsabilidad:
// tipos (contratos) -> runner (ejecucion) -> despacho (orquestacion).
// Re-export plano: `crate::hooks::{Hook, RunnerHook, DispatcherHooks}` sin cambios.
pub mod tipos;
pub mod runner;
pub mod despacho;

pub use tipos::*;
pub use runner::*;
pub use despacho::*;
