//! [079A-1 F3] Handlers web de proveedores y configuración (partido de web_datos.rs).

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::{HeaderMap, Method},
    Json,
};
use glory_harness_core::llm::LlavesProveedor;
use serde::Deserialize;
use serde_json::Value;

use super::{error, sesion_y_comun, ApiError, AppState, MAX_TITULO_CHARS};
use crate::servicio::SesionComun;
use crate::VENTANA_MINIMA;

/// Catálogo real de proveedores (los que `LlavesProveedor` conoce).
const PROVEEDORES: [&str; 5] = ["cerebras", "groq", "deepseek", "glory", "commandcode"];
/// Modos de turno (`OpcionesRun::modo`).
const MODOS: [&str; 4] = ["predeterminado", "meta", "autonomo", "plan"];
/// Niveles de razonamiento (`TurnoConfig::nivel_razonamiento`).
const RAZONAMIENTOS: [&str; 3] = ["low", "medium", "high"];
/// Default de `contexto_max_ventana` (igual que el servicio).
const VENTANA_DEFAULT: u32 = 150_000;

#[derive(Debug, Deserialize)]
pub(crate) struct ParcheConfig {
    pub(crate) provider: Option<String>,
    pub(crate) modelo: Option<String>,
    pub(crate) modo: Option<String>,
    pub(crate) razonamiento: Option<String>,
    pub(crate) max_ventana: Option<u32>,
}

// ── Proveedores y configuración ──────────────────────────────────────────

/// `GET /api/v1/providers` — allowlist sin credenciales.
pub(crate) async fn leer_proveedores(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let _ = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let llaves = LlavesProveedor::from_env();
    let conteos = [
        ("cerebras", llaves.cerebras.len()),
        ("groq", llaves.groq.len()),
        ("deepseek", llaves.deepseek.len()),
        ("glory", llaves.glory.len()),
        ("commandcode", llaves.commandcode.len()),
    ];
    let proveedores: Vec<Value> = conteos
        .into_iter()
        .map(|(nombre, claves)| serde_json::json!({ "nombre": nombre, "disponible": claves > 0 }))
        .collect();
    Ok(Json(
        serde_json::json!({ "ok": true, "proveedores": proveedores }),
    ))
}

/// `GET /api/v1/config` — solo opciones soportadas por el núcleo.
pub(crate) async fn leer_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    Ok(Json(
        serde_json::json!({ "ok": true, "config": config_efectiva(&comun)? }),
    ))
}

/// `PATCH /api/v1/config` — valida contra el catálogo real y reconfigura.
pub(crate) async fn guardar_config(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(peticion): Json<ParcheConfig>,
) -> Result<Json<Value>, ApiError> {
    if let Some(p) = &peticion.provider {
        if !PROVEEDORES.contains(&p.as_str()) {
            return Err(error("peticion_invalida", "proveedor no soportado"));
        }
    }
    if let Some(m) = &peticion.modelo {
        if m.trim().is_empty() || m.chars().count() > MAX_TITULO_CHARS {
            return Err(error("peticion_invalida", "modelo inválido"));
        }
    }
    if let Some(m) = &peticion.modo {
        if !MODOS.contains(&m.as_str()) {
            return Err(error("peticion_invalida", "modo no soportado"));
        }
    }
    if let Some(r) = &peticion.razonamiento {
        if !RAZONAMIENTOS.contains(&r.as_str()) {
            return Err(error("peticion_invalida", "razonamiento no soportado"));
        }
    }
    if let Some(v) = peticion.max_ventana {
        if v < VENTANA_MINIMA {
            return Err(error("peticion_invalida", "max_ventana bajo el mínimo"));
        }
    }

    let (sesion, _) = sesion_y_comun(&headers, &Method::PATCH, &state, &id).await?;
    let mut comun = sesion.comun.lock().await;
    if let Some(v) = peticion.max_ventana {
        comun
            .persistencia
            .config_guardar("contexto_max_ventana", &v.to_string())
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    /* [FG1-069A-10 F1] Persistir cada clave en BD ANTES de reconfigurar el
     * runtime: así sobrevive a recargas (mantiene paridad con el escritorio
     * que usa configGuardar de Tauri → persistencia SQLite directa). */
    let ParcheConfig {
        provider,
        modelo,
        modo,
        razonamiento,
        max_ventana: _,
    } = &peticion;
    if let Some(p) = provider {
        comun
            .persistencia
            .config_guardar("proveedor", p)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    if let Some(m) = modelo {
        comun
            .persistencia
            .config_guardar("modelo", m)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    if let Some(m) = modo {
        comun
            .persistencia
            .config_guardar("modo", m)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    if let Some(r) = razonamiento {
        comun
            .persistencia
            .config_guardar("nivelRazonamiento", r)
            .map_err(|e| error("sesion", e.to_string()))?;
    }
    comun
        .reconfigurar(
            peticion.provider,
            peticion.modelo,
            peticion.modo,
            peticion.razonamiento,
        )
        .map_err(|e| error("sesion", e.to_string()))?;
    let vista = config_efectiva(&comun)?;
    Ok(Json(serde_json::json!({ "ok": true, "config": vista })))
}

fn config_efectiva(comun: &SesionComun) -> Result<Value, ApiError> {
    let cfg = &comun.runtime.turno_config;
    let max_ventana = comun
        .persistencia
        .config_leer("contexto_max_ventana")
        .map_err(|e| error("sesion", e.to_string()))?
        .and_then(|t| t.trim().parse::<u32>().ok())
        .unwrap_or(VENTANA_DEFAULT);
    Ok(serde_json::json!({
        "provider": cfg.provider,
        "modelo": cfg.modelo,
        "modo": cfg.modo,
        "razonamiento": cfg.nivel_razonamiento,
        "max_ventana": max_ventana,
        "workspace": comun.workspace,
    }))
}
