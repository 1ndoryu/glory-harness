//! [Partición límite-500] Estado y tipos del turno: extraídos de `mod.rs`
//! (superaba 500 líneas efectivas). Sin cambios de semántica; `mod.rs`
//! re-exporta estos tipos para que `permisos`/`auditoria` los sigan viendo
//! como `super::`.
//!
//! [Por qué `pub(crate)` en todos los campos]: antes eran privados del árbol
//! `turno` y los tocaban los submódulos hermanos (`permisos`, `auditoria`,
//! `tools`); al vivir ahora en `turno::estado`, los hermanos dejarían de
//! verlos. `pub(crate)` los mantiene internos al crate sin exponerlos fuera.

use serde_json::Value;
use uuid::Uuid;

use crate::llm::AiMessage;

/// [059A-S3] Estado mutable de un turno (extraído de `ejecutar_turno` para
/// acotar las firmas de los helpers de fase).
pub(crate) struct EstadoTurno {
    /* `pub(crate)`: `cierre_wrap_up` vive en `tools.rs` ([129A-2]) y toca
     * estos dos campos; el resto sigue privado del turno. */
    pub(crate) mensajes: Vec<AiMessage>,
    /* [Bloque 3, F1] Cola de respuestas previas del asistente (del historial
     * del consumidor) para el detector de repetición de las guardas. Solo
     * contenido de texto real; tool_calls/Null no cuentan. */
    pub(crate) respuestas_asistente: Vec<String>,
    /* Un solo reintento por turno tras respuesta vacía. */
    pub(crate) ya_reintentado: bool,
    /* [318A-15 F3] Tools denegadas en este turno (por política o por negación
     * del usuario): si el modelo las vuelve a proponer en el MISMO turno, no se
     * re-emite el evento ni se le vuelve a explicar — se le devuelve
     * "denegada" para que cambie de plan (no reintento automático). */
    pub(crate) denegadas_en_turno: std::collections::HashSet<String>,
    pub(crate) tools_ejecutadas: usize,
    /* [29-08-2026] Persistencia de la conversación (Fase 4): la respuesta
     * final del asistente se guarda al terminar el turno para que recargar
     * conserve el historial completo (el mensaje del usuario lo persiste el
     * consumidor antes de llamar). */
    /* [29-08-2026] Persistencia de la conversación (Fase 4): la respuesta
     * final del asistente se guarda al terminar el turno para que recargar
     * conserve el historial completo (el mensaje del usuario lo persiste el
     * consumidor antes de llamar). */
    pub(crate) respuesta_final: Option<String>,
    /* [129A-1] Pensamientos del turno (uno por llamada LLM con
     * `reasoning_content`): se persisten como filas `rol = "reasoning"` para
     * repintar el summary al recargar, sin contaminar el historial que viaja
     * al proveedor (se filtra en `historial_desde_persistencia`). */
    pub(crate) razonamientos: Vec<String>,
    /* [20-09-2026] Sesión LLM estable del turno (= `conversacion_id`): viaja
     * como `sesion_externa` al transporte. El dialecto Responses de OpenCode
     * Go la exige como header `x-opencode-session` (enrutar + cachear entre
     * rondas del mismo turno); el resto de proveedores la ignora. */
    pub(crate) sesion_llm: Uuid,
}

impl EstadoTurno {
    /// [059A-S3] Ensambla el arranque del turno: system + historial + mensaje
    /// del usuario, y precarga la cola de respuestas previas del asistente.
    pub(crate) fn nuevo(
        prompt_sistema: String,
        historial: Vec<AiMessage>,
        mensaje_usuario: String,
        sesion_llm: Uuid,
    ) -> Self {
        let mut mensajes: Vec<AiMessage> = Vec::new();
        mensajes.push(AiMessage::texto("system", prompt_sistema));
        let respuestas_asistente: Vec<String> = historial
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| match &m.content {
                Value::String(t) if !t.trim().is_empty() => Some(t.clone()),
                _ => None,
            })
            .collect();
        mensajes.extend(historial);
        mensajes.push(AiMessage::texto("user", mensaje_usuario));
        EstadoTurno {
            mensajes,
            respuestas_asistente,
            ya_reintentado: false,
            denegadas_en_turno: std::collections::HashSet::new(),
            tools_ejecutadas: 0,
            respuesta_final: None,
            razonamientos: Vec::new(),
            sesion_llm,
        }
    }
}

/// [059A-S3] Resultado de un paso del bucle para que `ejecutar_turno` decida
/// el control (continue/break) sin duplicar la lógica de cada fase.
pub(crate) enum PasoIteracion {
    /// Respuesta final del modelo (texto): terminar el turno con normalidad.
    FinalizarTurno,
    /// Respuesta vacía y queda reintento: seguir el bucle con aviso de sistema.
    ReintentarVacio,
    /// La tool `ask_user` cortó el turno (la pregunta queda pendiente).
    TurnoCortadoPorPregunta,
    /// El SSE se cerró: no seguir ejecutando tools ni consumiendo tokens.
    ConexionCerrada,
    /// Hubo tool_calls procesadas: continuar a la siguiente iteración.
    ProcesarTools,
}

/// [059A-S3] Resultado de procesar una tool individual del lote.
pub(crate) enum PasoTool {
    /// Seguir con la siguiente tool del lote.
    Continua,
    /// El SSE se cerró a mitad de la ejecución.
    CerrarConexion,
    /// `ask_user` emitió la pregunta: el turno termina aquí.
    PreguntaUsuario,
}

/// [109A-4 F4] Petición de un turno: identidad, entrada y política forzada.
/// Agrupa lo que antes eran argumentos posicionales para que el override de
/// modo no añada un séptimo parámetro a `ejecutar_turno_con_modo` (límite de
/// clippy y, sobre todo, llamada legible desde el transporte Tauri).
pub struct PeticionTurno<'a> {
    pub user_id: Uuid,
    pub turno_id: Uuid,
    pub conversacion_id: Uuid,
    pub historial: Vec<AiMessage>,
    pub mensaje_usuario: String,
    /// Modo de ESTE turno; `None` = modo de la sesión.
    pub modo_forzado: Option<&'a str>,
}
