//! Backend Tauri in-process de Glory Harness (plan 039A-1, Fase 4, vía TS).
//!
//! La UI TS (`desktop/ui`) invoca estos comandos y escucha `agente-evento`.
//! No hay daemon TCP ni simulación: la sesión se construye con el MISMO lib
//! compartido del CLI (`glory_harness::construir_harness`) y los turnos
//! emiten el contrato real `AgenteEvento` (tag `evento`, snake_case).
//!
//! Optimización: un solo proceso (webview + núcleo), sin IPC de red, sin
//! proceso daemon extra; runtime del turno abortable al cancelar.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use glory_harness::{OpcionesRun, construir_harness, historial_desde_persistencia};
use glory_harness_core::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::LlavesProveedor;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::AgentPersistence;
use tauri::{Emitter, Manager, State};
use uuid::Uuid;

/// Sesión viva del núcleo (misma construcción que `chat`/`run` del CLI).
struct Sesion {
    runtime: Arc<AgentRuntime>,
    persistencia: Arc<glory_harness::PersistenciaMemoria>,
    user_id: Uuid,
    conversacion_id: Uuid,
}

/// Estado global: sesión opcional + turno en curso (para cancelar).
struct Estado {
    sesion: Mutex<Option<Arc<Sesion>>>,
    turno: Mutex<TurnoEnCurso>,
}

/// Turno en curso: el flag manda (el handle puede quedar huérfano tras
/// abortar; `is_finished` no existe en este JoinHandle).
struct TurnoEnCurso {
    handle: Option<tauri::async_runtime::JoinHandle<()>>,
    activo: bool,
}

impl Default for Estado {
    fn default() -> Self {
        Self {
            sesion: Mutex::new(None),
            turno: Mutex::new(TurnoEnCurso {
                handle: None,
                activo: false,
            }),
        }
    }
}

/// Marca el turno como terminado (lo llama la propia tarea al cerrar).
fn marcar_turno_terminado(estado: &Estado) {
    if let Ok(mut t) = estado.turno.lock() {
        t.handle = None;
        t.activo = false;
    }
}

#[derive(serde::Serialize)]
struct InfoSesion {
    modelo: String,
    workspace: String,
    proveedores: Vec<ProveedorConteo>,
}

#[derive(serde::Serialize)]
struct ProveedorConteo {
    nombre: &'static str,
    claves: usize,
}

fn conteos(llaves: &LlavesProveedor) -> Vec<ProveedorConteo> {
    vec![
        ProveedorConteo {
            nombre: "cerebras",
            claves: llaves.cerebras.len(),
        },
        ProveedorConteo {
            nombre: "groq",
            claves: llaves.groq.len(),
        },
        ProveedorConteo {
            nombre: "deepseek",
            claves: llaves.deepseek.len(),
        },
        ProveedorConteo {
            nombre: "glory/empero",
            claves: llaves.glory.len(),
        },
        ProveedorConteo {
            nombre: "commandcode",
            claves: llaves.commandcode.len(),
        },
    ]
}

/// Abre (o reabre) la sesión del núcleo con provider/modelo/dir/modo.
#[tauri::command]
fn abrir_sesion(
    estado: State<'_, Estado>,
    provider: Option<String>,
    modelo: Option<String>,
    dir: Option<String>,
    modo: Option<String>,
) -> Result<InfoSesion, String> {
    glory_harness::cargar_env_usuario();
    let opciones = OpcionesRun {
        provider,
        modelo,
        dir: dir.map(std::path::PathBuf::from),
        modo,
    };
    let harness = construir_harness(&opciones);
    let info = InfoSesion {
        modelo: format!("{}/{}", harness.config.provider, harness.config.modelo),
        workspace: harness
            .workspace
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<desconocido>".into()),
        proveedores: conteos(&LlavesProveedor::from_env()),
    };
    let sesion = Arc::new(Sesion {
        runtime: harness.runtime,
        persistencia: harness.persistencia,
        user_id: harness.user_id,
        conversacion_id: Uuid::new_v4(),
    });
    match estado.sesion.lock() {
        Ok(mut g) => {
            *g = Some(sesion);
            Ok(info)
        }
        Err(_) => Err("sesión bloqueada por otro turno".into()),
    }
}

