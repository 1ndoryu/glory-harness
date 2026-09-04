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
/// El runtime lo persiste al finalizar con las métricas de auditoría (el
/// consumidor hace UPSERT contra su tabla `agente_turnos`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnoPersistido {
    pub id: Uuid,
    pub conversacion_id: Uuid,
    pub user_id: Uuid,
    pub estado: String, // "ejecutando" | "ok" | "error" | "cancelado"
    pub resumen: Option<String>,
    pub creado_en: DateTime<Utc>,
    // Métricas de auditoría del turno (sin secretos).
    pub provider: Option<String>,
    pub modelo: Option<String>,
    pub tokens_prompt: u32,
    pub tokens_complecion: u32,
    pub tools_ejecutadas: u32,
    pub duracion_ms: u64,
    pub error: Option<String>,
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

/// [318A-16 F6] Registro completo de una tarea programada (cara CRUD de la
/// tool `programar_tarea` y del subcomando `schedule`). El scheduler solo ve
/// la vista [`TareaProgramadaPendiente`]; este registro añade estado,
/// próxima ejecución y fechas para listar/cancelar/auditar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TareaProgramada {
    pub id: Uuid,
    pub user_id: Uuid,
    pub nombre: String,
    pub prompt: String,
    /// "recurrente" | "una_vez".
    pub tipo: String,
    pub cron_expr: Option<String>,
    /// Próxima ejecución calculada (cron v1/v2); `None` = desprogramada.
    pub proxima_ejecucion: Option<DateTime<Utc>>,
    /// "pendiente" | "ejecutando" | "cancelada" | "completada" | "fallida".
    pub estado: String,
    pub creado_en: DateTime<Utc>,
}

/// [318A-16 F6] Datos para crear una tarea programada. La próxima ejecución
/// la calcula el núcleo (lógica agnóstica de cron) antes de llamar al puerto.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NuevaTareaProgramada {
    pub user_id: Uuid,
    pub nombre: String,
    pub prompt: String,
    pub tipo: String,
    pub cron_expr: String,
    pub proxima_ejecucion: DateTime<Utc>,
}

/// [318A-16 F6] Registro de una ejecución de una tarea programada (log).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogTareaEjecucion {
    pub id: Uuid,
    pub tarea_id: Uuid,
    pub ok: bool,
    pub resumen: String,
    pub ejecutada_en: DateTime<Utc>,
}

/// Acción auditada de un turno (tool ejecutada).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccionAuditable {
    pub turno_id: Uuid,
    pub tool: String,
    pub ok: bool,
    pub resumen: String,
    pub argumentos_json: Option<String>,
    /// [039A-1 04-09 H6] Diff de líneas del cambio (file_write/file_patch)
    /// para repintar el resultado al recargar el historial. `None` si no
    /// aplica. Se persiste fuera del contexto del LLM (auditoría/UI).
    pub diff: Option<String>,
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
    /// Toca la recencia de una conversación (p. ej. al persistir la respuesta
    /// del asistente, para que el orden por `actualizado_en` sea correcto).
    async fn conversacion_tocar(&self, conversacion_id: Uuid) -> Result<()>;

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
    /// Fija la próxima ejecución (`None` desprograma, p. ej. 'una_vez').
    /// El cálculo de la fecha es lógica agnóstica del scheduler del núcleo;
    /// el consumidor solo persiste.
    async fn tarea_reprogramar(
        &self,
        id: Uuid,
        user_id: Uuid,
        proxima: Option<DateTime<Utc>>,
    ) -> Result<()>;
}

/// [318A-16 F6] Puerto CRUD de tareas programadas (tool `programar_tarea` +
/// subcomando CLI `schedule`). Distinto de las operaciones del scheduler en
/// [`AgentPersistence`] (recuperar/tomar/reprogramar): este puerto es la cara
/// de gestión que el agente expone. `None` en el runtime → la tool no se
/// registra (fail-closed, como `EjecutorComando`); PT conserva su CRUD propio
/// y lo cableará aquí en una fase posterior (decisión del plan 318A-16 F6).
#[async_trait]
pub trait ProgramadorTareas: Send + Sync {
    /// Crea una tarea; devuelve su id.
    async fn tarea_crear(&self, nueva: &NuevaTareaProgramada) -> Result<Uuid>;
    /// Lista las tareas del usuario (orden de creación).
    async fn tareas_listar(&self, user_id: Uuid) -> Result<Vec<TareaProgramada>>;
    /// Cancela una tarea del usuario; `false` si no existe o no es suya.
    async fn tarea_cancelar(&self, id: Uuid, user_id: Uuid) -> Result<bool>;
    /// Últimos `limite` registros de ejecución de una tarea del usuario.
    async fn tarea_logs(
        &self,
        id: Uuid,
        user_id: Uuid,
        limite: u32,
    ) -> Result<Vec<LogTareaEjecucion>>;
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

/// Contenido de una página descargada por `web_fetch` (límites ya aplicados
/// por el proveedor: texto acotado a `limite_bytes`, sin binarios).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContenidoWeb {
    pub url: String,
    pub titulo: Option<String>,
    pub texto: String,
    pub bytes: usize,
}

/// [Bloque 3, Fase 1] Proveedor de descarga HTTP aportado por el consumidor
/// (CLI: reqwest). `web_fetch` ≠ `web_search`: descarga UNA url a texto
/// limpio; la búsqueda devuelve resultados. Sin proveedor → error claro,
/// nunca falso éxito.
#[async_trait]
pub trait WebFetchProvider: Send + Sync {
    /// Descarga `url` y devuelve el texto legible acotado a `limite_bytes`.
    async fn obtener(&self, url: &str, limite_bytes: usize) -> Result<ContenidoWeb>;
}

// ---------------------------------------------------------------------------
// Ejecución de comandos (puerto de la tool `comando`, 318A-16 F3)
// ---------------------------------------------------------------------------

/// Resultado de ejecutar un comando (síncrono o de fondo).
#[derive(Debug, Clone, Default)]
pub struct ResultadoEjecucionComando {
    /// Código de salida del proceso (`None` si aún corre o fue matado).
    pub codigo_salida: Option<i32>,
    /// Salida capturada (stdout+stderr), ya truncada por el runner.
    pub salida: String,
    /// La salida fue truncada por el límite del runner (8 KB en el CLI).
    pub truncada: bool,
    /// ¿Corre en background? (el comando devolvió `id_fondo` de inmediato)
    pub fondo: bool,
    /// Id de la tarea de fondo (para `comando_status`/`comando_matar`).
    pub id_fondo: Option<String>,
}

/// Puerto de ejecución de comandos. El núcleo define el contrato; el
/// consumidor aporta el runner real (CLI: timeout, truncado a 8 KB,
/// background con log propio). El runtime SOLO registra la tool `comando`
/// cuando este puerto está presente (fail-closed: sin runner → la tool no
/// existe y el modelo ni la ve).
#[async_trait]
pub trait EjecutorComando: Send + Sync {
    /// Ejecuta un comando. `fondo=true` devuelve de inmediato con `id_fondo`.
    async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando>;
    /// Estado/salida de una tarea de fondo (aún corriendo o final).
    async fn estado(&self, id_fondo: &str) -> Result<ResultadoEjecucionComando>;
    /// Mata una tarea de fondo.
    async fn matar(&self, id_fondo: &str) -> Result<()>;
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