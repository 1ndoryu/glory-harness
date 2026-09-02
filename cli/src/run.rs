//! Subcomando `glory-harness run` (Fase 3): responde un turno por CLI.
//!
//! Construye el runtime del núcleo con la persistencia en memoria y el
//! proveedor LLM cargado de las envs (`LlmProviderService::new(
//! LlavesProveedor::from_env())`), ejecuta un turno y vuelca la respuesta de
//! texto a stdout. El contrato de eventos es el mismo `AgenteEvento` de H3,
//! así que el resultado es idéntico al que vería el frontend de task vía SSE.

use std::sync::Arc;
use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::{LlmProviderService, LlavesProveedor};
use glory_harness_core::runtime::{AgentRuntime, PuertosHarness, TurnoConfig};
use glory_harness_core::tool::AgentToolRegistry;
use glory_harness_core::AgentPersistence;

use crate::persistencia::PersistenciaMemoria;

/// Resultado de un turno one-shot, listo para imprimir.
pub struct SalidaTurno {
    pub texto: String,
    pub tools: Vec<String>,
    pub ok: bool,
}

/// Ejecuta un turno con el mensaje dado y recoge la respuesta de texto.
/// Devuelve la salida o un error presentable al usuario de la CLI.
pub async fn ejecutar_turno_run(mensaje: String) -> Result<SalidaTurno, String> {
    let persistencia = Arc::new(PersistenciaMemoria::nuevo());
    // Añadir una skill base para dar contexto útil (standalone sin BD).
    let user_id = Uuid::new_v4();
    persistencia.con_skills_base(user_id);

    let llm = Arc::new(LlmProviderService::new(LlavesProveedor::from_env()));
    let persistencia_port: Arc<dyn AgentPersistence> = persistencia.clone();

    let registry = AgentToolRegistry::new();
    // El runtime añade web_search + file_* (fail-closed sin sandbox local).
    let puertos = PuertosHarness {
        persistencia: persistencia_port,
        llm,
        web_search: None,
        dominio: None,
    };
    let runtime = Arc::new(AgentRuntime::nuevo(registry, puertos, TurnoConfig::default()));

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
pub async fn run(prompt: Option<String>) -> std::process::ExitCode {
    let Some(prompt) = prompt else {
        eprintln!("glory-harness run: falta --prompt \"...\" (o usa --stdin)");
        return std::process::ExitCode::from(2);
    };

    match ejecutar_turno_run(prompt).await {
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