//! Runtime del agente (plan 318A-13, Fase 1c): port agnóstico de
//! `src/agent/runtime.rs` de task **sin SQL y sin `AppState`**. Todo acceso a
//! estado durable entra por [`AgentPersistence`]; el proveedor LLM es
//! [`LlmProviderService`] (movido al núcleo en Fase 1b).
//!
//! Frontera heredada de task (H2/H3): loop LLM → tools → LLM con límite de
//! turns (configurable), timeout por tool, fallo parcial como resultado de
//! tool (no aborta el turno) y cancelación real cuando el cliente corta el
//! SSE (`tx.is_closed()` → no se siguen ejecutando tools ni se consumen
//! tokens). El contexto de productividad (notas/tareas/hábitos) y la memoria/
//! skills NO se cargan aquí: el consumidor los inyecta en `historial` antes
//! de llamar (son consultas de su dominio; R3: el núcleo nunca persiste por
//! su cuenta).

use serde_json::Value;
use std::any::Any;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::Sender;
use uuid::Uuid;

use crate::context::{AgentContextManager, ContextoConfig};
use crate::error::Result;
use crate::evento::AgenteEvento;
use crate::guardas::{
    aviso_por_repeticion, aviso_vacio, decidir_reintento_vacio, texto_vacio, GuardasTurno,
};
use crate::hooks::{DispatcherHooks, EventoHook, SalidaHook};
use crate::llm::{AiChatOptions, AiMessage, AiToolCall, LlmProviderService};
use crate::memoria::registrar_tools_memoria;
use crate::ports::EjecutorComando;
use crate::ports::{
    AccionAuditable, AgentPersistence, AmbitoMemoria, MensajePersistido, NavegadorPort,
    ProgramadorTareas, TurnoPersistido, WebFetchProvider, WebSearchProvider,
};
use crate::pregunta::{procesar_pregunta, registrar_tool_ask_user};
use crate::sandbox::SandboxArchivos;
use std::collections::HashSet;

use crate::subagente::{
    concurrencia_permitida, perfil_subagente, perfiles_disponibles, presupuesto_efectivo,
    profundidad_permitida, registrar_tool_task, schema_hijo, GuardiaConcurrencia,
    GuardiaProfundidad, PerfilSubagente, CONCURRENTES_MAX_SUBAGENTES, SUBAGENTES_EN_CURSO,
};
use crate::telemetria::{construir_evento, motivo_cierre, TelemetriaTurno};
use crate::todo::registrar_tool_todo;
use crate::tool::{AgentToolContext, AgentToolRegistry};
use crate::tools_archivo::registrar_tools_archivo;
use crate::tools_web::registrar_tools_red;

/* [059A-N S2] Split estructural de runtime.rs: el bucle del turno principal
 * vive en `turno.rs`, la capa de llamada LLM/ejecución de tools en
 * `tools.rs` y las sesiones hijas en `subagente.rs`. Movimiento puro: cada
 * archivo abre su propio `impl AgentRuntime` (los campos privados viven en
 * este módulo; los hijos los ven por privacidad de módulo). */
mod subagente;
mod tools;
mod turno;
/// [109A-4 F4] Petición de turno con política forzada opcional: el transporte
/// la construye para `/meta <texto>` sin tocar el modo de la sesión.
pub use turno::PeticionTurno;

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

