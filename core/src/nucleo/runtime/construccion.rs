//! [139A-8 F4/S2] Construcción del runtime (movimiento puro desde `mod.rs`):
//! configuración por turno (`TurnoConfig`), puertos del consumidor
//! (`PuertosHarness`) y `AgentRuntime::nuevo` con el registro de tools
//! agnósticas. Sin cambios de semántica.

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use crate::context::{AgentContextManager, ContextoConfig};
use crate::guardas::GuardasTurno;
use crate::hooks::DispatcherHooks;
use crate::llm::LlmProviderService;
use crate::memoria::registrar_tools_memoria;
use crate::ports::EjecutorComando;
use crate::ports::{
    AgentPersistence, AmbitoMemoria, NavegadorPort, ProgramadorTareas, WebFetchProvider,
    WebSearchProvider,
};
use crate::pregunta::registrar_tool_ask_user;
use crate::sandbox::SandboxArchivos;
use crate::subagente::registrar_tool_task;
use crate::telemetria::TelemetriaTurno;
use crate::todo::registrar_tool_todo;
use crate::tool::AgentToolRegistry;
use crate::tools_archivo::registrar_tools_archivo;
use crate::tools_web::registrar_tools_red;

use super::{AgentRuntime, PlanesConversacion};

/// Configuración por turno del runtime (mismo contrato que task: el front la
/// persiste por conversación y viaja aislada entre tabs).
#[derive(Debug, Clone)]
pub struct TurnoConfig {
    pub provider: String,
    pub modelo: String,
    pub temperatura: f32,
    pub max_tokens: u32,
    pub idioma: String,
    pub incluir_notas: bool,
    pub incluir_tareas_completadas: bool,
    pub incluir_habitos_pausados: bool,
    pub permitir_busqueda_web: bool,
    pub permitir_recordatorios: bool,
    pub prompt_sistema: String,
    pub incluir_memoria: bool,
    pub incluir_skills: bool,
    pub max_turns: usize,
    pub timeout_tool: Duration,
    pub contexto: ContextoConfig,
    /// Modo de operación (sección 9.2): predeterminado | meta | autonomo.
    pub modo: String,
    /// [109A-2] Área de trabajo del turno para la memoria: las tools
    /// `memoria_*` y el prefetch/sync operan SOLO sobre este ámbito. Default
    /// `Global` (usuario sin área activa), que es el comportamiento previo a
    /// la feature: el consumidor que conozca el área activa debe fijarlo.
    pub ambito_memoria: AmbitoMemoria,
    /// [02-09-2026] Fase 5: estilo de respuesta (conciso|detallado|amable) y
    /// preferencias personales del usuario; ambos se inyectan en el prompt.
    pub estilo: String,
    pub preferencias: String,
    /// [02-09-2026] Fase 5: raíz del workspace SOLO en AGENTE_MODO=local
    /// (dev). None → AGENTE_WORKSPACE_ROOT env o cwd. En prod se ignora.
    pub workspace: Option<String>,
    /// [318A-10 02-09-2026] Nivel de razonamiento del modelo
    /// (low|medium|high). None = proveedor usa su default. Se envía como
    /// `reasoning_effort` a los proveedores que lo aceptan.
    pub nivel_razonamiento: Option<String>,
}

impl Default for TurnoConfig {
    fn default() -> Self {
        Self {
            /* [29-08-2026] Default del agente: Glory API sin key (free.empero.org),
             * modelo `commandcode` (la ruta "auto" que resuelve a DeepSeek Flash —
             * la vía que el usuario prefiere porque siempre funciona). Glory va
             * primero; el fallback global solo se usa si Glory falla. */
            provider: "glory".into(),
            modelo: "commandcode".into(),
            temperatura: 0.2,
            max_tokens: 2048,
            idioma: "es".into(),
            incluir_notas: false,
            incluir_tareas_completadas: false,
            incluir_habitos_pausados: false,
            permitir_busqueda_web: true,
            permitir_recordatorios: true,
            prompt_sistema: String::new(),
            incluir_memoria: true,
            incluir_skills: true,
            max_turns: 10,
            timeout_tool: Duration::from_secs(15),
            contexto: ContextoConfig::default(),
            modo: "predeterminado".into(),
            /* [109A-2] Sin ámbito explícito la memoria es global del usuario:
             * es el comportamiento que había antes de separar por proyecto. */
            ambito_memoria: AmbitoMemoria::default(),
            estilo: "conciso".into(),
            preferencias: String::new(),
            workspace: None,
            nivel_razonamiento: None,
        }
    }
}

