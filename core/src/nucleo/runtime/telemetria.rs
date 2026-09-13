//! [139A-8 F4/S2] Telemetría y compactación manual (movimiento puro desde
//! `mod.rs`): acceso tolerante a la telemetría del turno, oráculo de bloqueo
//! por conversación y resultado de `/compactar`. Sin cambios de semántica.

use uuid::Uuid;

use crate::telemetria::TelemetriaTurno;

use super::AgentRuntime;

impl AgentRuntime {
    /// [109A-5 F4] Bloqueo vigente del plan de una conversación (motivo +
    /// turnos cerrados con ese MISMO motivo).
    ///
    /// Lo lee el servicio al cerrar el turno para decidir la pausa de la meta:
    /// el contador vive en el plan (estado efímero del runtime) porque el motivo
    /// también, y contar turnos de un plan que ya no existe no significaría
    /// nada. La lista de la conversación ACTIVA vive en la store del registry y
    /// las demás en `planes.listas`, así que se consulta el sitio que
    /// corresponde. Con la store bloqueada por una tool (no debería: los turnos
    /// son secuenciales) responde `None` en vez de bloquear un camino síncrono.
    #[must_use]
    pub fn bloqueo_de(&self, conversacion_id: Uuid) -> Option<crate::todo::BloqueoPlan> {
        let planes = self.planes.lock().unwrap_or_else(|p| p.into_inner());
        if planes.activa == Some(conversacion_id) {
            let store = self.registry.todo()?;
            let lista = store.try_lock().ok()?;
            return lista.bloqueo().cloned();
        }
        planes
            .listas
            .get(&conversacion_id)
            .and_then(|lista| lista.bloqueo().cloned())
    }

    /// [109A-5 F4] Cierra el conteo de bloqueo del turno que acaba de terminar:
    /// si el plan de ESA conversación sigue bloqueado, suma un turno.
    ///
    /// Lo hace el runtime (una vez por turno) y no la tool `todo`: N
    /// declaraciones en el mismo turno contarían N veces cuando lo que se mide
    /// son TURNOS atascados, no llamadas. Con la lista bloqueada por una tool se
    /// registra y no se cuenta; el error nunca debe romper el turno, que ya
    /// terminó.
    pub(crate) async fn contar_bloqueo_del_turno(&self, conversacion_id: Uuid) {
        let Some(store) = self.registry.todo() else {
            return;
        };
        let es_activa = {
            let planes = self.planes.lock().unwrap_or_else(|p| p.into_inner());
            planes.activa == Some(conversacion_id)
        };
        if !es_activa {
            /* El plan de esta conversación no está cargado (el turno no llegó a
             * `cargar_plan_de`): el contador vive en su lista guardada. */
            let mut planes = self.planes.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(lista) = planes.listas.get_mut(&conversacion_id) {
                lista.contar_turno_bloqueado();
            }
            return;
        }
        let Ok(mut lista) = store.try_lock() else {
            tracing::warn!(
                %conversacion_id,
                "bloqueo del plan no contado: la lista de tareas estaba bloqueada"
            );
            return;
        };
        lista.contar_turno_bloqueado();
    }

    /// [318A-15 F0] Acceso a la telemetría tolerante a envenenamiento:
    /// un panic en otro hilo no debe abortar el turno (la telemetría nunca
    /// debe poder romper la ejecución — es observación, no contrato).
    pub(crate) fn telemetria(&self) -> std::sync::MutexGuard<'_, TelemetriaTurno> {
        self.telemetria.lock().unwrap_or_else(|p| p.into_inner())
    }
}

/// [109A-4 F3] Resultado de una compactación pedida por el usuario desde la
/// UI (`/compactar`). Fuera del bucle del turno no hay canal de eventos, así
/// que el runtime devuelve el resultado completo (métricas + resumen) para que
/// el consumidor lo muestre y lo persista. `motivo` nunca es `None` cuando
/// `compactado` es falso: un no-op siempre se explica.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CompactarManual {
    pub compactado: bool,
    pub motivo: Option<String>,
    pub tokens_antes: u32,
    pub tokens_despues: u32,
    pub ahorro_pct: f32,
    pub ocupacion_pct: f32,
    pub tramos: u32,
    /// Resumen del tramo compactado. El consumidor lo persiste para que los
    /// turnos siguientes arranquen de él en vez del historial entero.
    pub resumen: Option<String>,
}

impl CompactarManual {
    /// No-op explicado: nada se compactó y el motivo queda visible.
    #[must_use]
    pub(crate) fn no_compactado(
        motivo: impl Into<String>,
        tokens_antes: u32,
        ocupacion_pct: f32,
    ) -> Self {
        Self {
            compactado: false,
            motivo: Some(motivo.into()),
            tokens_antes,
            tokens_despues: tokens_antes,
            ahorro_pct: 0.0,
            ocupacion_pct,
            tramos: 0,
            resumen: None,
        }
    }
}
