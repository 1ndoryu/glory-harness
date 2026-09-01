//! Puertos (traits) que definen el núcleo. El consumidor (task, WANDORIUS,
//! scripts) implementa estos traits con su propia persistencia y servicios;
//! el núcleo no sabe quién los implementa (DIP).

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::error::Result;
use crate::evento::TokenStream;

// ---------------------------------------------------------------------------
// Persistencia (puerto del runtime)
// ---------------------------------------------------------------------------

/// Un mensaje del historial de una conversación.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MensajePersistido {
    pub id: Uuid,
    pub conversacion_id: Uuid,
    pub rol: String, // "user" | "assistant"
    pub contenido: String,
    pub creado_en: DateTime<Utc>,
}

/// Metadatos de un turno (una llamada al agente dentro de una conversación).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnoPersistido {
    pub id: Uuid,
    pub conversacion_id: Uuid,
    pub user_id: Uuid,
    pub estado: String, // "ejecutando" | "ok" | "error" | "cancelado"
    pub resumen: Option<String>,
    pub creado_en: DateTime<Utc>,
}

/// Entrada de memoria persistente (clave → contenido).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoriaEntrada {
    pub clave: String,
    pub contenido: String,
}

/// Skill persistente del agente (inyectada al prompt cuando está activa).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillEntrada {
    pub id: Uuid,
    pub nombre: String,
    pub descripcion: String,
    pub instrucciones: String,
    pub activa: bool,
}

/// Tarea programada pendiente de ejecutar (scheduler).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TareaProgramadaPendiente {
    pub id: Uuid,
    pub user_id: Uuid,
    pub nombre: String,
    pub prompt: String,
    pub tipo: String,
    pub cron_expr: Option<String>,
}

/// Acción auditada de un turno (tool ejecutada).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccionAuditable {
    pub turno_id: Uuid,
    pub tool: String,
    pub ok: bool,
    pub resumen: String,
    pub argumentos_json: Option<String>,
}

/// Puerto de persistencia: **todo** acceso a estado durable del agente pasa
/// por aquí. El núcleo nunca escribe por su cuenta; el consumidor es el único
/// dueño de la base de datos (R3 del plan 318A-13).
#[async_trait]
pub trait AgentPersistence: Send + Sync {
    // --- Turnos y mensajes ---
    async fn guardar_turno(&self, turno: &TurnoPersistido) -> Result<()>;
    async fn finalizar_turno(
        &self,
        turno_id: Uuid,
        estado: &str,
        resumen: Option<&str>,
    ) -> Result<()>;
    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> Result<()>;
    /// Historial de una conversación, ordenado por `creado_en` ascendente.
    async fn listar_mensajes(&self, conversacion_id: Uuid) -> Result<Vec<MensajePersistido>>;

    // --- Acciones (auditoría) ---
    async fn registrar_accion(&self, accion: &AccionAuditable) -> Result<()>;

    // --- Memoria ---
    async fn memoria_listar(&self, user_id: Uuid) -> Result<Vec<MemoriaEntrada>>;
    async fn memoria_upsert(&self, user_id: Uuid, entrada: &MemoriaEntrada) -> Result<()>;
    async fn memoria_borrar(&self, user_id: Uuid, clave: &str) -> Result<()>;

    // --- Skills ---
    async fn skills_listar(&self, user_id: Uuid) -> Result<Vec<SkillEntrada>>;

    // --- Tareas programadas (scheduler) ---
    /// Recupera tareas interrumpidas (heartbeat vencido) → 'pendiente'.
    async fn tareas_recuperar_interrumpidas(&self) -> Result<u64>;
    /// Pide las primeras `limite` tareas pendientes que tocan ejecutar.
    async fn tareas_pendientes(&self, limite: u32) -> Result<Vec<TareaProgramadaPendiente>>;
    /// Marca 'ejecutando' de forma atómica; `false` si otra réplica la tomó.
    async fn tarea_tomar(&self, id: Uuid) -> Result<bool>;
    async fn tarea_finalizar(&self, id: Uuid, ok: bool, resumen: Option<&str>) -> Result<()>;
}

// ---------------------------------------------------------------------------
// Búsqueda web (puerto opcional de tool)
// ---------------------------------------------------------------------------

/// Resultado de una búsqueda web.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResultadoWeb {
    pub titulo: String,
    pub url: String,
    pub fragmento: String,
}

/// Puerto de búsqueda web. El consumidor aporta el servicio real (con su
/// proveedor y su límite); el núcleo solo define el contrato.
#[async_trait]
pub trait WebSearchProvider: Send + Sync {
    async fn buscar(&self, query: &str, limite: usize) -> Result<Vec<ResultadoWeb>>;
}

// ---------------------------------------------------------------------------
// Proveedor LLM (puerto del runtime y de las tools)
// ---------------------------------------------------------------------------

/// Configuración efectiva de una llamada de chat (modelo + parámetros).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatRequest {
    pub system: String,
    pub mensajes: Vec<ChatMensaje>,
    pub modelo: String,
    pub temperatura: Option<f32>,
    pub max_tokens: Option<u32>,
    /// Identificador de sesión para el limiter del proveedor (si aplica).
    pub sesion_id: Option<String>,
    #[serde(default)]
    pub extra: HashMap<String, serde_json::Value>,
}

/// Mensaje de chat en el formato neutral del núcleo.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMensaje {
    pub rol: String, // "system" | "user" | "assistant" | "tool"
    pub contenido: String,
}

/// Estadísticas de uso devueltas por el proveedor.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Uso {
    pub prompt_tokens: Option<u32>,
    pub completion_tokens: Option<u32>,
    pub total_tokens: Option<u32>,
}

/// Puerto de proveedor LLM: streaming de tokens sobre un canal tokio.
/// El consumidor (task) implementa este puerto con su proxy de proveedores;
/// el núcleo consume el stream sin saber qué proveedor es.
#[async_trait]
pub trait ProviderPort: Send + Sync {
    async fn chat_stream(
        &self,
        request: ChatRequest,
    ) -> Result<TokenStream>;
}

/// Eventos que puede emitir un proveedor durante el streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
pub enum EventoTurno {
    Token { texto: String },
    Usage { uso: Uso },
    Fin { motivo: String },
}