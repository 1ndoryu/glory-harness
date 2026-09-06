//! Subcomando `glory-harness run` (Fase 3): responde un turno por CLI.
//!
//! Construye el runtime del núcleo con la persistencia en memoria y el
//! proveedor LLM cargado de las envs (`LlmProviderService::new(
//! LlavesProveedor::from_env())`), ejecuta un turno y vuelca la respuesta de
//! texto a stdout. El contrato de eventos es el mismo `AgenteEvento` de H3,
//! así que el resultado es idéntico al que vería el frontend de task vía SSE.
//!
//! [02-09-2026] Modo "trabaja en la carpeta donde lo ejecutas": `run` usa por
//! defecto el modelo gratuito Laguna S 2.1 (`commandcode` /
//! `poolside/laguna-s-2.1-free`) con la raíz del workspace = cwd actual (o la
//! de `--dir`), activando las tools de archivo en esa carpeta si
//! `AGENTE_MODO=local`. Si el proveedor falla, el núcleo salta solo a la
//! cadena de respaldo (gloryapi/auto, DeepSeek directo, etc.).

use std::path::PathBuf;
use std::sync::Arc;

use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::{LlavesProveedor, LlmProviderService};
use glory_harness_core::ports::NavegadorPort;
use glory_harness_core::runtime::{AgentRuntime, PuertosHarness, TurnoConfig};
use glory_harness_core::tool::AgentToolRegistry;
use glory_harness_core::{AgentPersistence, ProgramadorTareas};

use crate::persistencia::PersistenciaMemoria;
use crate::persistencia_sqlite::PersistenciaSqlite;

/// Opciones del subcomando `run`.
#[derive(Clone, Default)]
pub struct OpcionesRun {
    /// Proveedor LLM. `None` → `commandcode` (Laguna S 2.1 free).
    pub provider: Option<String>,
    /// Modelo LLM. `None` → `poolside/laguna-s-2.1-free`.
    pub modelo: Option<String>,
    /// Raíz del workspace. `None` → cwd actual (trabaja donde se ejecuta).
    pub dir: Option<PathBuf>,
    /// [318A-16 F5] Modo del turno: `predeterminado` (default), `meta`,
    /// `autonomo` o `plan` (propuesta: diff sin aplicar hasta aprobación).
    pub modo: Option<String>,
    /// [039A-1 04-09 H7] Nivel de razonamiento del modelo (low|medium|high).
    /// `None` → el proveedor usa su default. Se aplica a `TurnoConfig.
    /// nivel_razonamiento` y viaja como `reasoning_effort` a los proveedores
    /// que lo aceptan (deepseek/groq/cerebras/glory).
    pub razonamiento: Option<String>,
    /// [039A-3 P6-backend] Ventana de contexto inyectada por el consumidor
    /// (desktop: config `contexto_max_ventana`, default 150k). `None` (o bajo
    /// `VENTANA_MINIMA`) → default del core (128k, intacto para task/CLI).
    /// Se aplica ANTES de construir el runtime: el `AgentContextManager` y el
    /// desglose del turno clonan esta config al construir (una sola fuente).
    pub max_ventana: Option<u32>,
    /// [069A-3] Toast de Windows al terminar el turno o al pedir un permiso
    /// (`--notificar`). Solo CLI interactivo; sin flag no se registra ningún
    /// hook (emisión no-op como antes).
    pub notificar: bool,
    /// [069A-1 F5] Puerto del navegador interno. `None` → la tool no se
    /// registra (fail-closed). Solo el escritorio inyecta un valor real.
    pub navegador: Option<Arc<dyn NavegadorPort>>,
}

impl std::fmt::Debug for OpcionesRun {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcionesRun")
            .field("provider", &self.provider)
            .field("modelo", &self.modelo)
            .field("dir", &self.dir)
            .field("modo", &self.modo)
            .field("razonamiento", &self.razonamiento)
            .field("max_ventana", &self.max_ventana)
            .field("notificar", &self.notificar)
            .field(
                "navegador",
                &self
                    .navegador
                    .as_ref()
                    .map(|_| "Some(Arc<dyn NavegadorPort>)"),
            )
            .finish()
    }
}

/// [039A-3 P6-backend] Piso de sanidad para ventanas inyectadas por el
/// consumidor: por debajo se ignora (fail-closed al default del core).
pub const VENTANA_MINIMA: u32 = 10_000;

