//! Comandos de sesion y configuracion del desktop.

use super::*;
use glory_harness_core::hooks::ComandoGancho;

#[derive(serde::Serialize, Clone)]
pub(super) struct ProveedorConteo {
    pub(super) nombre: String,
    pub(super) claves: usize,
}

pub(super) fn conteos(llaves: &LlavesProveedor) -> Vec<ProveedorConteo> {
    vec![
        ProveedorConteo {
            nombre: "cerebras".into(),
            claves: llaves.cerebras.len(),
        },
        ProveedorConteo {
            nombre: "groq".into(),
            claves: llaves.groq.len(),
        },
        ProveedorConteo {
            nombre: "deepseek".into(),
            claves: llaves.deepseek.len(),
        },
        ProveedorConteo {
            nombre: "glory".into(),
            claves: llaves.glory.len(),
        },
        ProveedorConteo {
            nombre: "commandcode".into(),
            claves: llaves.commandcode.len(),
        },
    ]
}

/// Cambia provider/modelo/modo SIN perder la conversación: reconstruye solo
/// el runtime sobre la misma persistencia, usuario y conversación. Falla con
/// turno en curso. El workspace no cambia (para eso, `elegir_workspace`).
#[tauri::command]
pub(crate) fn reconfigurar_sesion(
    estado: State<'_, Estado>,
    provider: Option<String>,
    modelo: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
) -> Result<InfoSesion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    /* Reconfigurar mueve el provider/modelo/modo; el objeto comun cambia
     * in-situ y su runtime se recablea al vault del desktop. */
    let mut comun = sesion
        .comun
        .lock()
        .map_err(|_| "sesión bloqueada".to_string())?;
    comun
        .reconfigurar(provider, modelo, modo, razonamiento)
        .map_err(|e| e.to_string())?;
    /* El nuevo runtime trae el hook persistido porque `reconfigurar` lo
     * resuelve desde SQLite; se recablea además el sandbox del desktop. */
    cablear_vault_a(&comun.runtime, &sesion.vault);
    drop(comun);
    info_de_panel(&sesion, PANEL_PRINCIPAL)
}

/// Responde una petición de aprobación (canal F2, tres vías).
#[tauri::command]
pub(crate) fn responder_aprobacion(
    estado: State<'_, Estado>,
    id: String,
    respuesta: String,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    let runtime = sesion
        .comun
        .lock()
        .map(|g| Arc::clone(&g.runtime))
        .map_err(|_| "sesión bloqueada".to_string())?;
    let r = match respuesta.as_str() {
        "aprobar" => RespuestaAprobacion::Aprobar,
        "siempre" => RespuestaAprobacion::Siempre,
        _ => RespuestaAprobacion::Rechazar,
    };
    runtime
        .responder_aprobacion(&id, r)
        .map_err(|e| e.to_string())
}

/// Peticiones de aprobación aún pendientes (para pintar tarjetas al cerrar).
#[tauri::command]
pub(crate) fn pendientes_aprobacion(
    estado: State<'_, Estado>,
) -> Result<Vec<PeticionAprobacion>, String> {
    match estado.sesion.lock() {
        Ok(g) => Ok(g
            .as_ref()
            .map(|s| {
                s.comun
                    .lock()
                    .map(|c| c.runtime.peticiones_aprobacion_pendientes())
                    .unwrap_or_default()
            })
            .unwrap_or_default()),
        Err(_) => Err("sesión bloqueada".into()),
    }
}

// --- F4: catálogo, config, workspace, meta ---

#[derive(serde::Serialize)]
pub(super) struct ProveedorInfo {
    id: &'static str,
    modelos: Vec<&'static str>,
    claves: usize,
}

/// Proveedores/modelos REALES del allowlist del núcleo + nº de claves. La UI
/// ya no duplica la tabla: el selector se alimenta de aquí.
#[tauri::command]
pub(crate) fn proveedores_disponibles() -> Vec<ProveedorInfo> {
    let llaves = LlavesProveedor::from_env();
    catalogo_proveedores()
        .into_iter()
        .map(|(id, modelos)| {
            let claves = match id {
                "cerebras" => llaves.cerebras.len(),
                "groq" => llaves.groq.len(),
                "deepseek" => llaves.deepseek.len(),
                "glory" => llaves.glory.len(),
                "commandcode" => llaves.commandcode.len(),
                _ => 0,
            };
            ProveedorInfo {
                id,
                modelos,
                claves,
            }
        })
        .collect()
}

/// Lee una clave de configuración (`None` = no definida).
#[tauri::command]
pub(crate) fn config_leer(
    estado: State<'_, Estado>,
    clave: String,
) -> Result<Option<String>, String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .config_leer(clave.trim())
        .map_err(|e| e.to_string())
}

/// Guarda una clave de configuración (`provider_defecto`, `modelo_defecto`…).
/// El hook pre-compact tiene una ruta tipada: valida, persiste y reconstruye
/// el runtime en la misma operación lógica; `null` o vacío lo desactiva.
#[tauri::command]
pub(crate) fn config_guardar(
    estado: State<'_, Estado>,
    clave: String,
    valor: String,
) -> Result<(), String> {
    let clave = clave.trim().to_string();
    let sesion = sesion_actual(&estado)?;
    if clave == "gancho_pre_compact" {
        let hook = if valor.trim().is_empty() || valor.trim() == "null" {
            None
        } else {
            let hook = serde_json::from_str::<ComandoGancho>(&valor)
                .map_err(|e| format!("gancho_pre_compact inválido: {e}"))?;
            if hook.comando.trim().is_empty() {
                return Err("gancho_pre_compact vacío".into());
            }
            if hook.timeout_ms > 60_000 {
                return Err("timeout del hook demasiado alto".into());
            }
            Some(hook)
        };
        if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
            return Err("hay un turno en curso".into());
        }
        match hook {
            Some(hook) => {
                let raw = serde_json::to_string(&hook).map_err(|e| e.to_string())?;
                sesion
                    .persistencia
                    .config_guardar(&clave, &raw)
                    .map_err(|e| e.to_string())?;
            }
            None => sesion
                .persistencia
                .config_borrar(&clave)
                .map_err(|e| e.to_string())?,
        }
        let mut comun = sesion
            .comun
            .lock()
            .map_err(|_| "sesión bloqueada".to_string())?;
        comun
            .reconfigurar(None, None, None, None)
            .map_err(|e| e.to_string())?;
        cablear_vault_a(&comun.runtime, &sesion.vault);
        return Ok(());
    }
    sesion
        .persistencia
        .config_guardar(&clave, &valor)
        .map_err(|e| e.to_string())
}
