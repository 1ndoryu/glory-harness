//! Backend Tauri in-process de Glory Harness (plan 039A-1, Fases 3-4, vía TS).
//!
//! La UI TS (`desktop/ui`) invoca estos comandos y escucha `agente-evento`.
//! No hay daemon TCP ni simulación: la sesión se construye con el MISMO lib
//! compartido del CLI (`glory_harness::construir_harness_con`) y los turnos
//! emiten el contrato real `AgenteEvento` (tag `evento`, snake_case).
//!
//! Optimización: un solo proceso (webview + núcleo), sin IPC de red, sin
//! proceso daemon extra; runtime del turno abortable al cancelar.
//!
//! Persistencia (F3): `PersistenciaSqlite` en `%APPDATA%/glory-harness/` con
//! respaldo en memoria si la BD no abre (aviso en `InfoSesion.aviso`). El
//! `user_id` se conserva entre reinicios (tabla `config`): las conversaciones
//! sobreviven al cierre. El mensaje del usuario lo persiste el consumidor
//! antes de llamar (contrato del runtime); el turno y la respuesta los
//! persiste el núcleo vía puerto.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::{Arc, Mutex};

use glory_harness::{
    InfoConversacion, OpcionesRun, PersistenciaSqlite, construir_harness_con,
    historial_desde_persistencia,
};
use glory_harness_core::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::{LlavesProveedor, catalogo_proveedores};
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::sandbox::RespaldoArchivos;
use glory_harness_core::AgentPersistence;
use glory_harness_core::ProgramadorTareas;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

mod vault;

/// [039A-3 P3] Tramo rebobinado pendiente de restaurar archivos (acción
/// EXPLÍCITA tras "volver a punto"; nunca automática). `turnos` son los ids
/// que el rewind devolvió (los turnos borrados) y `archivos` las rutas que
/// tocaron (para que el front ofrezca la restauración). Se limpia al cambiar
/// de conversación o tras restaurar.
#[derive(Clone)]
struct TramoRewind {
    turnos: Vec<Uuid>,
    archivos: Vec<String>,
}