pub struct AgentRuntime {
    pub registry: AgentToolRegistry,
    pub contexto: Arc<tokio::sync::Mutex<AgentContextManager>>,
    pub turno_config: TurnoConfig,
    /// [059A-21 M3] Puertos del consumidor agrupados: la misma struct que
    /// recibe `nuevo` (una sola fuente; antes 5 campos sueltos la duplicaban).
    puertos: PuertosHarness,
    /// [318A-15 F4] Profundidad de sesiones hijas activas (máx 1). El schema
    /// del hijo excluye `task` (sin recursión por contrato); el contador es
    /// fail-closed para llamadas directas.
    profundidad_subagente: std::sync::atomic::AtomicU8,
    /// [318A-15 F0] Acumulador de telemetría del turno (interior-mutable;
    /// reseteado al emitir `Telemetria` justo antes de `Done`).
    telemetria: std::sync::Mutex<TelemetriaTurno>,
    /// [318A-15 F2] Reglas del consumidor (AGENTS.md / skills) inyectadas en
    /// la ranura `[REGLAS]`. Interior-mutable: el CLI la fija tras construir
    /// el runtime; vacía por defecto (ranura nunca huérfana).
    reglas: std::sync::Mutex<String>,
    /// [318A-15 F6] ¿Una tool está en curso? La compactación se omite durante
    /// tool_calls largos (ventana de seguridad configurable, item 4).
    tool_en_curso: std::sync::atomic::AtomicBool,
    /// [318A-16 F5] Store del modo plan del turno en curso. `Some` solo tras
    /// empezar un turno con `modo == "plan"`; `None` en el resto. Vive en el
    /// runtime (efímera, nunca en BD): el consumidor la lee tras el turno
    /// para mostrar el diff acumulado (CLI) o descartarla.
    plan_actual: std::sync::Mutex<Option<crate::plan::PlanCompartida>>,
    /// [109A-4 F4] Modo FORZADO para el turno en curso (`/meta <texto>`): un
    /// turno solo-lectura sin tocar el modo de la sesión. `None` = usar
    /// `turno_config.modo`. Interior-mutable porque `ejecutar_turno` toma
    /// `&self`; el guard del turno lo limpia al terminar (también si el
    /// future se cancela o el turno entra en pánico), de modo que nunca
    /// sobrevive a su turno.
    modo_turno: std::sync::Mutex<Option<String>>,
    /// [Bloque 3, F1] Guardas de turno (respuesta vacía → reintento único;
    /// repetición → aviso). Configurables por el consumidor; activas por
    /// defecto. Deterministas y sin I/O (guardas.rs).
    guardas: std::sync::Mutex<GuardasTurno>,
    /// [Bloque 3, F4] Hooks de ciclo de vida configurados (nucleo/hooks.rs):
    /// interior-mutable; vacíos por defecto = emisión no-op. El consumidor
    /// los fija con [`AgentRuntime::set_hooks`] tras construir el runtime.
    hooks: std::sync::Mutex<Arc<DispatcherHooks>>,
}

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

    /// [109A-4 F4] Modo efectivo AHORA: el forzado del turno en curso si lo
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

    /// [109A-4 F4] Fija (o limpia) el modo forzado del turno y devuelve el
    /// guard que lo limpia al soltarse (auxiliar de `ejecutar_turno_con_modo`).
    fn guarda_modo_turno(&self, modo: Option<&str>) -> GuardaModoTurno<'_> {
        *self.modo_turno.lock().unwrap_or_else(|p| p.into_inner()) =
            modo.map(str::to_string);
        GuardaModoTurno { runtime: self }
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
    pub fn responder_aprobacion(
        &self,
        id: &str,
        respuesta: crate::aprobacion::RespuestaAprobacion,
    ) -> std::result::Result<(), String> {
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

    /// [318A-15 F0] Acceso a la telemetría tolerante a envenenamiento:
    /// un panic en otro hilo no debe abortar el turno (la telemetría nunca
    /// debe poder romper la ejecución — es observación, no contrato).
    fn telemetria(&self) -> std::sync::MutexGuard<'_, TelemetriaTurno> {
        self.telemetria.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// [318A-15 F1/F2] Ensambla el system prompt de capas para el turno actual
    /// (base estática → ranura [REGLAS] con las reglas del consumidor → bloque
    /// [ENTORNO] con la fecha real).
    fn prompt_sistema(&self) -> String {
        let reglas = self
            .reglas
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        ensamblar_prompt_sistema(&self.turno_config, &reglas, &fecha_hoy())
    }

    /// Dispara un hook y devuelve su salida completa para los eventos que
    /// tienen un canal de ajuste. Los consumidores existentes que solo
    /// necesitan veto usan [`Self::disparar_hook`].
    async fn disparar_hook_con_salida(
        &self,
        evento: EventoHook,
        payload: Value,
    ) -> SalidaHook {
        let hooks = self.hooks.lock().unwrap_or_else(|p| p.into_inner()).clone();
        hooks.disparar_con_salida(evento, payload).await
    }

    async fn disparar_hook(&self, evento: EventoHook, payload: Value) -> bool {
        self.disparar_hook_con_salida(evento, payload).await.bloqueo
    }
}

