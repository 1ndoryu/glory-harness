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
use glory_harness_core::llm::{LlmProviderService, LlavesProveedor};
use glory_harness_core::runtime::{AgentRuntime, PuertosHarness, TurnoConfig};
use glory_harness_core::tool::AgentToolRegistry;
use glory_harness_core::AgentPersistence;

use crate::persistencia::PersistenciaMemoria;

/// Opciones del subcomando `run`.
#[derive(Debug, Clone, Default)]
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
}

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
    pub persistencia: Arc<PersistenciaMemoria>,
    pub user_id: Uuid,
    pub workspace: Option<PathBuf>,
    pub config: TurnoConfig,
}

pub fn construir_harness(opciones: &OpcionesRun) -> HarnessCli {
    let persistencia = Arc::new(PersistenciaMemoria::nuevo());
    // Añadir una skill base para dar contexto útil (standalone sin BD).
    let user_id = Uuid::new_v4();
    persistencia.con_skills_base(user_id);

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
    let persistencia_port: Arc<dyn AgentPersistence> = persistencia.clone();

    let mut config = turno_config_default(workspace.clone());
    if let Some(provider) = opciones.provider.as_deref().map(str::trim).filter(|p| !p.is_empty()) {
        config.provider = provider.to_string();
    }
    if let Some(modelo) = opciones.modelo.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        config.modelo = modelo.to_string();
    }
    /* [318A-16 F5] `--modo plan` activa la propuesta con diff; cualquier otro
     * valor explícito se respeta (meta/autonomo). El default sigue
     * `predeterminado` (fail-closed: un typo no abre permisos). */
    if let Some(modo) = opciones.modo.as_deref().map(str::trim).filter(|m| !m.is_empty()) {
        config.modo = modo.to_string();
    }

    let runtime = Arc::new(AgentRuntime::nuevo(
        AgentToolRegistry::new(),
        PuertosHarness {
            persistencia: persistencia_port,
            llm,
            web_search: None,
            dominio: None,
            ejecutor_comando: Some(Arc::new(crate::ejecutor::EjecutorCliente::nuevo())),
            programador_tareas: Some(Arc::new(
                crate::persistencia::ProgramadorMemoria::nuevo(),
            )),
        },
        config.clone(),
    ));
    /* [318A-15 F2] Reglas del repositorio (AGENTS.md, jerarquía: la raíz gana
     * a subcarpetas) → ranura `[REGLAS]` del system prompt. Sin AGENTS.md la
     * ranura queda vacía (el núcleo no emite encabezado huérfano). */
    if let Some(workspace_dir) = workspace.as_deref() {
        runtime.establecer_reglas(crate::reglas::cargar_reglas(workspace_dir).unwrap_or_default());
    }

    HarnessCli {
        runtime,
        persistencia,
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
pub async fn ejecutar_turno_run(mensaje: String, opciones: OpcionesRun) -> Result<SalidaTurno, String> {
    let harness = construir_harness(&opciones);
    let user_id = harness.user_id;
    let runtime = harness.runtime;

    let turno_id = Uuid::new_v4();
    let conversacion_id = Uuid::new_v4();
    let (tx, mut rx) = tokio::sync::mpsc::channel::<AgenteEvento>(64);

    let handle = tokio::spawn({
        let runtime = Arc::clone(&runtime);
        async move { runtime.ejecutar_turno(user_id, turno_id, conversacion_id, Vec::new(), mensaje, &tx).await }
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
        Ok(Ok(())) => Ok(SalidaTurno { texto, tools, ok }),
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