/// Resultado de un turno one-shot, listo para imprimir.
pub struct SalidaTurno {
    pub texto: String,
    pub tools: Vec<String>,
    pub ok: bool,
}

/// Harness compartido del CLI (run/chat/tui): runtime del núcleo con la misma
/// construcción — persistencia en memoria, proveedor LLM de las envs,
/// workspace = cwd (o `--dir`) y `AGENTE_MODO=local` para las tools de archivo.
/// [318A-13] Un único constructor para los tres subcomandos, sin duplicar.
pub struct HarnessCli {
    pub runtime: Arc<AgentRuntime>,
    /// Persistencia inyectable (B5): memoria por defecto; la app de escritorio
    /// pasa SQLite. Los consumidores solo usan el trait, nunca el concreto…
    /// salvo el propio CLI para gestionar conversaciones (`session`, `chat`),
    /// que necesita la cara CRUD fuera del puerto: la guarda `sqlite` cuando
    /// el harness se construyó durable ([069A-2]).
    pub persistencia: Arc<dyn AgentPersistence>,
    pub sqlite: Option<Arc<PersistenciaSqlite>>,
    pub user_id: Uuid,
    pub workspace: Option<PathBuf>,
    pub config: TurnoConfig,
}

/// Construye el harness con los servidores MCP declarados en la config
/// (`GLORY_MCP_CONFIG`, JSON `[{nombre, comando, argumentos}]`). Async porque
/// cada servidor se spawna y se negocia `initialize` + `tools/list` (Bloque 3
/// Fase 2). Fail-closed: un servidor que no arranca o no responde en el
/// timeout aborta el arranque con el error (nunca éxito falso).
pub async fn construir_harness(opciones: &OpcionesRun) -> Result<HarnessCli, String> {
    let persistencia = Arc::new(PersistenciaMemoria::nuevo());
    // Añadir una skill base para dar contexto útil (standalone sin BD).
    let user_id = Uuid::new_v4();
    persistencia.con_skills_base(user_id);
    let mut registry = AgentToolRegistry::new();
    crate::mcp_cli::registrar_desde_env(&mut registry).await?;
    let mut harness = construir_harness_con_impl(
        opciones,
        persistencia,
        Arc::new(crate::persistencia::ProgramadorMemoria::nuevo()),
        user_id,
        registry,
    );
    harness.sqlite = None;
    Ok(harness)
}

/// [069A-2] Harness durable para `chat`/`tui`/`session`: la misma BD sqlite
/// del CLI (`abrir_tiendas_durables`, `user_id` estable) como persistencia del
/// runtime, de modo que turnos, mensajes y conversaciones sobreviven a los
/// procesos y `session resume` recompone el contexto. Sin directorio de app
/// (APPDATA/HOME ausente) falla con error presentable (fail-closed: no se
/// finge durabilidad con memoria).
pub async fn construir_harness_durable(opciones: &OpcionesRun) -> Result<HarnessCli, String> {
    let (tiendas, user_id) = abrir_tiendas_durables()?;
    tiendas.con_skills_base(user_id);
    let mut registry = AgentToolRegistry::new();
    crate::mcp_cli::registrar_desde_env(&mut registry).await?;
    let mut harness = construir_harness_con_impl(
        opciones,
        tiendas.clone(),
        tiendas.clone(),
        user_id,
        registry,
    );
    harness.sqlite = Some(tiendas);
    Ok(harness)
}

/// [069A-2] Abre la BD durable del CLI y resuelve su `user_id` estable
/// (creado una vez en `config`). Compartido por `schedule` (tareas), `chat`,
/// `tui` y `session` (conversaciones): una sola tienda y un solo usuario.
pub fn abrir_tiendas_durables() -> Result<(Arc<PersistenciaSqlite>, Uuid), String> {
    let ruta = PersistenciaSqlite::ruta_bd_app()
        .ok_or_else(|| "sin directorio de app para la BD (APPDATA/HOME ausente)".to_string())?;
    let tiendas = Arc::new(PersistenciaSqlite::abrir(&ruta).map_err(|e| e.to_string())?);
    let user_id = usuario_cli_estable(&tiendas)?;
    Ok((tiendas, user_id))
}

