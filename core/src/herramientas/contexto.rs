//! [139A-8 F4/S1] Contexto que recibe cada tool al ejecutarse (extraído de
//! `tool.rs` sin cambios de semántica).
//!
//! El núcleo solo expone puertos (persistencia, búsqueda web, proveedor LLM)
//! y el sandbox de archivos; los servicios de dominio del consumidor viajan
//! en `dominio` (slot opaco que el consumidor downcastea, DIP).

use std::any::Any;
use std::sync::Arc;

use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::evento::AgenteEvento;
use crate::ports::{
    AgentPersistence, NavegadorPort, ProviderPort, WebFetchProvider, WebSearchProvider,
};
use crate::sandbox::SandboxArchivos;
use crate::todo::TodoCompartida;

/// Contexto que recibe cada tool al ejecutarse. El núcleo solo expone puertos
/// (persistencia, búsqueda web, proveedor LLM) y el sandbox de archivos; los
/// servicios de dominio del consumidor viajan en `dominio` (opaco al núcleo).
pub struct AgentToolContext<'a> {
    pub user_id: Uuid,
    /// [109A-2] Ámbito de memoria del turno: las tools `memoria_*` solo leen
    /// y escriben recuerdos de este ámbito (proyecto activo o global).
    /// Default `Global`: un consumidor que no lo fije conserva el
    /// comportamiento previo (memoria del usuario, sin mezclar proyectos).
    pub ambito_memoria: crate::ports::AmbitoMemoria,
    /// Puerto de persistencia (turnos, mensajes, memoria, skills, tareas
    /// programadas). El runtime audita las acciones por aquí.
    pub persistencia: &'a dyn AgentPersistence,
    /// Búsqueda web. `None` si el consumidor no aporta proveedor: las tools
    /// que la necesiten fallan con error claro (nunca falso éxito).
    pub web_search: Option<&'a dyn WebSearchProvider>,
    /// [Bloque 3, F1] Descarga HTTP de una URL (`web_fetch`). Mismo contrato
    /// que `web_search`: `None` → la tool falla con error claro.
    pub web_fetch: Option<&'a dyn WebFetchProvider>,
    /// Proveedor LLM (para tools que necesiten generar texto). `None` igual.
    pub ai_provider: Option<&'a dyn ProviderPort>,
    /// Sandbox de archivos (Fase 2). `None` en producción: las tools de
    /// archivo no existen (fail-closed, ni siquiera admin).
    pub sandbox_archivos: Option<Arc<SandboxArchivos>>,
    /// Slot de extensión para tools de dominio del consumidor: task inyecta
    /// aquí sus servicios (p. ej. `&PgPool` + repos), y sus tools hacen
    /// `downcast_ref`. El núcleo no interpreta este tipo.
    pub dominio: Option<&'a (dyn Any + Send + Sync)>,
    /// Plan `todo` compartido del runtime (318A-15 F5). `None` si el runtime
    /// no registró la tool (no debería pasar: el runtime la crea siempre).
    pub todo: Option<TodoCompartida>,
    /// [318A-16 F5] Store del modo plan: presente SOLO cuando el turno corre
    /// en modo `plan`. Las tools de escritura de archivos registran aquí su
    /// propuesta (diff) en vez de escribir; el resto de consumidores lo
    /// ignoran (`None`).
    pub plan: Option<crate::plan::PlanCompartida>,
    /// [069A-1 F5] Puerto del navegador interno (webview child). `None` →
    /// la tool `navegador_reflejo` falla con error claro.
    pub navegador: Option<&'a dyn NavegadorPort>,
    /// [209A-1 F1] Conversación dueña del turno (`Uuid::nil()` en el hijo
    /// subagente, que no tiene conversación propia): las tools que emiten
    /// eventos en vivo (p. ej. `comando` → consola) la adjuntan para el reap
    /// por conversación de F2.
    pub conversacion_id: Uuid,
    /// [209A-1 F1] Canal de eventos del turno para streaming en vivo
    /// (`comando` emite `ConsolaInicio`/`ConsolaChunk` aquí; el reenvío al
    /// SSE ya existe). `None` en tests: la tool sigue devolviendo el
    /// resultado final sin emitir nada.
    pub tx_eventos: Option<Sender<AgenteEvento>>,
}