/// Puertos que el consumidor inyecta al runtime (plan §6.3: `AgentRuntime::
/// nuevo(registro, puertos, config)`). El consumidor construye el registro
/// con sus tools de dominio ANTES de llamar a `nuevo`; el runtime añade las
/// tools agnósticas del núcleo (web + archivo si hay sandbox local).
pub struct PuertosHarness {
    /// Toda persistencia del turno (auditoría, mensajes, recencia).
    pub persistencia: Arc<dyn AgentPersistence>,
    /// Proveedor LLM (movido al núcleo en Fase 1b).
    pub llm: Arc<LlmProviderService>,
    /// Búsqueda web agnóstica. `None` si el consumidor no aporta proveedor:
    /// la tool `web_search` falla con error claro (nunca falso éxito).
    pub web_search: Option<Arc<dyn WebSearchProvider>>,
    /// [Bloque 3, F1] Descarga HTTP (`web_fetch`). `None` → la tool no
    /// disponible con error claro.
    pub web_fetch: Option<Arc<dyn WebFetchProvider>>,
    /// Slot de extensión para las tools de dominio del consumidor (opaco al
    /// núcleo; task inyecta aquí sus repos/servicios y sus tools hacen
    /// `downcast_ref`).
    pub dominio: Option<Arc<dyn Any + Send + Sync>>,
    /// [318A-16 F3] Runner de comandos del consumidor. `None` → la tool
    /// `comando` NO se registra (fail-closed: el modelo ni la ve; PT lo deja
    /// en None por invariante).
    pub ejecutor_comando: Option<Arc<dyn EjecutorComando>>,
    /// [318A-16 F6] Puerto CRUD de tareas programadas. `None` → la tool
    /// `programar_tarea` NO se registra (fail-closed: el agente no programa
    /// desde la conversación si el consumidor no gestiona tareas; PT tiene su
    /// CRUD propio y lo cableará aquí en una fase posterior).
    pub programador_tareas: Option<Arc<dyn ProgramadorTareas>>,
    /// [069A-1 F5] Puerto del navegador interno (webview child). `None` →
    /// la tool `navegador_reflejo` NO se registra (fail-closed).
    pub navegador: Option<Arc<dyn NavegadorPort>>,
}

impl AgentRuntime {
    /// Construye el runtime con los puertos del consumidor. El `registry`
    /// puede traer ya las tools de dominio; aquí se añaden las agnósticas
    /// (web_search siempre; file_* solo con sandbox local, fail-closed).
    #[must_use]
    pub fn nuevo(
        mut registry: AgentToolRegistry,
        puertos: PuertosHarness,
        turno_config: TurnoConfig,
    ) -> Self {
        registrar_tools_red(&mut registry);
        /* [318A-15 F5] Tool `todo` (plan visible) siempre disponible: es
         * agnóstica y efímera (la store vive en este runtime, nunca en BD). */
        registrar_tool_todo(&mut registry);
        /* [318A-15 F4] Tool `task` (subagente): siempre disponible; el
         * runtime la intercepta en el bucle y ejecuta la sesión hija
         * (`ejecutar_subagente`). */
        registrar_tool_task(&mut registry);
        /* [069A-4] Tools `memoria_*` (guardar/recordar/borrar): siempre
         * disponibles; la persistencia es puerto obligatorio y la escritura
         * pasa por el sanitizado (los secretos se rechazan, nunca se
         * guardan). */
        registrar_tools_memoria(&mut registry);
        /* [Bloque 3, F1] Tool `ask_user`: siempre disponible; el runtime la
         * intercepta en el bucle (patrón `task`) y termina el turno tras
         * emitir el evento `Pregunta`. */
        registrar_tool_ask_user(&mut registry);
        /* [318A-16 F3] Tool `comando` SOLO con runner inyectado (fail-closed:
         * sin ejecutor, el modelo no ve la tool). */
        if let Some(ejecutor) = puertos.ejecutor_comando.clone() {
            crate::comando::registrar_tools_comando(&mut registry, ejecutor);
        }
        /* [318A-16 F6] Tool `programar_tarea` SOLO con puerto de gestión
         * inyectado (fail-closed: sin ProgramadorTareas el modelo no la ve).
         * Solo el agente principal: los perfiles de subagente no la incluyen. */
        if let Some(programador) = puertos.programador_tareas.clone() {
            crate::tareas::registrar_tool_programar_tarea(&mut registry, programador);
        }
        /* [069A-1 F5] Tool `navegador_reflejo` SOLO con puerto inyectado.
         * Fail-closed: sin NavegadorPort el modelo no ve la tool. */
        if puertos.navegador.is_some() {
            registry.registrar(Box::new(crate::navegador::ToolNavegadorReflejo));
        }
        /* [29-08-2026] Fase 2: tools de archivo SOLO en AGENTE_MODO=local.
         * Fail-closed: si el sandbox no se puede construir (raíz inválida o
         * modo no-local), no se registran y el contexto va sin sandbox. */
        if let Some(sandbox) = sandbox_desde_entorno(turno_config.workspace.as_deref()) {
            registrar_tools_archivo(&mut registry, Some(sandbox));
        }
        Self {
            registry,
            contexto: Arc::new(tokio::sync::Mutex::new(AgentContextManager::new(
                turno_config.contexto.clone(),
            ))),
            turno_config,
            puertos,
            profundidad_subagente: std::sync::atomic::AtomicU8::new(0),
            telemetria: std::sync::Mutex::new(TelemetriaTurno::nuevo()),
            reglas: std::sync::Mutex::new(String::new()),
            tool_en_curso: std::sync::atomic::AtomicBool::new(false),
            plan_actual: std::sync::Mutex::new(None),
            planes: std::sync::Mutex::new(PlanesConversacion::default()),
            modo_turno: std::sync::Mutex::new(None),
            guardas: std::sync::Mutex::new(GuardasTurno::default()),
            hooks: std::sync::Mutex::new(Arc::new(DispatcherHooks::vacia())),
        }
    }