/// Lee o crea el `user_id` estable del CLI en la tabla `config` (un valor
/// corrupto se sustituye por uno nuevo, nunca se aborta por ello).
pub fn usuario_cli_estable(tiendas: &PersistenciaSqlite) -> Result<Uuid, String> {
    const CLAVE: &str = "usuario_cli";
    if let Some(previo) = tiendas.config_leer(CLAVE).map_err(|e| e.to_string())? {
        if let Ok(id) = Uuid::parse_str(previo.trim()) {
            return Ok(id);
        }
    }
    let nuevo = Uuid::new_v4();
    tiendas
        .config_guardar(CLAVE, &nuevo.to_string())
        .map_err(|e| e.to_string())?;
    Ok(nuevo)
}

/// Constructor con persistencia y programador inyectables (B5): la app Tauri
/// pasa `PersistenciaSqlite` para historial durable. `user_id` lo genera el
/// llamador (la app lo conserva entre reinicios vía config). Sin MCP: el
/// consumidor que quiera servidores MCP usa [`construir_harness_con_impl`]
/// con su registry ya poblado (mismo patrón que el sandbox/todo).
pub fn construir_harness_con(
    opciones: &OpcionesRun,
    persistencia: Arc<dyn AgentPersistence>,
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> HarnessCli {
    construir_harness_con_impl(
        opciones,
        persistencia,
        programador,
        user_id,
        AgentToolRegistry::new(),
    )
}

/// Núcleo compartido de construcción (registro de tools inyectado).
pub fn construir_harness_con_impl(
    opciones: &OpcionesRun,
    persistencia: Arc<dyn AgentPersistence>,
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
    registry: AgentToolRegistry,
) -> HarnessCli {
    /* La raíz del workspace: `--dir`, o el cwd donde se invocó el comando.
     * Así el agente "trabaja en esa carpeta" con sus tools de archivo. */
    let workspace = opciones
        .dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.canonicalize().unwrap_or(p))
        .map(quitar_prefijo_verbatim);

    /* [02-09-2026] En el CLI standalone queremos tools de archivo sobre el
     * workspace, que el núcleo solo activa con AGENTE_MODO=local. Fijamos
     * `local` por defecto salvo que el usuario ya haya elegido otro modo
     * explícitamente (p. ej. prod). */
    if std::env::var_os("AGENTE_MODO").is_none() {
        // edition 2021: set_var es seguro (sin unsafe).
        std::env::set_var("AGENTE_MODO", "local");
    }

    let llm = Arc::new(LlmProviderService::new(LlavesProveedor::from_env()));

    let mut config = turno_config_default(workspace.clone());
    if let Some(provider) = opciones
        .provider
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        config.provider = provider.to_string();
    }
    if let Some(modelo) = opciones
        .modelo
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        config.modelo = modelo.to_string();
    }
    /* [318A-16 F5] `--modo plan` activa la propuesta con diff; cualquier otro
     * valor explícito se respeta (meta/autonomo). El default sigue
     * `predeterminado` (fail-closed: un typo no abre permisos). */
    if let Some(modo) = opciones
        .modo
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty())
    {
        config.modo = modo.to_string();
    }
    /* [039A-1 04-09 H7] Nivel de razonamiento del turno. Se valida contra el
     * conjunto conocido (low|medium|high) y se ignora cualquier otro valor
     * (fail-closed: un typo no rompe el arranque; el proveedor usa su
     * default). `None` no se toca: el default de `TurnoConfig` es `None`. */
    if let Some(razonamiento) = opciones
        .razonamiento
        .as_deref()
        .map(str::trim)
        .filter(|r| !r.is_empty())
    {
        if matches!(razonamiento, "low" | "medium" | "high") {
            config.nivel_razonamiento = Some(razonamiento.to_string());
        }
    }
    /* [039A-3 P6-backend] Ventana inyectada por el consumidor (desktop 150k).
     * Bajo el piso se ignora (fail-closed al default del core 128k). Se fija
     * aquí, ANTES de `AgentRuntime::nuevo`, porque el manager de contexto y
     * el desglose del turno clonan `config.contexto` al construir. */
    if let Some(v) = opciones.max_ventana.filter(|v| *v >= VENTANA_MINIMA) {
        config.contexto.max_ventana = v;
    }

    /* [Bloque 3, F3] Skills del workspace (carpeta `.glory/skills`, archivos
     * markdown con frontmatter): la tool `skill` se registra ANTES de mover
     * el registry al runtime, y el índice se anexa a la ranura [REGLAS]
     * (helper puro, sin I/O en el núcleo). Sin carpeta de skills no hay tool
     * ni índice (fail-closed). */
    let skills = workspace
        .as_deref()
        .map(|dir| glory_harness_core::skill::descubrir_en(&dir.join(".glory").join("skills")))
        .unwrap_or_default();
    let mut registry = registry;
    if !skills.is_empty() {
        registry.registrar(Box::new(glory_harness_core::skill::ToolSkill::nuevo(
            skills.clone(),
        )));
    }

    let runtime = Arc::new(AgentRuntime::nuevo(
        registry,
        PuertosHarness {
            persistencia: Arc::clone(&persistencia),
            llm,
            web_search: None,
            /* [Bloque 3, F1] web_fetch real del CLI (reqwest ligero). */
            web_fetch: Some(Arc::new(crate::fetch::FetchCli::nuevo())),
            dominio: None,
            ejecutor_comando: Some(Arc::new(crate::ejecutor::EjecutorCliente::nuevo())),
            programador_tareas: Some(programador),
            /* [069A-1 F5] Navegador interno: solo el desktop inyecta un
             * puerto real; el CLI y schedule lo dejan en None, la tool no
             * se registra (fail-closed). */
            navegador: opciones.navegador.clone(),
        },
        config.clone(),
    ));
    /* [318A-15 F2] Reglas del repositorio (AGENTS.md, jerarquía: la raíz gana
     * a subcarpetas) → ranura `[REGLAS]` del system prompt. Sin AGENTS.md la
     * ranura queda vacía (el núcleo no emite encabezado huérfano). [Bloque 3,
     * F3] El índice de skills descubiertas se anexa al mismo slot: el modelo
     * ve las skills disponibles y usa la tool `skill` para cargar una. */
    if let Some(workspace_dir) = workspace.as_deref() {
        runtime.establecer_reglas(glory_harness_core::skill::reglas_con_skills(
            &crate::reglas::cargar_reglas(workspace_dir).unwrap_or_default(),
            &skills,
        ));
    }

    HarnessCli {
        runtime,
        persistencia,
        sqlite: None,
        user_id,
        workspace,
        config,
    }
}

