//! [139A-8 F4/S2] Ciclo del turno (movimiento puro desde `mod.rs`): reparto
//! del plan por conversación, emisión de tareas, aprobaciones, curador
//! nativo, system prompt, hooks y texto de cierre. Sin cambios de semántica.

use serde_json::Value;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::error::Result;
use crate::evento::AgenteEvento;
use crate::hooks::{EventoHook, SalidaHook};

use super::{ensamblar_prompt_sistema, fecha_hoy, AgentRuntime};

impl AgentRuntime {
    /// [109A-5 F2] Carga en el registry la lista de ESTA conversación, dejando
    /// guardada la de la anterior (ver [`PlanesConversacion`]). Se llama al
    /// arrancar cada turno: un turno de otra conversación nunca ve ni publica
    /// las tareas de la previa, y volver a una conversación ya visitada
    /// restaura su plan en vez de perderlo.
    ///
    /// Si una tool tiene la lista bloqueada (no debería: los turnos son
    /// secuenciales) se deja el reparto como está y se registra; forzar el
    /// cambio con `lock()` desde un camino síncrono bloquearía el runtime.
    pub(crate) fn cargar_plan_de(&self, conversacion_id: Uuid) {
        let Some(store) = self.registry.todo() else {
            return;
        };
        let mut planes = self.planes.lock().unwrap_or_else(|p| p.into_inner());
        if planes.activa == Some(conversacion_id) {
            return;
        }
        /* Primer turno del runtime: la store todavía no pertenece a ninguna
         * conversación, así que se ADOPTA su contenido en vez de vaciarlo
         * (vaciarlo aquí perdería el plan del turno anterior en consumidores
         * que construyen el runtime por turno). */
        let Some(anterior) = planes.activa else {
            planes.activa = Some(conversacion_id);
            return;
        };
        let Ok(mut lista) = store.try_lock() else {
            tracing::warn!(
                %conversacion_id,
                "plan visible no reasignado: la lista de tareas estaba bloqueada"
            );
            return;
        };
        planes.listas.insert(anterior, lista.clone());
        let nueva = planes.listas.remove(&conversacion_id).unwrap_or_default();
        *lista = nueva;
        planes.activa = Some(conversacion_id);
    }

    /// [109A-5 F2] Publica el plan visible COMPLETO al canal del turno como
    /// evento `TareasActualizadas`. Se emite tras cada acción de la tool
    /// `todo` y al arrancar un turno que ya tenía plan vigente (resume).
    ///
    /// `solo_si_hay` evita el ruido del arranque: una lista vacía al empezar un
    /// turno haría que la UI dibujara un bloque de tareas sin tareas. Tras una
    /// acción de `todo` sí se publica aunque quede vacía: el usuario debe ver
    /// que el plan terminó. Sin store de `todo` (defensivo: siempre está
    /// registrada) no emite.
    pub(crate) async fn emitir_tareas(&self, tx: &Sender<AgenteEvento>, solo_si_hay: bool) {
        let Some(store) = self.registry.todo() else {
            return;
        };
        let items = store.lock().await.visibles();
        if solo_si_hay && items.is_empty() {
            return;
        }
        let _ = tx.send(AgenteEvento::TareasActualizadas { items }).await;
    }

    /// [109A-5 F2] Vacía el plan de la conversación porque su meta se CERRÓ
    /// (lograda o limpiada): el plan perseguía esa meta y ya no aplica.
    ///
    /// El scope importa: se vacía el de ESA conversación (guardado o cargado),
    /// no "el que esté activo". Devuelve `false` solo si la lista activa estaba
    /// bloqueada por una tool en ese instante; el llamador lo registra en vez de
    /// fingir que se limpió. Con turnos secuenciales no puede ocurrir entre
    /// turnos, así que no propaga error.
    #[must_use]
    pub fn olvidar_tareas(&self, conversacion_id: Uuid) -> bool {
        let Some(store) = self.registry.todo() else {
            return true;
        };
        let mut planes = self.planes.lock().unwrap_or_else(|p| p.into_inner());
        planes.listas.remove(&conversacion_id);
        if planes.activa != Some(conversacion_id) {
            return true;
        }
        let Ok(mut lista) = store.try_lock() else {
            return false;
        };
        lista.vaciar();
        true
    }

