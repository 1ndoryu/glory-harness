//! Memoria de aprendizaje ([069A-4], diseño
//! `Agente/documentacion/memoria-aprendizaje-diseno-2026-09-06.md`):
//! puerto [`ProveedorMemoria`](crate::ports::ProveedorMemoria) (contrato),
//! implementación base sobre `AgentPersistence::memoria_*`, sanitizado de
//! secretos, curador determinista y tools `memoria_*` del agente.
//!
//! Decisiones frente al diseño:
//! - `sync` recibe además `origen` (qué turno o pasada produjo el recuerdo;
//!   el diseño proponía solo `(user_id, resumen)`).
//! - Sin re-scoring con LLM en v1 (diseño §1): la extracción propone el
//!   turno con reglas deterministas (intención explícita) y el curador poda
//!   por edad/uso/duplicados.
//! - Archivo sin tabla propia: el curador marca `origen = "archivada:<fecha>"`
//!   y `prefetch` excluye archivadas (conservadas para auditar/revertir).
//! - El curador corre nativo vía marcador `[curador-memoria]` interceptado
//!   en el motor del cron (cero coste LLM, entrega normal en `tarea_logs`)
//!   o bajo demanda con el subcomando CLI `memoria curar`.
//!
//! [069A-5 F3] Partido desde `memoria.rs` (555 líneas efectivas) en
//! `sanitize` + `proveedor` + `curador` + `tools` (+ `soporte` de tests).

pub mod curador;
pub mod proveedor;
pub mod sanitize;
pub mod tools;

pub use curador::{
    ejecutar_curador, ejecutar_curador_todos, es_peticion_curador, PoliticaCurador,
    ResumenCurador, MARCADOR_CURADOR,
};
pub use proveedor::{extraer_candidatos, puntuar_y_formatear, MemoriaBase};
pub use sanitize::{parece_secreto, sanitize_para_memoria};
pub use tools::{
    registrar_tools_memoria, ToolMemoriaBorrar, ToolMemoriaGuardar, ToolMemoriaRecordar,
};

#[cfg(test)]
pub(crate) mod soporte;