/// Default del CLI: Laguna S 2.1 free (commandcode directo). Si falla, el
/// núcleo salta solo a la cadena de respaldo (glory/auto, deepseek, …).
/// Compartido con `chat` (Fase 5): ambos subcomandos construyen el mismo
/// runtime con la misma configuración por defecto.
pub fn turno_config_default(workspace: Option<PathBuf>) -> TurnoConfig {
    TurnoConfig {
        provider: "commandcode".into(),
        modelo: "poolside/laguna-s-2.1-free".into(),
        workspace: workspace.map(|p| p.to_string_lossy().into_owned()),
        ..TurnoConfig::default()
    }
}

/// En Windows `canonicalize` devuelve rutas con prefijo verbatim `\\?\C:\...`;
/// se quita para que el sandbox y los mensajes usen la forma legible `C:\...`.
/// Compartido con `chat` (Fase 5).
pub fn quitar_prefijo_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    let limpio = s.strip_prefix(r"\\?\").unwrap_or(&s);
    PathBuf::from(limpio)
}

/// Ejecuta un turno con el mensaje dado y recoge la respuesta de texto.
/// Devuelve la salida o un error presentable al usuario de la CLI.
pub async fn ejecutar_turno_run(
    mensaje: String,
    opciones: OpcionesRun,
) -> Result<SalidaTurno, String> {
    let harness = construir_harness(&opciones).await?;
    let user_id = harness.user_id;
    let runtime = harness.runtime;
    // [069A-3] Avisos de escritorio solo si el operador los pidió.
    crate::notificar::aplicar_notificacion(&runtime, opciones.notificar);

    // [069A-4] Prefetch de memoria: la misma vía que chat/tui (aquí la
    // tienda es efímera, pero el camino queda cableado y verificado).
    let historial = crate::memoria::anteponer_memoria(
        Vec::new(),
        crate::memoria::bloque_memoria_para_turno(
            &harness.persistencia,
            user_id,
            &mensaje,
            harness.config.incluir_memoria,
            harness.config.incluir_skills,
        )
        .await,
    );

    let turno_id = Uuid::new_v4();
    let conversacion_id = Uuid::new_v4();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgenteEvento>(64);

    let handle = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        let mensaje_usuario = mensaje.clone();
        async move {
            runtime
                .ejecutar_turno(
                    user_id,
                    turno_id,
                    conversacion_id,
                    historial,
                    mensaje_usuario,
                    &tx,
                )
                .await
        }
    });

    let mut texto = String::new();
    let mut tools = Vec::new();
    let mut ok = true;
    while let Some(evento) = rx.recv().await {
        match evento {
            AgenteEvento::Token { texto: t } => texto.push_str(&t),
            AgenteEvento::ToolStart { tool, .. } => tools.push(tool),
            AgenteEvento::Error { mensaje: m, .. } => {
                eprintln!("[glory-harness] error: {m}");
                ok = false;
            }
            AgenteEvento::Done { .. } => break,
            _ => {}
        }
    }

    /* No fallo silencioso: el runtime propaga los errores de proveedor/red con
     * `?` sin emitir necesariamente un `AgenteEvento::Error`; si el turno
     * terminó en error real, se devuelve como `Err` para que el CLI salga con
     * código ≠ 0 y muestre la causa (antes se descartaba con `let _` y un
     * turno fallido salía vacío con exit 0). */
    let resultado = handle.await;
    match resultado {
        Ok(Ok(())) => {
            // [069A-4] Sync post-turno (mejor esfuerzo con aviso; el turno
            // ya respondió y su éxito no depende de la memoria).
            crate::memoria::sincronizar_memoria_tras_turno(
                &harness.persistencia,
                user_id,
                &texto,
                &mensaje,
                "turno:run",
            )
            .await;
            Ok(SalidaTurno { texto, tools, ok })
        }
        // El runtime propaga errores de proveedor/red con `?`; sean o no
        // acompañados por un evento Error previo, el turno fallido se reporta
        // como `Err` para que el CLI salga con código ≠ 0 y muestre la causa.
        Ok(Err(err)) => Err(err.to_string()),
        Err(err) => Err(format!("el turno abortó con pánico: {err}")),
    }
}

