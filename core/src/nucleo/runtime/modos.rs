//! [139A-8 F4/S2] Modos del turno (movimiento puro desde `mod.rs`): guard
//! del modo forzado `/meta`, modo efectivo y store del plan en curso. Sin
//! cambios de semántica.

use super::AgentRuntime;

/// [109A-4 F4] Guard del modo forzado de un turno: al dropearse deja el
/// runtime sin override, pase lo que pase con el turno (fin, error,
/// cancelación del cliente o panic). Vive solo dentro de `ejecutar_turno`.
pub(crate) struct GuardaModoTurno<'a> {
    runtime: &'a AgentRuntime,
}

impl Drop for GuardaModoTurno<'_> {
    fn drop(&mut self) {
        *self
            .runtime
            .modo_turno
            .lock()
            .unwrap_or_else(|p| p.into_inner()) = None;
    }
}

impl AgentRuntime {
    /// [109A-4 F4] Fija (o limpia) el modo forzado del turno y devuelve el
    /// guard que lo limpia al soltarse (auxiliar de `ejecutar_turno_con_modo`).
    pub(crate) fn guarda_modo_turno(&self, modo: Option<&str>) -> GuardaModoTurno<'_> {
        *self.modo_turno.lock().unwrap_or_else(|p| p.into_inner()) = modo.map(str::to_string);
        GuardaModoTurno { runtime: self }
    }

    /// [109A-5 F2] Modo efectivo AHORA: el forzado del turno en curso si lo
    /// hay, el modo de la sesión si no. Todo el turno (schemas que ve el
    /// modelo, permisos, subagente, store del plan) lee de aquí, así que un
    /// `/meta` afecta a un turno entero y a nada más. Mutex envenenado → modo
    /// de sesión (el override es una restricción adicional, no un permiso:
    /// caer al modo global nunca abre más de lo que el usuario configuró).
    #[must_use]
    pub fn modo_efectivo(&self) -> String {
        self.modo_turno
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_else(|| self.turno_config.modo.clone())
    }

    /// [318A-16 F5] Store del plan del turno actual (si el turno corrió en
    /// modo plan). El consumidor la usa tras `ejecutar_turno` para mostrar el
    /// diff acumulado, aprobarlo (`crate::plan::aplicar_plan` con su sandbox)
    /// o descartarlo.
    pub fn plan_actual(&self) -> Option<crate::plan::PlanCompartida> {
        self.plan_actual
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
