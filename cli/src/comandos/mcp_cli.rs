//! Config de servidores MCP del consumidor CLI (Bloque 3 Fase 2).
//!
//! Superficie: variable `GLORY_MCP_CONFIG` → ruta de un JSON con la forma
//! `[{ "nombre": "...", "comando": "...", "argumentos": ["..."] }]`. Sin la
//! variable no hay servidores MCP (fail-closed). Cada servidor se spawna y se
//! negocia (initialize + tools/list) con timeout acotado; un fallo aborta el
//! arranque del chat con el error concreto — nunca se ignora ni se registra a
//! medias. La app Tauri / IA de Tasks aporta su propia config (mismo formato)
//! en su construcción del runtime.

use std::sync::Arc;
use std::time::Duration;

use glory_harness_core::regla::CAT_MCP;
use glory_harness_core::tool::AgentToolRegistry;

/// Descriptor de un servidor MCP en la config del consumidor.
#[derive(Debug, serde::Deserialize)]
struct ServidorMcpConfig {
    nombre: String,
    comando: String,
    #[serde(default)]
    argumentos: Vec<String>,
}

/// Registra en `registry` los servidores MCP declarados por la config.
/// Sin `GLORY_MCP_CONFIG` no hace nada. Con config, cada servidor se negocia
/// con `timeout` (10 s: arranque + initialize + tools/list) y un fallo se
/// propaga con el nombre del servidor (fail-closed, arranque abortado).
pub async fn registrar_desde_env(registry: &mut AgentToolRegistry) -> Result<(), String> {
    let ruta = match std::env::var_os("GLORY_MCP_CONFIG") {
        Some(r) => r,
        None => return Ok(()),
    };
    let contenido = std::fs::read_to_string(&ruta).map_err(|e| {
        format!(
            "GLORY_MCP_CONFIG={} no se puede leer: {e}",
            ruta.to_string_lossy()
        )
    })?;
    let servidores: Vec<ServidorMcpConfig> =
        serde_json::from_str(&contenido).map_err(|e| format!("config MCP inválida: {e}"))?;
    for servidor in servidores {
        let nombre = servidor.nombre.trim().to_string();
        if nombre.is_empty() {
            return Err("config MCP: servidor con `nombre` vacío".into());
        }
        let proveedor = tokio::time::timeout(
            Duration::from_secs(10),
            glory_harness_core::mcp::McpProveedorStdio::nuevo(
                &servidor.comando,
                servidor.argumentos,
            ),
        )
        .await
        .map_err(|_| format!("servidor MCP `{nombre}`: timeout (10 s) en arranque/initialize"))?
        .map_err(|e| format!("servidor MCP `{nombre}`: {e}"))?;
        registry
            .registrar_mcp(&nombre, Arc::new(proveedor))
            .await
            .map_err(|e| format!("servidor MCP `{nombre}`: {e}"))?;
    }
    Ok(())
}

/// `true` si la config actual declara servidores MCP (para el banner de
/// arranque del chat: qué tools MCP quedaron disponibles).
#[must_use]
pub fn hay_servidores_configurados() -> bool {
    std::env::var_os("GLORY_MCP_CONFIG").is_some()
}

/// Categoría de permiso de las tools MCP (re-export para el banner).
#[must_use]
pub fn categoria_mcp() -> &'static str {
    CAT_MCP
}