/// Sesión viva del núcleo (misma construcción que `chat`/`run` del CLI, pero
/// con `PersistenciaSqlite` en vez de memoria). El runtime va tras un Mutex
/// para que `reconfigurar_sesion` pueda sustituirlo sin invalidar la sesión.
struct Sesion {
    runtime: Mutex<Arc<AgentRuntime>>,
    persistencia: Arc<PersistenciaSqlite>,
    user_id: Uuid,
    conversacion_id: Mutex<Uuid>,
    /// Turno cuyo `turno-fin` aún no se emitió (para marcar `cancelado`).
    turno_id: Mutex<Option<Uuid>>,
    /// [039A-3 P3] Vault de respaldos del workspace (hook de `SandboxArchivos`
    /// que el core ya tiene cableado): el árbol/índice viven aquí, y el hook
    /// (que no conoce el turno) se fija con `fijar_contexto` en cada turno.
    vault: Arc<vault::VaultArchivos>,
    /// [039A-3 P3] Último tramo rebobinado pendiente de restaurar (ver
    /// `TramoRewind`). Se puebla en `rewind_conversacion`.
    tramo_rewind: Mutex<Option<TramoRewind>>,
    /// Meta del modo `meta` (prefijo `[META: …]` en cada turno).
    meta: Mutex<Option<String>>,
    /// Modo con el que se construyó el runtime (`meta` activa el prefijo).
    modo: Mutex<String>,
    modelo: Mutex<String>,
    workspace: String,
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

/// [039A-3 P1] Acumulador de `AgenteEvento::Usage` de un turno. Un turno con
/// N tools emite N Usage parciales (uno por `llm_llamada`): los tokens se
/// SUMAN y provider/modelo se conservan los del ÚLTIMO Usage (el que respondió
/// de verdad tras la cadena de fallback). Al `turno-fin` ok se persiste en
/// `turnos` (los campos reales, no los del turno solicitado).
#[derive(Default)]
struct UsoAcumulado {
    tokens_prompt: u32,
    tokens_complecion: u32,
    provider: Option<String>,
    modelo: Option<String>,
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

#[derive(serde::Serialize, Clone)]
struct InfoSesion {
    modelo: String,
    workspace: String,
    proveedores: Vec<ProveedorConteo>,
    conversacion: InfoConversacion,
    #[serde(skip_serializing_if = "Option::is_none")]
    aviso: Option<String>,
}

#[derive(serde::Serialize, Clone)]
struct ProveedorConteo {
    nombre: String,
    claves: usize,
}

fn conteos(llaves: &LlavesProveedor) -> Vec<ProveedorConteo> {
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

/// [039A-1 04-09 H5] Nombre breve de conversación desde el primer mensaje del
/// usuario: primeras ~4 palabras (o ~42 caracteres), una sola línea, sin
/// prefijos de modo (`[META: …]`). Si no hay palabras, "Conversación".
fn titulo_auto_desde_mensaje(mensaje: &str) -> String {
    let limpio = mensaje
        .trim()
        .lines()
        .next()
        .unwrap_or("")
        .trim();
    let sin_meta = limpio
        .strip_prefix("[META:")
        .and_then(|resto| resto.find(']').map(|i| &resto[i + 1..]))
        .unwrap_or(limpio)
        .trim();
    if sin_meta.is_empty() {
        return "Conversación".into();
    }
    let palabras: Vec<&str> = sin_meta.split_whitespace().collect();
    let mut out = String::new();
    for (i, p) in palabras.iter().enumerate() {
        if i == 4 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(p);
        if out.chars().count() >= 42 {
            break;
        }
    }
    if out.is_empty() {
        "Conversación".into()
    } else {
        out
    }
}

fn sesion_actual(estado: &State<'_, Estado>) -> Result<Arc<Sesion>, String> {
    match estado.sesion.lock() {
        Ok(g) => g.clone().ok_or_else(|| "abre la sesión primero".to_string()),
        Err(_) => Err("sesión bloqueada por otro turno".into()),
    }
}

/// [039A-3 P3] Cablea un vault como hook del sandbox del runtime: todas las
/// escrituras del agente (file_write/file_patch/aplicar_plan/todo) pasan por
/// el MISMO `Arc<SandboxArchivos>` que el registry inyecta al contexto, así
/// que el respaldo cubre TODA escritura, no solo la de una tool.
fn cablear_vault_a(runtime: &Arc<AgentRuntime>, vault: &Arc<vault::VaultArchivos>) {
    if let Some(sandbox) = runtime.registry.sandbox() {
        sandbox.con_respaldo(Some(Arc::clone(vault) as Arc<dyn RespaldoArchivos>));
    }
}

/// [039A-3 P3] Crea el vault sobre la raíz REAL del sandbox del runtime (la
/// que el core usa para validar; no una reconstruida) y lo cablea. Devuelve
/// el vault para guardarlo en la sesión. Sin sandbox (modo no-local), crea el
/// vault igualmente sobre el workspace del harness (no se cablea, no hay
/// escrituras de agente que respaldar).
fn crear_y_cablear_vault(
    runtime: &Arc<AgentRuntime>,
    workspace: Option<&std::path::Path>,
) -> Arc<vault::VaultArchivos> {
    let raiz = runtime
        .registry
        .sandbox()
        .map(|sb| sb.raiz().to_path_buf())
        .or_else(|| workspace.map(std::path::Path::to_path_buf))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
    let vault = Arc::new(vault::VaultArchivos::nuevo(&raiz));
    cablear_vault_a(runtime, &vault);
    vault
}

fn info_desde_sesion(sesion: &Sesion) -> Result<InfoSesion, String> {
    let conv_id = sesion
        .conversacion_id
        .lock()
        .map(|g| *g)
        .map_err(|_| "sesión bloqueada".to_string())?;
    let modelo = sesion
        .modelo
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    let conversacion = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == conv_id)
        .ok_or_else(|| "conversación actual no encontrada".to_string())?;
    Ok(InfoSesion {
        modelo,
        workspace: sesion.workspace.clone(),
        proveedores: conteos(&LlavesProveedor::from_env()),
        conversacion,
        aviso: None,
    })
}

/// Núcleo de apertura compartido por `abrir_sesion` y `elegir_workspace`:
/// SQLite (o memoria con aviso) + `user_id` estable + conversación inicial.
fn abrir_sesion_interna(
    estado: &State<'_, Estado>,
    provider: Option<String>,
    modelo: Option<String>,
    dir: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
    nueva_conversacion: bool,
) -> Result<InfoSesion, String> {
    glory_harness::cargar_env_usuario();
    /* F3: la BD vive en el perfil del usuario; si no abre (permisos, disco),
     * la sesión sigue en memoria y la UI muestra el aviso (fail-open: nunca
     * se deja al usuario sin agente por un fallo de disco). */
    let (persistencia, aviso) = match PersistenciaSqlite::ruta_bd_app() {
        Some(ruta) => match PersistenciaSqlite::abrir(&ruta) {
            Ok(p) => (p, None),
            Err(e) => (
                PersistenciaSqlite::en_memoria().map_err(|e| e.to_string())?,
                Some(format!("BD no disponible ({}): sesión en memoria", e)),
            ),
        },
        None => (
            PersistenciaSqlite::en_memoria().map_err(|e| e.to_string())?,
            Some("sin ruta de datos: sesión en memoria".to_string()),
        ),
    };
    /* [039A-1 04-09 H3] Workspace real: si el llamador no aporta `dir`, se
     * usa el guardado en config (si existe); sin config, `None` → cwd del
     * proceso (último recurso). */
    let dir = match dir {
        Some(d) => Some(d),
        None => match persistencia
            .config_leer("workspace")
            .map_err(|e| e.to_string())?
        {
            Some(guardado) if !guardado.trim().is_empty() => Some(guardado),
            _ => None,
        },
    };
    /* [039A-1 04-09 H7] Nivel de razonamiento: lo resuelve el backend desde
     * config cuando el llamador no lo aporta (el front no hace roundtrip). */
    let razonamiento = match razonamiento {
        Some(r) => Some(r),
        None => persistencia
            .config_leer("nivelRazonamiento")
            .map_err(|e| e.to_string())?,
    };
    let opciones = OpcionesRun {
        provider,
        modelo,
        dir: dir.map(std::path::PathBuf::from),
        modo,
        razonamiento,
    };
    /* `user_id` estable entre reinicios: las conversaciones pertenecen a un
     * usuario y sobreviven al cierre (tabla `config`, clave `user_id`). */
    let user_id = match persistencia
        .config_leer("user_id")
        .map_err(|e| e.to_string())?
    {
        Some(guardado) => Uuid::parse_str(guardado.trim())
            .map_err(|_| "user_id guardado corrupto".to_string())?,
        None => {
            let nuevo = Uuid::new_v4();
            persistencia
                .config_guardar("user_id", &nuevo.as_hyphenated().to_string())
                .map_err(|e| e.to_string())?;
            nuevo
        }
    };
    persistencia.con_skills_base(user_id);
    /* [039A-1 04-09 H4] No crear conversación nueva en cada apertura: si el
     * llamador no pide una nueva explícitamente y ya existe alguna NO
     * archivada, se reutiliza la más reciente (la lista viene ordenada por
     * `actualizada_en` DESC). Solo se crea si no hay ninguna. */
    let conv_id = if !nueva_conversacion {
        persistencia
            .conversaciones_listar(user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| !c.archivada)
            .map(|c| c.id)
    } else {
        None
    };
    let conv_id = match conv_id {
        Some(id) => id,
        None => persistencia
            .conversacion_crear(user_id, "Nueva conversación")
            .map_err(|e| e.to_string())?,
    };
    let persistencia = Arc::new(persistencia);
    let programador = Arc::clone(&persistencia);
    let harness = construir_harness_con(
        &opciones,
        Arc::clone(&persistencia) as Arc<dyn AgentPersistence>,
        programador,
        user_id,
    );
    /* El título real de la conversación reutilizada/creada (no asumir que es
     * "Nueva conversación": H4 puede reutilizar una con nombre propio). */
    let conversacion = persistencia
        .conversaciones_listar(user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == conv_id)
        .ok_or_else(|| "conversación inicial no encontrada".to_string())?;
    /* [039A-1 04-09 H3] Primer arranque sin workspace guardado: persistir el
     * que el constructor resolvió (cwd del proceso o `--dir`) para que el
     * modal muestre la ruta real y el próximo arranque la reutilice. */
    if persistencia
        .config_leer("workspace")
        .map_err(|e| e.to_string())?
        .is_none()
    {
        if let Some(ws) = harness.workspace.as_ref() {
            persistencia
                .config_guardar("workspace", &ws.to_string_lossy().into_owned())
                .map_err(|e| e.to_string())?;
        }
    }
    let info = InfoSesion {
        modelo: format!("{}/{}", harness.config.provider, harness.config.modelo),
        workspace: harness
            .workspace
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<desconocido>".into()),
        proveedores: conteos(&LlavesProveedor::from_env()),
        conversacion,
        aviso,
    };
    /* [039A-3 P3] Vault del workspace: se crea y se cablea al sandbox del
     * runtime ANTES de guardar la sesión. La raíz real la da el sandbox del
     * runtime (la misma que valida); si el harness no trae workspace ni
     * sandbox, la carpeta queda en el cwd del proceso (el vault se crea
     * igualmente, sin cablear: no hay escrituras que respaldar). */
    let vault = crear_y_cablear_vault(&harness.runtime, harness.workspace.as_deref());
    let sesion = Arc::new(Sesion {
        runtime: Mutex::new(harness.runtime),
        persistencia,
        user_id,
        conversacion_id: Mutex::new(conv_id),
        turno_id: Mutex::new(None),
        vault,
        tramo_rewind: Mutex::new(None),
        meta: Mutex::new(None),
        modo: Mutex::new(harness.config.modo.clone()),
        modelo: Mutex::new(info.modelo.clone()),
        workspace: info.workspace.clone(),
    });
    match estado.sesion.lock() {
        Ok(mut g) => {
            *g = Some(sesion);
            Ok(info)
        }
        Err(_) => Err("sesión bloqueada por otro turno".into()),
    }
}

/// Abre (o reabre) la sesión del núcleo con provider/modelo/dir/modo.
#[tauri::command]
fn abrir_sesion(
    estado: State<'_, Estado>,
    _app: AppHandle,
    provider: Option<String>,
    modelo: Option<String>,
    dir: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
) -> Result<InfoSesion, String> {
    abrir_sesion_interna(&estado, provider, modelo, dir, modo, razonamiento, false)
}

/// Ejecuta un turno real y reemite cada `AgenteEvento` a la UI.
/// Emite `agente-evento` por evento y `turno-fin` (`{ok, error?}`) al cerrar.
#[tauri::command]
async fn enviar_turno(
    estado: State<'_, Estado>,
    window: tauri::Window,
    mensaje: String,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
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
    let conv_id = sesion
        .conversacion_id
        .lock()
        .map(|g| *g)
        .map_err(|_| "sesión bloqueada".to_string())?;
    let meta = sesion
        .meta
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    /* Modo meta: el objetivo viaja como prefijo del turno (el historial
     * guarda el mensaje original, sin prefijo, para que se pueda releer).
     * La puerta es el modo del runtime: al salir de `meta` no hay que
     * limpiar nada, el prefijo deja de aplicarse solo. */
    let modo = sesion
        .modo
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    let mensaje_efectivo = match (modo.as_str(), meta) {
        ("meta", Some(m)) if !m.trim().is_empty() => format!("[META: {}]\n{}", m.trim(), mensaje),
        _ => mensaje.clone(),
    };
    /* El historial se lee ANTES de guardar el mensaje nuevo: si no, el turno
     * vería el mensaje del usuario duplicado (una vez en historial y otra
     * como `mensaje_usuario`). */
    let historial_previo = match sesion.persistencia.listar_mensajes(conv_id).await {
        Ok(mensajes) => historial_desde_persistencia(mensajes),
        Err(e) => return Err(e.to_string()),
    };
    /* El mensaje del usuario lo persiste el consumidor antes de llamar
     * (contrato del runtime): así el historial sobrevive al cierre aunque el
     * turno falle o se cancele antes de responder. */
    sesion
        .persistencia
        .guardar_mensaje(&MensajePersistido {
            id: Uuid::new_v4(),
            conversacion_id: conv_id,
            rol: "user".into(),
            contenido: mensaje.clone(),
            creado_en: chrono::Utc::now(),
        })
        .await
        .map_err(|e| e.to_string())?;
    sesion
        .persistencia
        .conversacion_tocar(conv_id)
        .await
        .map_err(|e| e.to_string())?;
    /* [039A-1 04-09 H5] Auto-nombre tras el primer mensaje: solo cuando el
     * título sigue siendo el default "Nueva conversación" y no había historial
     * previo (evita pisar renombres manuales y no re-nombra una conversación
     * ya autonombrada). El nombre sale del primer mensaje del usuario. */
    if historial_previo.is_empty() {
        let es_default = sesion
            .persistencia
            .conversaciones_listar(sesion.user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| c.id == conv_id)
            .map(|c| c.titulo == "Nueva conversación")
            .unwrap_or(false);
        if es_default {
            let nuevo = titulo_auto_desde_mensaje(&mensaje);
            sesion
                .persistencia
                .conversacion_renombrar(conv_id, sesion.user_id, &nuevo)
                .map_err(|e| e.to_string())?;
        }
    }
    let (tx_ev, mut rx_ev) = tokio::sync::mpsc::channel::<AgenteEvento>(64);
    let w = window.clone();
    let turno_id = Uuid::new_v4();
    /* El Arc se clona fuera del spawn: dentro no vale `?` y no se retiene el
     * Mutex durante el turno (reconfigurar puede sustituirlo entre turnos). */
    let runtime = sesion
        .runtime
        .lock()
        .map(|g| Arc::clone(&*g))
        .map_err(|_| "sesión bloqueada".to_string())?;
    /* [039A-3 P3] Fijar el contexto del vault para este turno (conversación +
     * turno): las escrituras del harness durante `ejecutar_turno` se
     * atribuyen a este tramo. El hook del núcleo no recibe el turno; el
     * desktop lo deja aquí ANTES de cada turno. */
    sesion.vault.fijar_contexto(vault::ContextoTurnoVault {
        conversacion_id: Some(conv_id),
        turno_id: Some(turno_id),
        tool_name: None,
    });
    /* [039A-3 P3] Al enviar un turno nuevo tras un "volver a punto" el tramo
     * pendiente de restaurar deja de ser el último (el usuario siguió
     * hablando en vez de restaurar): se limpia para no ofrecer una
     * restauración obsoleta. */
    if let Ok(mut g) = sesion.tramo_rewind.lock() {
        *g = None;
    }
    if let Ok(mut g) = sesion.turno_id.lock() {
        *g = Some(turno_id);
    }
    let handle = tauri::async_runtime::spawn(async move {
        // Al cerrar (ok, fallo o abort) se libera el flag para el próximo turno.
        let terminar = |w: &tauri::Window| {
            if let Some(e) = w.app_handle().try_state::<Estado>() {
                marcar_turno_terminado(&e);
            }
            if let Ok(mut g) = sesion.turno_id.lock() {
                *g = None;
            }
            /* [039A-3 P3] Limpiar el contexto del vault: el siguiente turno
             * vuelve a fijarlo; sin limpieza, una escritura fuera de turno
             * (p. ej. un setup posterior) se atribuiría al último turno. */
            sesion.vault.fijar_contexto(vault::ContextoTurnoVault::default());
        };
        // Reenvío en la misma tarea: el loop termina con Done (último evento).
        // [039A-3 P1] Se acumula el Usage real que emite el núcleo (cada
        // llm_llamada emite uno parcial; un turno con N tools acumula N) y se
        // propaga al cierre para persistirlo en `turnos`.
        let w_fw = w.clone();
        let uso_accum = std::sync::Arc::new(std::sync::Mutex::new(UsoAcumulado::default()));
        let uso_reenvio = Arc::clone(&uso_accum);
        let reenvio = tauri::async_runtime::spawn(async move {
            while let Some(ev) = rx_ev.recv().await {
                let es_done = matches!(ev, AgenteEvento::Done { .. });
                if let AgenteEvento::Usage {
                    tokens_prompt,
                    tokens_complecion,
                    provider,
                    modelo,
                    ..
                } = &ev
                {
                    if let Ok(mut u) = uso_reenvio.lock() {
                        u.tokens_prompt = u.tokens_prompt.saturating_add(*tokens_prompt);
                        u.tokens_complecion = u.tokens_complecion.saturating_add(*tokens_complecion);
                        if let Some(p) = provider {
                            u.provider = Some(p.clone());
                        }
                        if let Some(m) = modelo {
                            u.modelo = Some(m.clone());
                        }
                    }
                }
                let _ = w_fw.emit("agente-evento", &ev);
                if es_done {
                    break;
                }
            }
        });
        let resultado = runtime
            .ejecutar_turno(
                sesion.user_id,
                turno_id,
                conv_id,
                historial_previo,
                mensaje_efectivo,
                &tx_ev,
            )
            .await;
        drop(tx_ev);
        let _ = reenvio.await;
        match resultado {
            Ok(()) => {
                // [039A-3 P1] Persistir el uso/modelo REAL del turno (los
                // campos que el runtime guardó son 0 / solicitado). El UPDATE
                // es best-effort: si falla, el pie de turno no se bloquea.
                if let Ok(uso) = uso_accum.lock() {
                    if uso.tokens_prompt > 0 || uso.tokens_complecion > 0 || uso.provider.is_some() {
                        let _ = sesion.persistencia.turno_actualizar_uso(
                            turno_id,
                            uso.tokens_prompt,
                            uso.tokens_complecion,
                            uso.provider.as_deref(),
                            uso.modelo.as_deref(),
                        );
                    }
                }
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

/// Aborta el turno en curso (el runtime se detiene al cerrar el canal) y lo
/// marca `cancelado` en la BD para que no quede como pendiente eternamente.
#[tauri::command]
fn cancelar_turno(estado: State<'_, Estado>, window: tauri::Window) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    match estado.turno.lock() {
        Ok(mut t) => {
            if let Some(h) = t.handle.take() {
                h.abort();
            }
            t.activo = false;
        }
        Err(_) => return Err("no se pudo acceder al turno".into()),
    }
    let turno_id = sesion
        .turno_id
        .lock()
        .map(|mut g| g.take())
        .map_err(|_| "sesión bloqueada".to_string())?;
    tauri::async_runtime::spawn(async move {
        if let Some(id) = turno_id {
            let _ = sesion
                .persistencia
                .finalizar_turno(id, "cancelado", Some("abortado por el usuario"))
                .await;
        }
        let _ = window.emit(
            "turno-fin",
            serde_json::json!({"ok": false, "error": "cancelado por el usuario"}),
        );
    });
    Ok(())
}

/// Cambia provider/modelo/modo SIN perder la conversación: reconstruye solo
/// el runtime sobre la misma persistencia, usuario y conversación. Falla con
/// turno en curso. El workspace no cambia (para eso, `elegir_workspace`).
#[tauri::command]
fn reconfigurar_sesion(
    estado: State<'_, Estado>,
    provider: Option<String>,
    modelo: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
) -> Result<InfoSesion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    glory_harness::cargar_env_usuario();
    /* `dir` = workspace actual: con `None` el constructor resolvería el cwd
     * del proceso y el agente cambiaría de carpeta sin avisar. */
    let opciones = OpcionesRun {
        provider,
        modelo,
        dir: Some(std::path::PathBuf::from(sesion.workspace.clone())),
        modo,
        razonamiento: match razonamiento {
            Some(r) => Some(r),
            /* [039A-1 04-09 H7] Al reconfigurar por modelo/modo sin pasar
             * razonamiento, se conserva el del runtime actual (no se pierde
             * el nivel ya aplicado con un `None` que resetea a default). */
            None => sesion
                .runtime
                .lock()
                .map(|g| g.turno_config.nivel_razonamiento.clone())
                .map_err(|_| "sesión bloqueada".to_string())?,
        },
    };
    let harness = construir_harness_con(
        &opciones,
        Arc::clone(&sesion.persistencia) as Arc<dyn AgentPersistence>,
        Arc::clone(&sesion.persistencia) as Arc<dyn ProgramadorTareas>,
        sesion.user_id,
    );
    let modelo_nuevo = format!("{}/{}", harness.config.provider, harness.config.modelo);
    /* [039A-3 P3] El nuevo runtime trae un sandbox fresco SIN el hook: se
     * re-cablea el vault de la sesión (misma raíz de workspace). */
    cablear_vault_a(&harness.runtime, &sesion.vault);
    sesion
        .runtime
        .lock()
        .map(|mut g| *g = harness.runtime)
        .map_err(|_| "sesión bloqueada".to_string())?;
    sesion
        .modelo
        .lock()
        .map(|mut g| *g = modelo_nuevo)
        .map_err(|_| "sesión bloqueada".to_string())?;
    sesion
        .modo
        .lock()
        .map(|mut g| *g = harness.config.modo.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    info_desde_sesion(&sesion)
}

/// Responde una petición de aprobación (canal F2, tres vías).
#[tauri::command]
fn responder_aprobacion(
    estado: State<'_, Estado>,
    id: String,
    respuesta: String,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    let runtime = sesion
        .runtime
        .lock()
        .map(|g| Arc::clone(&*g))
        .map_err(|_| "sesión bloqueada".to_string())?;
    let r = match respuesta.as_str() {
        "aprobar" => RespuestaAprobacion::Aprobar,
        "siempre" => RespuestaAprobacion::Siempre,
        _ => RespuestaAprobacion::Rechazar,
    };
    runtime.responder_aprobacion(&id, r).map_err(|e| e.to_string())
}

/// Peticiones de aprobación aún pendientes (para pintar tarjetas al cerrar).
#[tauri::command]
fn pendientes_aprobacion(
    estado: State<'_, Estado>,
) -> Result<Vec<PeticionAprobacion>, String> {
    match estado.sesion.lock() {
        Ok(g) => Ok(g
            .as_ref()
            .map(|s| {
                s.runtime
                    .lock()
                    .map(|r| r.peticiones_aprobacion_pendientes())
                    .unwrap_or_default()
            })
            .unwrap_or_default()),
        Err(_) => Err("sesión bloqueada".into()),
    }
}

// --- F4: conversaciones (CRUD) ---

/// Crea una conversación y la deja como actual (falla si hay turno en curso).
#[tauri::command]
fn conversacion_nueva(
    estado: State<'_, Estado>,
    titulo: Option<String>,
) -> Result<InfoConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    let titulo = titulo
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Nueva conversación".into());
    let id = sesion
        .persistencia
        .conversacion_crear(sesion.user_id, &titulo)
        .map_err(|e| e.to_string())?;
    sesion
        .conversacion_id
        .lock()
        .map(|mut g| *g = id)
        .map_err(|_| "sesión bloqueada".to_string())?;
    /* [039A-3 P3] Conversación nueva = contexto nuevo: no hay tramo previo
     * que restaurar desde aquí. */
    if let Ok(mut g) = sesion.tramo_rewind.lock() {
        *g = None;
    }
    Ok(InfoConversacion {
        id,
        titulo,
        archivada: false,
        actualizada_en: chrono::Utc::now(),
    })
}

/// Lista las conversaciones del usuario (recientes primero, incluye archivo).
#[tauri::command]
fn listar_conversaciones(
    estado: State<'_, Estado>,
) -> Result<Vec<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
struct CargaConversacion {
    id: Uuid,
    titulo: String,
    mensajes: Vec<MensajePersistido>,
    /// [039A-1 04-09 H6] Acciones (tools) de la conversación en orden de
    /// ejecución, para repintar los bloques `.herramienta` al recargar.
    acciones: Vec<glory_harness::AccionRecuperada>,
    /// [039A-3 P1] Uso/modelo real del último turno (para repintar el pie de
    /// turno al recargar). `None` si no hay turno con uso registrado.
    ultimo_uso: Option<UsoTurnoPersistido>,
    /// [039A-3 P3] Archivos que tocó el último tramo rebobinado ("volver a
    /// punto"), listos para la acción EXPLÍCITA "restaurar archivos de este
    /// tramo". Vacío cuando la carga no viene de un rewind (no hay nada que
    /// restaurar). El front lo ofrece solo cuando no está vacío.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    archivos_tramo: Vec<String>,
}

/// [039A-3 P1] Uso real de un turno persistido (serializable al front).
#[derive(serde::Serialize)]
struct UsoTurnoPersistido {
    provider: String,
    modelo: String,
    tokens_prompt: u32,
    tokens_complecion: u32,
}

/// Carga una conversación como actual con su historial (falla con turno vivo).
#[tauri::command]
async fn cargar_conversacion(
    estado: State<'_, Estado>,
    id: String,
) -> Result<CargaConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.titulo)
        .ok_or_else(|| "conversación no encontrada".to_string())?;
    let mensajes = sesion
        .persistencia
        .listar_mensajes(id)
        .await
        .map_err(|e| e.to_string())?;
    let acciones = sesion
        .persistencia
        .acciones_por_conversacion(id)
        .map_err(|e| e.to_string())?;
    let ultimo_uso = sesion
        .persistencia
        .turno_ultimo_uso_por_conversacion(id)
        .map_err(|e| e.to_string())?
        .map(|(provider, modelo, tokens_prompt, tokens_complecion)| UsoTurnoPersistido {
            provider,
            modelo,
            tokens_prompt,
            tokens_complecion,
        });
    /* [039A-3 P3] Al cambiar de conversación se limpia el tramo pendiente de
     * restaurar (pertenece a la conversación anterior): su restauración ya no
     * es accesible desde aquí. */
    sesion
        .conversacion_id
        .lock()
        .map(|mut g| *g = id)
        .map_err(|_| "sesión bloqueada".to_string())?;
    sesion
        .tramo_rewind
        .lock()
        .map(|mut g| *g = None)
        .map_err(|_| "sesión bloqueada".to_string())?;
    Ok(CargaConversacion {
        id,
        titulo,
        mensajes,
        acciones,
        ultimo_uso,
        archivos_tramo: Vec::new(),
    })
}

/// Renombra una conversación (`false` = no existe o no es del usuario).
#[tauri::command]
fn renombrar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    titulo: String,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = titulo.trim();
    if titulo.is_empty() {
        return Err("título vacío".into());
    }
    sesion
        .persistencia
        .conversacion_renombrar(id, sesion.user_id, titulo)
        .map_err(|e| e.to_string())
}

/// Archiva/desarchiva (`false` = no existe o no es del usuario).
#[tauri::command]
fn archivar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    archivada: bool,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_archivar(id, sesion.user_id, archivada)
        .map_err(|e| e.to_string())
}

/// Elimina con mensajes y turnos; si era la actual, crea una nueva vacía.
#[tauri::command]
fn eliminar_conversacion(
    estado: State<'_, Estado>,
    id: String,
) -> Result<InfoConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_eliminar(id, sesion.user_id)
        .map_err(|e| e.to_string())?;
    /* [039A-3 P3] Al eliminar la conversación se limpia su índice del vault y
     * se hace GC de los hashes que quedaron huérfanos. */
    sesion.vault.eliminar_conversacion(id);
    let actual = sesion
        .conversacion_id
        .lock()
        .map(|g| *g)
        .map_err(|_| "sesión bloqueada".to_string())?;
    if actual == id {
        /* El tramo pendiente pertenecía a la conversación borrada: se limpia. */
        if let Ok(mut g) = sesion.tramo_rewind.lock() {
            *g = None;
        }
        return conversacion_nueva(estado, None);
    }
    info_desde_sesion(&sesion).map(|i| i.conversacion)
}

