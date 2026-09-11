//! Contrato de eventos del turno (invariante H3 del plan 318A-13: el
//! frontend de task consume exactamente estos eventos vía SSE; el daemon y el
//! CLI del harness emiten el mismo contrato).
//!
//! Cada variante serializa 1:1 al SSE de task (contrato §6.4: «el handler de
//! task serializa ese enum a SSE; el frontend no cambia»). Los nombres de
//! campo coinciden byte a byte con `src/agent/runtime.rs::AgenteEvento` de
//! task (Token, ToolStart, ToolResult, RequiereAprobacion, Usage, Contexto,
//! ContextoDetalle, Error, Done).

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Stream de tokens de un turno: canal mpsc sin límite; `None`/cierre = fin.
pub type TokenStream = tokio::sync::mpsc::UnboundedReceiver<crate::ports::EventoTurno>;

/// [318A-15 F0] Agregado por tool de un turno (telemetría).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetriaTool {
    pub tool: String,
    pub usos: u32,
    pub fallos: u32,
    pub duracion_ms_total: u64,
}

/// [109A-5 F2] Estado de una tarea visible en la UI. Los nombres serializados
/// (`pendiente`/`en_curso`/`completada`) son contrato con el front; la marca
/// textual equivalente vive en `EstadoTodo::marca` del dominio (`[ ]`/`[/]`/`[x]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EstadoTareaVisible {
    Pendiente,
    EnCurso,
    Completada,
}

/// [109A-5 F2] Tarea del plan visible de la conversación (tool `todo`): DTO del
/// contrato, sin acoplar el front al tipo interno de la herramienta.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TareaVisible {
    /// ID estable dentro de la lista del runtime (1-based).
    pub id: u32,
    pub texto: String,
    pub estado: EstadoTareaVisible,
}