/// Ejecuta el subcomando `run`. Lee el prompt de `--prompt` (o `--mensaje`)
/// o de `--stdin`; imprime la respuesta. Devuelve `ExitCode`.
pub async fn run(prompt: Option<String>, opciones: OpcionesRun) -> std::process::ExitCode {
    let Some(prompt) = prompt else {
        eprintln!("glory-harness run: falta --prompt \"...\" (o usa --stdin)");
        return std::process::ExitCode::from(2);
    };

    let raiz = opciones
        .dir
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .map(|p| p.canonicalize().unwrap_or(p))
        .map(quitar_prefijo_verbatim)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    eprintln!("[glory-harness] workspace: {raiz}");

    match ejecutar_turno_run(prompt, opciones).await {
        Ok(salida) => {
            if !salida.tools.is_empty() {
                eprintln!(
                    "[glory-harness] tools ejecutadas: {}",
                    salida.tools.join(", ")
                );
            }
            if salida.ok {
                println!("{}", salida.texto);
            }
            if salida.ok {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(1)
            }
        }
        Err(err) => {
            eprintln!("[glory-harness] error: {err}");
            std::process::ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod pruebas_ventana {
    //! [039A-3 P6-backend] La ventana inyectada llega al runtime ANTES de
    //! construir (el desglose del turno lee `turno_config.contexto`); sin
    //! inyección o bajo el piso, el default del core (128k) queda intacto.
    use super::*;

    fn harness_con(max_ventana: Option<u32>) -> HarnessCli {
        let persistencia = Arc::new(crate::persistencia::PersistenciaMemoria::nuevo());
        let programador = Arc::new(crate::persistencia::ProgramadorMemoria::nuevo());
        construir_harness_con(
            &OpcionesRun {
                max_ventana,
                ..OpcionesRun::default()
            },
            persistencia,
            programador,
            Uuid::new_v4(),
        )
    }

    #[test]
    fn sin_ventana_se_conserva_el_default_del_core() {
        let h = harness_con(None);
        assert_eq!(h.config.contexto.max_ventana, 128_000);
        assert_eq!(h.runtime.turno_config.contexto.max_ventana, 128_000);
    }

    #[test]
    fn ventana_del_desktop_se_inyecta_antes_del_runtime() {
        let h = harness_con(Some(150_000));
        assert_eq!(h.config.contexto.max_ventana, 150_000);
        assert_eq!(h.runtime.turno_config.contexto.max_ventana, 150_000);
    }

    #[test]
    fn ventana_bajo_el_piso_se_ignora() {
        let h = harness_con(Some(1_000));
        assert_eq!(h.config.contexto.max_ventana, 128_000);
        assert_eq!(h.runtime.turno_config.contexto.max_ventana, 128_000);
    }
}