/// [039A-3 P2] Rebobina la conversación hasta un mensaje de usuario.
///
/// Con `editar=false` (volver a este punto) conserva el mensaje objetivo como
/// último mensaje; con `editar=true` lo borra para reescribirlo (editar+enviar
/// hace rewind aquí y luego `enviar_turno` persiste el texto nuevo). Borra en
/// una transacción mensajes/turnos/acciones del tramo posterior. Falla con
/// turno en curso y si el mensaje no es de la conversación actual del usuario.
/// Devuelve la `CargaConversacion` resultante para que el front se reconcilie
/// (repintar sin recargar).
#[tauri::command]
async fn rewind_conversacion(
    estado: State<'_, Estado>,
    hasta_mensaje_id: String,
    editar: bool,
) -> Result<CargaConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    let msg_id = Uuid::parse_str(hasta_mensaje_id.trim())
        .map_err(|_| "id de mensaje inválido".to_string())?;
    let conv_id = sesion
        .conversacion_id
        .lock()
        .map(|g| *g)
        .map_err(|_| "sesión bloqueada".to_string())?;
    /* [039A-3 P3] El rewind devuelve los ids de los turnos borrados: con ellos
     * el vault localiza las rutas que tocó el tramo. En "volver a punto"
     * (editar=false) el tramo queda pendiente de una restauración EXPLÍCITA;
     * en "editar+reenviar" (editar=true) se limpia porque el reenvío inmediato
     * va a reescribir los archivos (ofrecer restaurar sería incoherente). */
    let turnos_tramo = sesion
        .persistencia
        .rewind_conversacion(conv_id, msg_id, sesion.user_id, editar)
        .map_err(|e| e.to_string())?;
    let archivos_tramo: Vec<String> = if editar {
        Vec::new()
    } else {
        sesion
            .vault
            .archivos_del_tramo(&turnos_tramo)
            .into_iter()
            .map(|e| e.ruta_relativa)
            .collect()
    };
    sesion
        .tramo_rewind
        .lock()
        .map(|mut g| {
            *g = if editar {
                None
            } else {
                Some(TramoRewind {
                    archivos: archivos_tramo.clone(),
                    turnos: turnos_tramo,
                })
            }
        })
        .map_err(|_| "sesión bloqueada".to_string())?;
    // Reconstruir la carga resultante (igual que cargar_conversacion).
    let titulo = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == conv_id)
        .map(|c| c.titulo)
        .ok_or_else(|| "conversación no encontrada".to_string())?;
    let mensajes = sesion
        .persistencia
        .listar_mensajes(conv_id)
        .await
        .map_err(|e| e.to_string())?;
    let acciones = sesion
        .persistencia
        .acciones_por_conversacion(conv_id)
        .map_err(|e| e.to_string())?;
    let ultimo_uso = sesion
        .persistencia
        .turno_ultimo_uso_por_conversacion(conv_id)
        .map_err(|e| e.to_string())?
        .map(|(provider, modelo, tokens_prompt, tokens_complecion)| UsoTurnoPersistido {
            provider,
            modelo,
            tokens_prompt,
            tokens_complecion,
        });
    Ok(CargaConversacion {
        id: conv_id,
        titulo,
        mensajes,
        acciones,
        ultimo_uso,
        archivos_tramo,
    })
}