/// Un evento emitido durante un turno del agente. Este es el contrato público
/// estable; el transporte (SSE de task, daemon loopback) serializa estos
/// eventos igual en todos los consumidores.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum AgenteEvento {
    /// Un fragmento de texto generado por el LLM.
    Token { texto: String },
    /// Inicio de una herramienta.
    ToolStart {
        tool: String,
        argumentos: serde_json::Value,
    },
    /// Resultado de una herramienta (con diff opcional de líneas).
    ToolResult {
        tool: String,
        ok: bool,
        resumen: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        diff: Option<String>,
    },
    /// La tool requiere aprobación del usuario (política de permisos: modo
    /// predeterminado / override `ask`).
    RequiereAprobacion {
        tool: String,
        argumentos: serde_json::Value,
    },
    /// [318A-16 F2] Petición de aprobación con `id` para responder por canal
    /// explícito (`AgentRuntime::responder_aprobacion`) con tres vías
    /// (Rechazar / Permitir / Permitir siempre). Se emite junto a
    /// `RequiereAprobacion` (que se conserva por compatibilidad); `ask` NO
    /// suspende el turno: el modelo pide confirmación y la UI responde entre
    /// turnos, o el humano confirma en texto (flujo conversacional previo).
    PeticionAprobacion {
        id: String,
        tool: String,
        argumentos: serde_json::Value,
        /// Clase derivada F1 ("categoría:patrón" o "tool:*"): lo que
        /// "Permitir siempre" recordará como regla.
        clasificacion: String,
    },
    /// [04-09-2026 B3-F1] La tool `ask_user` preguntó al usuario con opciones
    /// (claurst `ask_user.rs` / opencode `question.ts`): la UI la muestra y el
    /// turno termina — la respuesta llega como nuevo mensaje de usuario (el
    /// `id` permite correlacionar). Aditivo para los consumidores existentes.
    Pregunta {
        id: String,
        texto: String,
        /// Opciones sugeridas (vacío = respuesta libre).
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        opciones: Vec<String>,
    },
    /// [318A-15 F3] La tool fue denegada por política (`deny` silencioso por
    /// override o modo meta) o por negación del usuario en la UI. El runtime
    /// no reintenta la tool en ese turno: el modelo recibe el estado como
    /// resultado de tool y cambia de plan.
    PermisoDenegado {
        tool: String,
        /// Motivo presentable: "denegada_por_usuario" | "denegada_por_politica".
        motivo: String,
    },
    /// [318A-15 F4] Inicio de una sesión hija (subagente): la tool `task`
    /// del modelo padre delegó trabajo a un perfil efímero con presupuesto
    /// propio. La sesión hija nunca escribe en la conversación del padre.
    SubagenteInicio { perfil: String, instruccion: String },
    /// [318A-16 F5] Propuesta acumulada del modo plan al cerrar el turno:
    /// la UI muestra el diff (`resumen`) y ofrece "Aprobar y aplicar" (una
    /// sola aplicación) o descartar. Solo se emite en modo `plan` con
    /// cambios pendientes; aditivo para los consumidores existentes.
    PlanPropuesto { cambios: usize, resumen: String },
    /// [318A-15 F4] Fin de la sesión hija: resumen acotado devuelto al
    /// padre como resultado de la tool `task`. `parcial=true` cuando el
    /// presupuesto de pasos se agotó sin respuesta final del hijo (cierre
    /// estructurado "hecho / pendiente / siguiente paso" en vez de fallar).
    SubagenteFin {
        resumen: String,
        ok: bool,
        parcial: bool,
    },
    /// Uso parcial/final de tokens. `ocupacion_pct` lo emite el runtime tras
    /// cada compactación (barra de contexto del front); `None` en los demás.
    /// [02-09-2026] `provider`/`modelo` son el proveedor/modelo REAL que
    /// respondió (el fallback del core puede saltar a otro distinto del
    /// solicitado en `turno_config`); los emite `llm_llamada` tras resolver
    /// la cadena de candidatos. Campos opcionales: retrocompatibles para los
    /// consumidores (CLI/daemon/task) que aún no los leen.
    Usage {
        tokens_prompt: u32,
        tokens_complecion: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ocupacion_pct: Option<f32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        modelo: Option<String>,
    },
    /// [31-08-2026] Fase 3 (skills v1): cuántas skills activas se inyectaron
    /// como contexto en este turno (observabilidad real; el front lo ignora
    /// de forma segura).
    Contexto { skills: usize },
    /// Desglose de la ventana de contexto de la conversación: total usado por
    /// sección + reserva de salida + ventana máxima (barra con desglose del
    /// front, estilo Claude).
    ContextoDetalle {
        max_ventana: u32,
        reserva_salida: u32,
        system_instrucciones: u32,
        definiciones_tools: u32,
        mensajes: u32,
        resultados_tools: u32,
        total_entrada: u32,
        ocupacion_pct: f32,
    },
    /// [318A-15 F0] Telemetría del turno (no invasiva): agregados que el
    /// runtime ya observa durante la ejecución (usos/fallos/duración por
    /// tool, compactaciones, denegaciones y subagentes parciales). Evento
    /// nuevo en el contrato SSE; los consumidores existentes lo ignoran de
    /// forma segura. Se emite una sola vez, justo antes de `Done`; los turnos
    /// fallidos emiten `Error` (motivo + retryable) sin `Telemetria`.
    /// Diseñado para que F2/F6 (umbrales) y la decisión F4 item-8 la
    /// consuman: `herramientas`/`denegaciones`/`subagentes_parciales` son
    /// agregables por `conversacion_id`.
    Telemetria {
        conversacion_id: Uuid,
        /// Cierre del turno: `respuesta_final` | `limite_pasos` |
        /// `sse_cortado` | `sin_respuesta`.
        motivo_cierre: String,
        /// Compactaciones registradas por el gestor de contexto de esta
        /// conversación (acumulado desde que el runtime existe; en el CLI el
        /// runtime vive por sesión de chat).
        compactaciones: u32,
        /// Denegaciones de permiso (política o usuario) emitidas en el turno.
        denegaciones: u32,
        /// Subagentes cerrados como parciales (presupuesto agotado) en el
        /// turno.
        subagentes_parciales: u32,
        herramientas: Vec<TelemetriaTool>,
    },
    /// Error del turno (mensaje presentable, sin detalles internos).
    /// `retryable` lo decide el consumidor (el handler de task marca
    /// reintentables Upstream/ServiceUnavailable/NotConfigured).
    Error { mensaje: String, retryable: bool },
    /// Fin del turno. `turno_id` es el id de auditoría que el consumidor
    /// creó antes del turno (el front lo usa para asociar la respuesta).
    Done { turno_id: Uuid },
    /// [069A-1 F6] El agente ejecutó una operación del navegador interno
    /// (`navegador_reflejo`). El front refleja la acción en el panel y, si
    /// es `capturar`, muestra la imagen.
    ToolNavegador {
        accion: String,
        ok: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        url: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selector: Option<String>,
        /// Base64 de la captura PNG (solo para accion="capturar").
        #[serde(default, skip_serializing_if = "Option::is_none")]
        captura_base64: Option<String>,
        descripcion: String,
    },
    /// [109A-5 F2] Plan visible de la conversación: se emite tras cada acción
    /// de la tool `todo` y también al arrancar un turno que ya tenía plan
    /// vigente (resume entre turnos por conversación). Lleva la lista COMPLETA,
    /// no un delta: el front pinta verdad absoluta y no acumula estados
    /// divergentes. Aditivo: los consumidores que no lo conocen lo ignoran.
    TareasActualizadas { items: Vec<TareaVisible> },
    /// [109A-5 F3] Meta declarada como lograda: cierra el ciclo de vida de la
    /// meta de una conversación. Se emite en el momento de marcarla (no al
    /// cerrar el turno), porque `lograr` puede llegar entre turnos; `turno_id`
    /// es el turno que respalda el logro y ancla el badge al pie de ESE turno.
    /// `elapsed_ms` es el tiempo de persecución NETO de pausas, calculado por
    /// el dominio (`cli/src/servicio/meta.rs`), no por el front. Aditivo.
    MetaLograda {
        meta: String,
        /// RFC 3339 en UTC; el front solo lo muestra en el `title`.
        lograda_en: String,
        elapsed_ms: u64,
        turno_id: Uuid,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /* [109A-5 F2] La UI compara literales (`tareas_actualizadas`, `en_curso`)
     * para decidir qué pintar, así que el nombre serializado es contrato: si
     * alguien renombra una variante o el `rename_all`, este test lo caza antes
     * de que el bloque de tareas deje de aparecer en el transcript. */
    #[test]
    fn tareas_actualizadas_serializa_con_el_contrato_que_espera_la_ui() {
        let evento = AgenteEvento::TareasActualizadas {
            items: vec![
                TareaVisible {
                    id: 1,
                    texto: "leer el plan".into(),
                    estado: EstadoTareaVisible::Completada,
                },
                TareaVisible {
                    id: 2,
                    texto: "escribir la fase".into(),
                    estado: EstadoTareaVisible::EnCurso,
                },
                TareaVisible {
                    id: 3,
                    texto: "verificar".into(),
                    estado: EstadoTareaVisible::Pendiente,
                },
            ],
        };
        let json = serde_json::to_value(&evento).expect("serializa");
        assert_eq!(json["tipo"], "tareas_actualizadas");
        assert_eq!(json["items"][0]["estado"], "completada");
        assert_eq!(json["items"][1]["estado"], "en_curso");
        assert_eq!(json["items"][2]["estado"], "pendiente");
        assert_eq!(json["items"][1]["id"], 2);
        assert_eq!(json["items"][1]["texto"], "escribir la fase");

        let vuelta: AgenteEvento = serde_json::from_value(json).expect("deserializa");
        match vuelta {
            AgenteEvento::TareasActualizadas { items } => {
                assert_eq!(items.len(), 3);
                assert_eq!(items[1].estado, EstadoTareaVisible::EnCurso);
            }
            otro => panic!("variante inesperada: {otro:?}"),
        }
    }

    /* [109A-5 F3] Mismo motivo que el test anterior: la UI localiza el pie del
     * turno por `turno_id` y muestra `elapsed_ms` tal cual, así que estos
     * nombres y el tipo del id (string, no objeto) forman parte del contrato. */
    #[test]
    fn meta_lograda_serializa_con_el_contrato_que_espera_la_ui() {
        let turno = Uuid::new_v4();
        let evento = AgenteEvento::MetaLograda {
            meta: "publicar la fase 3".into(),
            lograda_en: "2026-09-10T12:00:00+00:00".into(),
            elapsed_ms: 96_000,
            turno_id: turno,
        };
        let json = serde_json::to_value(&evento).expect("serializa");
        assert_eq!(json["tipo"], "meta_lograda");
        assert_eq!(json["meta"], "publicar la fase 3");
        assert_eq!(json["elapsed_ms"], 96_000);
        assert_eq!(json["turno_id"], turno.to_string());
        assert_eq!(json["lograda_en"], "2026-09-10T12:00:00+00:00");

        let vuelta: AgenteEvento = serde_json::from_value(json).expect("deserializa");
        match vuelta {
            AgenteEvento::MetaLograda {
                meta,
                elapsed_ms,
                turno_id,
                ..
            } => {
                assert_eq!(meta, "publicar la fase 3");
                assert_eq!(elapsed_ms, 96_000);
                assert_eq!(turno_id, turno);
            }
            otro => panic!("variante inesperada: {otro:?}"),
        }
    }
}
