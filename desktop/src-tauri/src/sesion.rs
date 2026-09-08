//! Comandos de sesion y configuracion del desktop.

use super::*;

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
    /* El nuevo runtime trae un sandbox fresco SIN el hook: se re-cablea. */
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
#[tauri::command]
pub(crate) fn config_guardar(
    estado: State<'_, Estado>,
    clave: String,
    valor: String,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .config_guardar(clave.trim(), &valor)
        .map_err(|e| e.to_string())
}