/// [039A-3 P3] Restaura los archivos del último tramo rebobinado ("volver a
/// punto"): acción EXPLÍCITA, nunca automática. Comprueba la fuente de cada
/// ruta contra su último respaldo GLOBAL (si alguien editó fuera del harness,
/// NO toca y avisa). Nunca borra archivos. Tras restaurar se limpia el tramo
/// pendiente (las escrituras deshechas se podan del índice y se hace GC).
#[tauri::command]
fn restaurar_archivos_tramo(estado: State<'_, Estado>) -> Result<RestauracionTramo, String> {
    let sesion = sesion_actual(&estado)?;
    if estado
        .turno
        .lock()
        .map(|t| t.activo)
        .unwrap_or(true)
    {
        return Err("hay un turno en curso".into());
    }
    let tramo = sesion
        .tramo_rewind
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "sesión bloqueada".to_string())?
        .ok_or_else(|| "no hay ningún tramo rebobinado pendiente de restaurar".to_string())?;
    let archivos = tramo.archivos.clone();
    let resultado = sesion.vault.restaurar_tramo(&tramo.turnos);
    /* Tras restaurar (o al decidir no hacerlo por completo) el tramo ya no
     * está pendiente: la próxima "restaurar" volvería a intentar los mismos
     * turnos (no-op tras la purga), así que se limpia el estado. */
    sesion
        .tramo_rewind
        .lock()
        .map(|mut g| *g = None)
        .map_err(|_| "sesión bloqueada".to_string())?;
    Ok(RestauracionTramo {
        archivos,
        restaurados: resultado.restaurados,
        omitidos: resultado.omitidos,
    })
}