/// Ejecuta un turno real y reemite cada `AgenteEvento` a la UI.
/// Emite `agente-evento` por evento y `turno-fin` (`{ok, error?}`) al cerrar.
#[tauri::command]
fn enviar_turno(
    estado: State<'_, Estado>,
    window: tauri::Window,
    mensaje: String,
) -> Result<(), String> {
    let sesion = match estado.sesion.lock() {
        Ok(g) => g.clone().ok_or_else(|| "abre la sesión primero".to_string())?,
        Err(_) => return Err("sesión bloqueada por otro turno".into()),
    };
    {
        let puede = match estado.turno.lock() {
            Ok(t) => !t.activo,
            Err(_) => return Err("no se pudo acceder al turno".into()),
        };
        if !puede {
            return Err("ya hay un turno en curso".into());
        }
    }
    if mensaje.trim().is_empty() {
        return Err("mensaje vacío".into());
    }
    let (tx_ev, mut rx_ev) = tokio::sync::mpsc::channel::<AgenteEvento>(64);
    let w = window.clone();
    let handle = tauri::async_runtime::spawn(async move {
        // Al cerrar (ok, fallo o abort) se libera el flag para el próximo turno.
        let terminar = |w: &tauri::Window| {
            if let Some(e) = w.app_handle().try_state::<Estado>() {
                marcar_turno_terminado(&e);
            }
        };
        let turno_id = Uuid::new_v4();
        let historial = match sesion
            .persistencia
            .listar_mensajes(sesion.conversacion_id)
            .await
        {
            Ok(mensajes) => historial_desde_persistencia(mensajes),
            Err(e) => {
                let _ = w.emit("turno-fin", serde_json::json!({"ok": false, "error": e.to_string()}));
                terminar(&w);
                return;
            }
        };
        // Reenvío en la misma tarea: el loop termina con Done (último evento).
        let w_fw = w.clone();
        let reenvio = tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx_ev.recv().await {
                let es_done = matches!(ev, AgenteEvento::Done { .. });
                let _ = w_fw.emit("agente-evento", &ev);
                if es_done {
                    break;
                }
            }
        });
        let resultado = sesion
            .runtime
            .ejecutar_turno(
                sesion.user_id,
                turno_id,
                sesion.conversacion_id,
                historial,
                mensaje,
                &tx_ev,
            )
            .await;
        drop(tx_ev);
        let _ = reenvio.await;
        match resultado {
            Ok(()) => {
                let _ = w.emit("turno-fin", serde_json::json!({"ok": true}));
            }
            Err(e) => {
                let _ = w.emit("agente-evento", &AgenteEvento::Error {
                    mensaje: e.to_string(),
                    retryable: true,
                });
                let _ = w.emit("turno-fin", serde_json::json!({"ok": false, "error": e.to_string()}));
            }
        }
        terminar(&w);
    });
    match estado.turno.lock() {
        Ok(mut t) => {
            t.handle = Some(handle);
            t.activo = true;
            Ok(())
        }
        Err(_) => Err("no se pudo registrar el turno".into()),
    }
}

/// Aborta el turno en curso (el runtime se detiene al cerrar el canal).
#[tauri::command]
fn cancelar_turno(estado: State<'_, Estado>) -> Result<(), String> {
    match estado.turno.lock() {
        Ok(mut t) => {
            if let Some(h) = t.handle.take() {
                h.abort();
            }
            t.activo = false;
            Ok(())
        }
        Err(_) => Err("no se pudo acceder al turno".into()),
    }
}

/// Responde una petición de aprobación (canal F2, tres vías).
#[tauri::command]
fn responder_aprobacion(
    estado: State<'_, Estado>,
    id: String,
    respuesta: String,
) -> Result<(), String> {
    let sesion = match estado.sesion.lock() {
        Ok(g) => g.clone().ok_or_else(|| "abre la sesión primero".to_string())?,
        Err(_) => return Err("sesión bloqueada".into()),
    };
    let r = match respuesta.as_str() {
        "aprobar" => RespuestaAprobacion::Aprobar,
        "siempre" => RespuestaAprobacion::Siempre,
        _ => RespuestaAprobacion::Rechazar,
    };
    sesion
        .runtime
        .responder_aprobacion(&id, r)
        .map_err(|e| e.to_string())
}

/// Peticiones de aprobación aún pendientes (para pintar tarjetas al cerrar).
#[tauri::command]
fn pendientes_aprobacion(
    estado: State<'_, Estado>,
) -> Result<Vec<PeticionAprobacion>, String> {
    match estado.sesion.lock() {
        Ok(g) => Ok(g
            .as_ref()
            .map(|s| s.runtime.peticiones_aprobacion_pendientes())
            .unwrap_or_default()),
        Err(_) => Err("sesión bloqueada".into()),
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Estado::default())
        .invoke_handler(tauri::generate_handler![
            abrir_sesion,
            enviar_turno,
            cancelar_turno,
            responder_aprobacion,
            pendientes_aprobacion,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("[glory-harness-desktop] error fatal: {e}");
            std::process::exit(1);
        });
}