    /// [Bloque 3, F1] Guardas de turno activas (respuesta vacía y
    /// repetición). El consumidor puede afinarlas o desactivarlas; el
    /// comportamiento por defecto no cambia los contratos previos.
    #[must_use]
    pub fn guardas(&self) -> GuardasTurno {
        *self.guardas.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn set_guardas(&self, guardas: GuardasTurno) {
        *self.guardas.lock().unwrap_or_else(|p| p.into_inner()) = guardas;
    }

    /// [318A-15 F2] Fija las reglas del consumidor (contenido de AGENTS.md o
    /// skills) que se inyectan en la ranura `[REGLAS]` del system prompt.
    pub fn establecer_reglas(&self, reglas: impl Into<String>) {
        *self.reglas.lock().unwrap_or_else(|p| p.into_inner()) = reglas.into();
    }

    /// [Bloque 3, F4] Configura los hooks de ciclo de vida (command/http).
    /// Sin llamada el runtime queda sin hooks (emisión no-op, ningún cambio
    /// de comportamiento); `DispatcherHooks::vacia()` los limpia.
    pub fn set_hooks(&self, hooks: DispatcherHooks) {
        *self.hooks.lock().unwrap_or_else(|p| p.into_inner()) = Arc::new(hooks);
    }
}

/// [29-08-2026] Fase 2: construye el sandbox de archivos desde el entorno.
/// Solo AGENTE_MODO=local; la raíz viene del override de la conversación, de
/// AGENTE_WORKSPACE_ROOT (o el cwd como fallback para dev). Fail-closed:
/// cualquier error → None (sin tools). El override nunca aplica en prod porque
/// este gate exige AGENTE_MODO=local.
fn sandbox_desde_entorno(workspace: Option<&str>) -> Option<Arc<SandboxArchivos>> {
    if std::env::var("AGENTE_MODO").as_deref() != Ok("local") {
        return None;
    }
    let raiz = workspace
        .map(str::trim)
        .filter(|r| !r.is_empty())
        .map(str::to_owned)
        .or_else(|| {
            std::env::var("AGENTE_WORKSPACE_ROOT")
                .ok()
                .filter(|r| !r.trim().is_empty())
        })
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    match SandboxArchivos::nuevo(&raiz) {
        Ok(sandbox) => Some(Arc::new(sandbox)),
        Err(error) => {
            tracing::warn!(%error, "AGENTE_MODO=local pero el workspace no es accesible; tools de archivo desactivadas");
            None
        }
    }
}