/// [039A-3 P3] Resultado de la restauración explícita de un tramo, para que
/// el front muestre qué se restauró y qué se omitió (y por qué).
#[derive(serde::Serialize)]
struct RestauracionTramo {
    /// Rutas del tramo que se intentaron restaurar (para el aviso).
    archivos: Vec<String>,
    restaurados: Vec<vault::RestauracionArchivo>,
    omitidos: Vec<vault::RestauracionArchivo>,
}

// --- F4: catálogo, config, workspace, meta ---

#[derive(serde::Serialize)]
struct ProveedorInfo {
    id: &'static str,
    modelos: Vec<&'static str>,
    claves: usize,
}

/// Proveedores/modelos REALES del allowlist del núcleo + nº de claves. La UI
/// ya no duplica la tabla: el selector se alimenta de aquí.
#[tauri::command]
fn proveedores_disponibles() -> Vec<ProveedorInfo> {
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
fn config_leer(estado: State<'_, Estado>, clave: String) -> Result<Option<String>, String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .config_leer(clave.trim())
        .map_err(|e| e.to_string())
}

/// Guarda una clave de configuración (`provider_defecto`, `modelo_defecto`…).
#[tauri::command]
fn config_guardar(
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

/// Diálogo nativo de carpeta → reabre la sesión sobre ese workspace. Si el
/// usuario cancela, devuelve la sesión actual sin cambios (no es un error).
/// [039A-1 04-09 H3] La ruta elegida se persiste en config (`workspace`) para
/// que el próximo arranque la use y el modal muestre la real.
#[tauri::command]
fn elegir_workspace(estado: State<'_, Estado>) -> Result<InfoSesion, String> {
    let actual = sesion_actual(&estado)?;
    let carpeta = rfd::FileDialog::new()
        .set_title("Elegir carpeta de trabajo del agente")
        .pick_folder();
    match carpeta {
        Some(dir) => {
            let ruta = dir.to_string_lossy().into_owned();
            /* Persistir la ruta elegida ANTES de reabrir: el nuevo arranque la
             * usará como workspace por defecto. */
            actual
                .persistencia
                .config_guardar("workspace", &ruta)
                .map_err(|e| e.to_string())?;
            abrir_sesion_interna(&estado, None, None, Some(ruta), None, None, true)
        }
        None => info_desde_sesion(&actual),
    }
}

/// Fija la meta del modo `meta` (`None`/vacía = sin meta). Normalizada.
#[tauri::command]
fn actualizar_meta(
    estado: State<'_, Estado>,
    meta: Option<String>,
) -> Result<Option<String>, String> {
    let sesion = sesion_actual(&estado)?;
    let normalizada = meta
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());
    sesion
        .meta
        .lock()
        .map(|mut g| *g = normalizada.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    Ok(normalizada)
}

fn main() {
    tauri::Builder::default()
        .manage(Estado::default())
        .invoke_handler(tauri::generate_handler![
            abrir_sesion,
            reconfigurar_sesion,
            enviar_turno,
            cancelar_turno,
            responder_aprobacion,
            pendientes_aprobacion,
            conversacion_nueva,
            listar_conversaciones,
            cargar_conversacion,
            rewind_conversacion,
            restaurar_archivos_tramo,
            renombrar_conversacion,
            archivar_conversacion,
            eliminar_conversacion,
            proveedores_disponibles,
            config_leer,
            config_guardar,
            elegir_workspace,
            actualizar_meta,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("[glory-harness-desktop] error fatal: {e}");
            std::process::exit(1);
        });
}