    /// [069A-4] Pasada del curador de memoria sin LLM (diseño §3): poda
    /// duplicadas, archiva obsoletas sin uso reciente y promueve a skill lo
    /// maduro y muy usado. La usa el motor del cron cuando el prompt es el
    /// marcador [`crate::memoria::MARCADOR_CURADOR`] y el subcomando CLI
    /// `memoria curar`. Determinista y sin coste de proveedor.
    ///
    /// [109A-2] Recorre **todos** los ámbitos del usuario (global + cada
    /// proyecto con recuerdos): curar solo el global dejaría los proyectos
    /// sin pasar nunca. El resumen agregado anota el proyecto de cada clave.
    pub async fn ejecutar_curador_nativo(
        &self,
        user_id: Uuid,
    ) -> Result<crate::memoria::ResumenCurador> {
        crate::memoria::ejecutar_curador_todos(
            &self.puertos.persistencia,
            user_id,
            &crate::memoria::PoliticaCurador::default(),
        )
        .await
    }

    /* [318A-16 F2] Canal de aprobación explícito: la UI responde las
     * peticiones emitidas como `PeticionAprobacion` (id) entre turnos. Las
     * tres vías — Aprobar (una vez), Rechazar (regla deny de la clase),
     * Siempre (regla allow de la clase) — se aplican en el registro, que es
     * el mismo que consulta la decisión del siguiente turno. */

    /// Responde una petición de aprobación pendiente (tres vías).
    /// `Err` si el id es desconocido o ya fue respondido.
    /// [129A-5] Devuelve si se despertó a un turno en espera.
    pub fn responder_aprobacion(
        &self,
        id: &str,
        respuesta: crate::aprobacion::RespuestaAprobacion,
    ) -> std::result::Result<bool, String> {
        self.registry.responder_peticion(id, respuesta)
    }

    /// Peticiones de aprobación pendientes sin responder (para que la UI
    /// ofrezca las tres vías después del turno).
    #[must_use]
    pub fn peticiones_aprobacion_pendientes(&self) -> Vec<crate::aprobacion::PeticionAprobacion> {
        self.registry.peticiones_pendientes()
    }

    #[must_use]
    pub fn tools_registradas(&self) -> Vec<&str> {
        self.registry.ids()
    }

    /// [318A-15 F1/F2] Ensambla el system prompt de capas para el turno actual
    /// (base estática → ranura [REGLAS] con las reglas del consumidor → bloque
    /// [ENTORNO] con la fecha real).
    ///
    /// [109A-5 F2] En modo `meta` (turno de persecución) se anexan las reglas de
    /// meta a la ranura del consumidor: la obligación de mantener las tareas
    /// visibles la tiene el modelo, así que no puede depender de que el
    /// consumidor las escriba en su AGENTS.md.
    pub(crate) fn prompt_sistema(&self) -> String {
        let mut reglas = self
            .reglas
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if self.modo_efectivo() == "meta" {
            if !reglas.trim().is_empty() {
                reglas.push_str("\n\n");
            }
            reglas.push_str(crate::nucleo::prompt::REGLAS_META);
        }
        ensamblar_prompt_sistema(&self.turno_config, &reglas, &fecha_hoy())
    }

    /// Dispara un hook y devuelve su salida completa para los eventos que
    /// tienen un canal de ajuste. Los consumidores existentes que solo
    /// necesitan veto usan [`Self::disparar_hook`].
    pub(crate) async fn disparar_hook_con_salida(
        &self,
        evento: EventoHook,
        payload: Value,
    ) -> SalidaHook {
        let hooks = self.hooks.lock().unwrap_or_else(|p| p.into_inner()).clone();
        hooks.disparar_con_salida(evento, payload).await
    }

    pub(crate) async fn disparar_hook(&self, evento: EventoHook, payload: Value) -> bool {
        self.disparar_hook_con_salida(evento, payload).await.bloqueo
    }
}

pub(crate) fn mensajes_usuario_resumen(mensaje: &str) -> String {
    mensaje.chars().take(500).collect()
}

/// [318A-15 F5] Consigna del wrap-up al agotar `max_turns`: en vez de cortar
/// en seco, el modelo cierra con un resumen estructurado. Se inyecta como
/// mensaje system en la última llamada (sin tools).
pub(crate) const WRAP_UP_TEXTO: &str = "Has agotado el límite de pasos de este turno. NO ejecutes más herramientas.\nCierra con un resumen breve y estructurado:\n- HECHO: qué se completó hasta ahora.\n- PENDIENTE: qué quedó sin hacer y por qué.\n- SIGUIENTE PASO: qué harías si pudieras continuar.\nSi el objetivo ya está cumplido, dilo y resume el resultado.";

#[must_use]
pub(crate) fn wrap_up_instruccion() -> String {
    WRAP_UP_TEXTO.to_string()
}