/* [059A-21] El veredicto de permiso del turno (`VerdictoPermiso` +
 * `decidir_permiso`) vive en `politica::permiso` junto a las demás decisiones
 * de política (una sola casa: permiso por modo, override, reglas, veredicto).
 * Aquí solo se re-exporta para que los flujos del turno (turno/permisos.rs,
 * subagente.rs) lo consuman vía `super::*` sin conocer el detalle. */
pub use crate::politica::permiso::{decidir_permiso, VerdictoPermiso};
/* [059A-21] La maquinaria del prompt por capas (`SYSTEM_PROMPT`,
 * `DesgloseContexto`, `ensamblar_prompt_sistema`, `fecha_hoy`,
 * `workspace_visible`, `info_git`) vive en `nucleo::prompt` (extraida de
 * aqui en 059A-21). El runtime solo la re-exporta para que los flujos del
 * turno (`super::*`) y los consumidores (`crate::runtime::fecha_hoy` en
 * context.rs, `glory_harness_core::runtime::ensamblar_prompt_sistema` en el
 * cli) sigan resolviendo sin conocer el detalle. */
pub(crate) use crate::nucleo::prompt::fecha_hoy;
#[cfg(test)]
pub(crate) use crate::nucleo::prompt::info_git;
pub use crate::nucleo::prompt::{ensamblar_prompt_sistema, DesgloseContexto};

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
    fn no_compactado(motivo: impl Into<String>, tokens_antes: u32, ocupacion_pct: f32) -> Self {
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

fn mensajes_usuario_resumen(mensaje: &str) -> String {
    mensaje.chars().take(500).collect()
}

/// [318A-15 F5] Consigna del wrap-up al agotar `max_turns`: en vez de cortar
/// en seco, el modelo cierra con un resumen estructurado. Se inyecta como
/// mensaje system en la última llamada (sin tools).
const WRAP_UP_TEXTO: &str = "Has agotado el límite de pasos de este turno. NO ejecutes más herramientas.\nCierra con un resumen breve y estructurado:\n- HECHO: qué se completó hasta ahora.\n- PENDIENTE: qué quedó sin hacer y por qué.\n- SIGUIENTE PASO: qué harías si pudieras continuar.\nSi el objetivo ya está cumplido, dilo y resume el resultado.";

#[must_use]
fn wrap_up_instruccion() -> String {
    WRAP_UP_TEXTO.to_string()
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
#[cfg(test)]
mod tests {
    use super::{mensajes_usuario_resumen, DesgloseContexto};
    use crate::context::ContextoConfig;
    use crate::llm::AiMessage;

    #[test]
    fn resumen_acota_prompt() {
        let largo = "x".repeat(2000);
        assert_eq!(mensajes_usuario_resumen(&largo).len(), 500);
    }

    /* [318A-15 F3] Gate de permisos: ask emite la pregunta, deny deniega,
     * y ninguno de los dos se reintenta en el mismo turno (el repetido no
     * vuelve a emitir el evento: el modelo ya fue informado). */
    #[test]
    fn f3_ask_pregunta_y_el_repetido_no_reeventa() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Ask, false),
            VerdictoPermiso::Preguntar
        );
        assert_eq!(
            decidir_permiso(Permiso::Ask, true),
            VerdictoPermiso::RepetidoPregunta
        );
    }

    #[test]
    fn f3_deny_deniega_y_el_repetido_no_reeventa() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Deny, false),
            VerdictoPermiso::Denegar
        );
        assert_eq!(
            decidir_permiso(Permiso::Deny, true),
            VerdictoPermiso::RepetidoDenegado
        );
    }

    #[test]
    fn f3_allow_ejecuta_siempre() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Allow, false),
            VerdictoPermiso::Ejecutar
        );
        assert_eq!(
            decidir_permiso(Permiso::Allow, true),
            VerdictoPermiso::Ejecutar
        );
    }

    /* [318A-15 F5] El wrap-up al agotar `max_turns` cierra con estructura
     * (hecho / pendiente / siguiente) y prohíbe seguir ejecutando tools. */
    #[test]
    fn wrap_up_pide_cierre_estructurado_sin_tools() {
        use super::{wrap_up_instruccion, WRAP_UP_TEXTO};
        let consigna = wrap_up_instruccion();
        assert_eq!(consigna, WRAP_UP_TEXTO);
        for eje in ["HECHO", "PENDIENTE", "SIGUIENTE PASO"] {
            assert!(
                consigna.contains(eje),
                "la consigna de cierre cubre el eje {eje}"
            );
        }
        assert!(consigna.contains("NO ejecutes más herramientas"));
    }

    fn config_prueba() -> ContextoConfig {
        ContextoConfig {
            max_ventana: 128_000,
            reserva_salida: 20_000,
            umbral: 0.5,
            cola_verbatim: 0.025,
            umbral_piso: 0.75,
            umbral_degenerado: 0.85,
            ..ContextoConfig::default()
        }
    }

    fn mensaje(rol: &str, texto: impl Into<String>) -> AiMessage {
        AiMessage::texto(rol, texto)
    }

    /* [318A-7] Tests del desglose de contexto emitido en cada llamada LLM:
     * separa system / definiciones de tools / mensajes / resultados de tools
     * y calcula la ocupación contra la ventana efectiva. */

    #[test]
    fn desglose_separa_secciones() {
        /* "aaaa" = 4 chars = 1 token; "bbbbbbbb" = 8 chars = 2 tokens. */
        let mensajes = vec![
            mensaje("system", "aaaa"),
            mensaje("user", "bbbbbbbb"),
            mensaje("assistant", "bbbbbbbb"),
            mensaje("tool", "aaaa"),
        ];
        /* JSON serializado: {"name":"aaaa"} = 14 chars = 4 tokens. */
        let schemas = vec![serde_json::json!({"name": "aaaa"})];
        let desglose = DesgloseContexto::calcular(&mensajes, &schemas, &config_prueba());

        assert_eq!(desglose.system_instrucciones, 1);
        assert_eq!(desglose.mensajes, 4); // user 2 + assistant 2
        assert_eq!(desglose.resultados_tools, 1);
        assert_eq!(desglose.definiciones_tools, 4);
        assert_eq!(desglose.total_entrada, 10);
        assert_eq!(desglose.max_ventana, 128_000);
        assert_eq!(desglose.reserva_salida, 20_000);
    }

    #[test]
    fn desglose_calcula_ocupacion_sobre_ventana_efectiva() {
        /* Ventana efectiva = 128_000 − 20_000 = 108_000. 10_800 tokens = 10%. */
        let mensajes = vec![mensaje("system", "a".repeat(43_200))]; // 10_800 tokens
        let desglose = DesgloseContexto::calcular(&mensajes, &[], &config_prueba());

        assert_eq!(desglose.total_entrada, 10_800);
        assert!(
            (desglose.ocupacion_pct - 10.0).abs() < 0.001,
            "esperado 10%, got {}",
            desglose.ocupacion_pct
        );
    }

    #[test]
    fn desglose_sin_mensajes_es_cero() {
        let desglose = DesgloseContexto::calcular(&[], &[], &config_prueba());
        assert_eq!(desglose.total_entrada, 0);
        assert_eq!(desglose.ocupacion_pct, 0.0);
    }

    /* [318A-15 F1] Tests del prompt por capas. `ensamblar_prompt_sistema`
     * recibe la fecha como parámetro para que las aserciones sean
     * deterministas (el E2E no depende del proveedor ni del reloj). */

    use super::{ensamblar_prompt_sistema, info_git, TurnoConfig};
    use crate::context::{CIERRE_ENTORNO, CIERRE_REGLAS, MARCA_ENTORNO, MARCA_REGLAS};

    fn config_con_workspace(workspace: Option<&str>) -> TurnoConfig {
        TurnoConfig {
            workspace: workspace.map(str::to_owned),
            ..TurnoConfig::default()
        }
    }

    /// Directorio temporal único por test (bajo el temp del sistema), para
    /// ejercitar la detección git sin tocar el árbol del proyecto.
    fn dir_temporal(nombre: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-f1-{}-{}-{nombre}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    #[test]
    fn prompt_fixture_turno_contiene_fecha_workspace_y_marcadores() {
        /* Fixture del E2E: un turno que inyecta reglas en la ranura y un
         * workspace real; el prompt ensamblado debe llevar fecha, workspace,
         * modelo y AMBOS marcadores, con el bloque de entorno cerrado. */
        let config = config_con_workspace(Some("C:/workspace/fixture-proyecto"));
        let reglas = "Regla de prueba: los cambios se describen en español.";
        let prompt = ensamblar_prompt_sistema(&config, reglas, "2026-09-03");

        assert!(prompt.contains(MARCA_ENTORNO), "marca [ENTORNO] presente");
        assert!(
            prompt.contains(CIERRE_ENTORNO),
            "cierre [/ENTORNO] presente"
        );
        assert!(prompt.contains("Fecha: 2026-09-03"), "fecha inyectada");
        assert!(
            prompt.contains("Workspace: C:/workspace/fixture-proyecto"),
            "workspace inyectado"
        );
        assert!(
            prompt.contains(MARCA_REGLAS),
            "marca [REGLAS] presente con contenido"
        );
        assert!(prompt.contains(CIERRE_REGLAS), "cierre [/REGLAS] presente");
        assert!(prompt.contains(reglas), "contenido de reglas presente");
        assert!(
            prompt.contains("Modelo activo"),
            "modelo activo en el entorno"
        );
        assert!(
            prompt.contains("Git: no"),
            "sin repo en el fixture → Git: no"
        );
    }

    #[test]
    fn capa_reglas_vacia_no_deja_marcador_huerfano() {
        let config = config_con_workspace(None);
        let prompt = ensamblar_prompt_sistema(&config, "   ", "2026-09-03");

        assert!(prompt.contains(MARCA_ENTORNO));
        assert!(
            !prompt.contains(MARCA_REGLAS),
            "sin [REGLAS] huérfano cuando la capa está vacía"
        );
        assert!(!prompt.contains(CIERRE_REGLAS));
        assert!(
            prompt.contains("Workspace: (no disponible)"),
            "sin workspace no se inventa una ruta (no cae al cwd del proceso)"
        );
    }

    #[test]
    fn capas_en_orden_estatico_luego_dinamico() {
        let config = config_con_workspace(Some("C:/workspace/x"));
        let prompt = ensamblar_prompt_sistema(&config, "una regla", "2026-09-03");

        let base = prompt.find("DIRECTRICES:").expect("capa base presente");
        let reglas = prompt.find(MARCA_REGLAS).expect("ranura reglas presente");
        let entorno = prompt.find(MARCA_ENTORNO).expect("entorno presente");
        let modelo = prompt.find("Modelo activo").expect("modelo presente");
        assert!(
            base < reglas && reglas < entorno && entorno < modelo,
            "orden base → reglas → entorno"
        );
    }

    #[test]
    fn prompt_sistema_del_runtime_lleva_fecha_iso() {
        /* El camino real del turno usa `fecha_hoy()`; verificamos el formato
         * sin depender del reloj (determinismo): "Fecha: AAAA-MM-DD". */
        let config = config_con_workspace(None);
        let fecha = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let prompt = ensamblar_prompt_sistema(&config, "", &fecha);
        assert!(prompt.contains(&format!("Fecha: {fecha}")));
    }

    #[test]
    fn info_git_detecta_rama_y_ausencia_de_repo() {
        let repo = dir_temporal("git-rama");
        std::fs::create_dir_all(repo.join(".git")).expect("crear .git");
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n")
            .expect("escribir HEAD");
        let rama = info_git(repo.to_str().expect("ruta utf8"));
        assert_eq!(rama.as_deref(), Some("main"));
        std::fs::remove_dir_all(&repo).ok();

        let sin_repo = dir_temporal("sin-repo");
        let rama = info_git(sin_repo.to_str().expect("ruta utf8"));
        assert_eq!(rama, None);
        std::fs::remove_dir_all(&sin_repo).ok();
    }

    /* [109A-4 F4] `/meta <texto>`: UN turno solo-lectura. El modo forzado vive
     * en el runtime (no en la config de la sesión) y el guard del turno lo
     * limpia al salir, así que no se filtra al turno siguiente. */

    /// Runtime mínimo para tests del override: sin tools de dominio (las
    /// agnósticas de memoria/web las añade `nuevo`), LLM sin claves (no se
    /// llama) y persistencia en memoria.
    fn runtime_de_prueba(modo: &str) -> super::AgentRuntime {
        use crate::llm::{LlavesProveedor, LlmProviderService};
        use crate::nucleo::memoria::soporte::TiendaPrueba;
        use crate::tool::AgentToolRegistry;
        use std::sync::Arc;

        super::AgentRuntime::nuevo(
            AgentToolRegistry::new(),
            super::PuertosHarness {
                persistencia: Arc::new(TiendaPrueba::default()),
                llm: Arc::new(LlmProviderService::new(LlavesProveedor::from_env())),
                web_search: None,
                web_fetch: None,
                dominio: None,
                ejecutor_comando: None,
                programador_tareas: None,
                navegador: None,
            },
            super::TurnoConfig {
                modo: modo.into(),
                ..super::TurnoConfig::default()
            },
        )
    }

    #[test]
    fn f4_modo_forzado_no_toca_el_modo_de_la_sesion() {
        let runtime = runtime_de_prueba("predeterminado");
        assert_eq!(runtime.modo_efectivo(), "predeterminado");

        let guarda = runtime.guarda_modo_turno(Some("meta"));
        assert_eq!(runtime.modo_efectivo(), "meta");
        /* El modo de la sesión sigue intacto: el override es del turno. */
        assert_eq!(runtime.turno_config.modo, "predeterminado");
        drop(guarda);

        /* Soltado el guard (fin, error o cancelación del turno) el runtime
         * vuelve al modo de la sesión: nada se filtra al turno siguiente. */
        assert_eq!(runtime.modo_efectivo(), "predeterminado");

        /* Un turno normal posterior tampoco hereda un override previo. */
        let guarda = runtime.guarda_modo_turno(Some("meta"));
        drop(guarda);
        let guarda = runtime.guarda_modo_turno(None);
        assert_eq!(runtime.modo_efectivo(), "predeterminado");
        drop(guarda);
    }

    #[test]
    fn f4_modo_forzado_deniega_efectos_y_deja_leer() {
        use crate::permiso::Permiso;

        let runtime = runtime_de_prueba("autonomo");
        /* Sin override manda la sesión (autónomo): no se pregunta nada. */
        assert_eq!(
            runtime
                .registry
                .permiso_para("memoria_guardar", &runtime.modo_efectivo()),
            Permiso::Allow
        );

        let _guarda = runtime.guarda_modo_turno(Some("meta"));
        let modo = runtime.modo_efectivo();

        /* Con efecto → deny, y la tool NI SE OFRECE en el schema del turno
         * (deny silencioso: el modelo no la ve en ese turno). */
        assert_eq!(
            runtime.registry.permiso_para("memoria_guardar", &modo),
            Permiso::Deny
        );
        assert!(runtime.registry.esta_denegada("memoria_guardar", &modo));
        let nombres: Vec<String> = runtime
            .registry
            .schemas_openai(None, &modo)
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(
            !nombres.iter().any(|n| n == "memoria_guardar"),
            "una tool con efecto no debe verse en un turno forzado a meta: {nombres:?}"
        );

        /* Sin efecto → allow, y sigue disponible para que el turno pueda
         * leer y responder. */
        assert_eq!(
            runtime.registry.permiso_para("memoria_recordar", &modo),
            Permiso::Allow
        );
        assert!(nombres.iter().any(|n| n == "memoria_recordar"));
    }
}
