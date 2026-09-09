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

use glory_harness::servicio::{Apertura, OpcionesSesion, SesionComun};
use glory_harness::{InfoConversacion, PersistenciaSqlite, Workspace};
use glory_harness_core::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::{catalogo_proveedores, AiMessage, LlavesProveedor};
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::sandbox::RespaldoArchivos;
use glory_harness_core::AgentPersistence;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

mod archivo;
mod filesystem;
mod git;
mod conversaciones;
mod navegador;
mod sesion;
mod turno;
mod vault;
mod workspaces;

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

/// [039A-3 P5] Estado por panel (hasta 2, M1: 1 runtime compartido, turnos NO
/// simultáneos). Cada panel conoce la conversación que muestra, el turno que
/// está ejecutando (para marcarlo `cancelado`) y su tramo rebobinado
/// por restaurar. M1 no tiene concurrencia real: el guard de turno
/// es global y un único `Mutex` por mapa serializa el acceso, así que los
/// campos son planos (sin Mutex interno) y se copian los escalares antes de
/// cualquier `.await`.
struct PanelDatos {
    /// [069A-7] Conversación que muestra el panel. `None` = borrador: aún no
    /// hay conversación creada (create-on-write); se crea al enviar el primer
    /// mensaje, al pulsar "Nueva conversación" tras escribir, o al cargar una.
    conversacion_id: Option<Uuid>,
    turno_id: Option<Uuid>,
    tramo_rewind: Option<TramoRewind>,
}

/// Sesión viva del núcleo (misma construcción que `chat`/`run` del CLI, pero
/// con `PersistenciaSqlite` en vez de memoria). El runtime va tras un Mutex
/// para que `reconfigurar_sesion` pueda sustituirlo sin invalidar la sesión.
/// [039A-3 P5] Lo COMPARTIDO entre paneles vive aquí (runtime/persistencia/
/// vault/meta/modo/modelo/workspace); lo específico de cada conversación
/// (cuál muestra, turno en curso, tramo a restaurar) vive en `paneles`.
struct Sesion {
    /// Servicio común; Tauri conserva fuera de él solo su ciclo de vida.
    /// runtime/modelo/modo/workspace viven aquí (única fuente de verdad).
    comun: Mutex<SesionComun>,
    /// Datos INMUTABLES de la sesión (se fijan al abrir y nunca cambian), que
    /// se copian de `comun` para que los comandos CRUD no bloqueen el Mutex.
    persistencia: Arc<PersistenciaSqlite>,
    user_id: Uuid,
    /// [039A-3 P5] Panel principal (`"principal"`) y, si se abre, el lateral
    /// (`"lateral"`): mapa `panel_id → PanelDatos`. Máx 2 por decisión M1 del
    /// plan 039A-3 §2.2/§6.1. El front etiqueta cada comando/evento con el
    /// `panel_id` para saber a qué chat va.
    paneles: Mutex<std::collections::HashMap<String, PanelDatos>>,
    /// [039A-3 P3] Vault de respaldos del workspace (hook de `SandboxArchivos`
    /// que el core ya tiene cableado): el árbol/índice viven aquí, y el hook
    /// (que no conoce el turno) se fija con `fijar_contexto` en cada turno.
    vault: Arc<vault::VaultArchivos>,
    /// Objetivo del modo `meta` (el modo activo vive en `SesionComun`).
    meta: Mutex<Option<String>>,
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

#[derive(serde::Serialize, Clone)]
struct InfoSesion {
    modelo: String,
    workspace: String,
    proveedores: Vec<sesion::ProveedorConteo>,
    /// [069A-7] `None` = sin conversación (borrador, create-on-write). Antes
    /// era siempre alguna; ahora la apertura con lista vacía o tras borrar la
    /// última deja el panel sin conversación hasta el primer mensaje.
    conversacion: Option<InfoConversacion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    aviso: Option<String>,
}

fn sesion_actual(estado: &State<'_, Estado>) -> Result<Arc<Sesion>, String> {
    match estado.sesion.lock() {
        Ok(g) => g
            .clone()
            .ok_or_else(|| "abre la sesión primero".to_string()),
        Err(_) => Err("sesión bloqueada por otro turno".into()),
    }
}

/// [039A-3 P5] `panel_id` canónico para el panel principal. Los comandos de
/// turno aceptan `panel_id` con este default para no romper el front actual
/// (que aún no lo envía) durante la transición a 2 paneles.
const PANEL_PRINCIPAL: &str = "principal";

/// [039A-3 P5] Normaliza el `panel_id` que llega del front: `None`/vacío →
/// panel principal. Sin normalizar, un front antiguo (sin `panel_id`) apuntaría
/// a un panel inexistente y todo turno fallaría con "panel no encontrado".
fn normalizar_panel(panel_id: Option<String>) -> String {
    match panel_id {
        Some(p) if !p.trim().is_empty() => p.trim().to_string(),
        _ => PANEL_PRINCIPAL.to_string(),
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

/// [039A-3 P5] `InfoSesion` de un panel concreto: describe la conversación
/// que ese panel muestra (no "la actual" global, que ya no existe). Lo usa
/// `abrir_sesion`/`elegir_workspace` (panel principal) y los comandos que
/// devuelven la sesión tras actuar sobre un panel.
/// [069A-7] El panel puede estar en borrador (`None`), p. ej. tras abrir con
/// lista vacía o eliminar la última conversación.
fn info_de_panel(sesion: &Sesion, panel_id: &str) -> Result<InfoSesion, String> {
    let conv_id = conversaciones::conv_id_de_panel(sesion, panel_id)?;
    match conv_id {
        Some(cid) => info_de_conversacion(sesion, cid),
        None => info_sin_conversacion(sesion),
    }
}

/// [069A-7] Base de `InfoSesion` con la conversación de un panel: carga la
/// fila si existe (error si no), o devuelve sin conversación si el panel está
/// en borrador (`None`).
fn info_de_conversacion(sesion: &Sesion, conv_id: Uuid) -> Result<InfoSesion, String> {
    let (modelo, workspace) = {
        let comun = sesion
            .comun
            .lock()
            .map_err(|_| "sesión bloqueada".to_string())?;
        (comun.modelo.clone(), comun.workspace.clone())
    };
    let conversacion = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == conv_id)
        .ok_or_else(|| "conversación actual no encontrada".to_string())?;
    Ok(InfoSesion {
        modelo,
        workspace,
        proveedores: sesion::conteos(&LlavesProveedor::from_env()),
        conversacion: Some(conversacion),
        aviso: None,
    })
}

/// [069A-7] `InfoSesion` para un panel sin conversación (borrador).
fn info_sin_conversacion(sesion: &Sesion) -> Result<InfoSesion, String> {
    let (modelo, workspace) = {
        let comun = sesion
            .comun
            .lock()
            .map_err(|_| "sesión bloqueada".to_string())?;
        (comun.modelo.clone(), comun.workspace.clone())
    };
    Ok(InfoSesion {
        modelo,
        workspace,
        proveedores: sesion::conteos(&LlavesProveedor::from_env()),
        conversacion: None,
        aviso: None,
    })
}

fn apertura_a_info(apertura: Apertura) -> InfoSesion {
    InfoSesion {
        modelo: apertura.modelo,
        workspace: apertura.workspace,
        proveedores: apertura
            .proveedores
            .into_iter()
            .map(|p| sesion::ProveedorConteo {
                nombre: p.nombre,
                claves: p.claves,
            })
            .collect(),
        /* [069A-7] Si la apertura trae una conversación auto-creada vacía
         * (lista sin filas), el desktop la descarta y reporta borrador. La
         * conversación anclada a una existente sí se reporta. */
        conversacion: if apertura.conv_autocreada {
            None
        } else {
            Some(apertura.conversacion)
        },
        aviso: apertura.aviso,
    }
}

/// [079A-1 F5] Opciones de apertura de sesión: agrupa los parámetros de
/// `abrir_sesion_interna` para el límite de parámetros del gate.
#[derive(Default)]
struct OpcionesApertura {
    provider: Option<String>,
    modelo: Option<String>,
    dir: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
    nueva_conversacion: bool,
}

fn abrir_sesion_interna(
    estado: &State<'_, Estado>,
    app: &AppHandle,
    opciones: OpcionesApertura,
) -> Result<InfoSesion, String> {
    let OpcionesApertura {
        provider,
        modelo,
        dir,
        modo,
        razonamiento,
        nueva_conversacion,
    } = opciones;
    /* [069A-1 F5] Si el escritorio tiene soporte de navegador, crea el puerto
     * real y lo inyecta al runtime. Sin navegador → None (fail-closed). */
    let navegador: Option<Arc<dyn glory_harness_core::ports::NavegadorPort>> = app
        .try_state::<std::sync::Mutex<navegador::EstadoNavegador>>()
        .map(|_| Arc::new(navegador::NavegadorTauri::nuevo(app)) as Arc<_>);

    let (comun, apertura) = SesionComun::abrir(OpcionesSesion {
        provider,
        modelo,
        dir,
        modo,
        razonamiento,
        nueva_conversacion,
        navegador,
    })
    .map_err(|e| e.to_string())?;
    /* [069A-7] Create-on-write: si el servicio auto-creó una "Nueva
     * conversación" vacía (no había ninguna), se DESCARTA la fila y el panel
     * arranca en borrador. Si se ancló una existente, el panel la muestra. Se
     * capturan los ids ANTES de mover `apertura` a `apertura_a_info`. */
    let conv_inicial = apertura.conversacion.id;
    let autocreada = apertura.conv_autocreada;
    if autocreada {
        comun
            .persistencia
            .conversacion_eliminar(conv_inicial, comun.user_id)
            .map_err(|e| e.to_string())?;
    }
    let info = apertura_a_info(apertura);
    /* [039A-3 P3] Vault del workspace: se crea y se cablea al sandbox del
     * runtime. */
    let vault = crear_y_cablear_vault(
        &comun.runtime,
        (!comun.workspace.is_empty()).then(|| std::path::Path::new(&comun.workspace)),
    );
    let mut paneles = std::collections::HashMap::new();
    paneles.insert(
        PANEL_PRINCIPAL.to_string(),
        PanelDatos {
            /* [069A-7] `None` si se descartó la auto-creada (borrador); si la
             * apertura ancló una existente, es su id. */
            conversacion_id: if autocreada { None } else { Some(conv_inicial) },
            turno_id: None,
            tramo_rewind: None,
        },
    );
    let sesion = Arc::new(Sesion {
        persistencia: Arc::clone(&comun.persistencia),
        user_id: comun.user_id,
        comun: Mutex::new(comun.clone()),
        paneles: Mutex::new(paneles),
        vault,
        meta: Mutex::new(None),
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
    app: AppHandle,
    provider: Option<String>,
    modelo: Option<String>,
    dir: Option<String>,
    modo: Option<String>,
    razonamiento: Option<String>,
) -> Result<InfoSesion, String> {
    abrir_sesion_interna(
        &estado,
        &app,
        OpcionesApertura {
            provider,
            modelo,
            dir,
            modo,
            razonamiento,
            ..Default::default()
        },
    )
}
// --- [069A-Proyectos] Áreas de trabajo (workspaces) ---

/// [069A-Proyectos] Área registrada para la carpeta ACTIVA de la sesión
/// (`comun.workspace`). Si la ruta activa es un directorio real pero no está
/// registrada en la tabla, SE AUTO-REGISTRA (workspace implícito, como
/// opencode/claurst/grok usan `cwd` como área). Así el frontend siempre
/// recibe `activa: Some(...)` cuando hay workspaces o una ruta válida.
fn area_activa(sesion: &Sesion) -> Result<Option<Workspace>, String> {
    let workspace = sesion
        .comun
        .lock()
        .map(|c| c.workspace.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    if workspace.is_empty() || workspace == "<desconocido>" {
        // Sin ruta activa: el primer workspace registrado, o None.
        return sesion
            .persistencia
            .workspaces_listar(sesion.user_id)
            .map(|ws| ws.into_iter().next())
            .map_err(|e| e.to_string());
    }
    // La ruta activa de la sesión puede estar o no registrada en la tabla.
    match sesion
        .persistencia
        .workspace_por_ruta(sesion.user_id, &workspace)
        .map_err(|e| e.to_string())?
    {
        Some(ws) => Ok(Some(ws)),
        None => {
            // La ruta activa existe como directorio real pero no está
            // registrada: auto-registrarla.
            let path = std::path::Path::new(&workspace);
            if path.is_dir() {
                let nombre = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "Área de trabajo".to_string());
                sesion
                    .persistencia
                    .workspace_crear(sesion.user_id, &nombre, &workspace)
                    .map(Some)
                    .map_err(|e| e.to_string())
            } else {
                // La ruta persistida ya no existe (disco extraído, carpeta
                // borrada): primer workspace registrado o None.
                sesion
                    .persistencia
                    .workspaces_listar(sesion.user_id)
                    .map(|ws| ws.into_iter().next())
                    .map_err(|e| e.to_string())
            }
        }
    }
}

fn main() {
    tauri::Builder::default()
        .manage(Estado::default())
        .manage(std::sync::Mutex::new(navegador::EstadoNavegador::new()))
        .invoke_handler(tauri::generate_handler![
            abrir_sesion,
            sesion::reconfigurar_sesion,
            turno::enviar_turno,
            turno::cancelar_turno,
            sesion::responder_aprobacion,
            sesion::pendientes_aprobacion,
            conversaciones::conversacion_nueva,
            conversaciones::listar_conversaciones,
            conversaciones::cargar_conversacion,
            conversaciones::rewind_conversacion,
            conversaciones::restaurar_archivos_tramo,
            conversaciones::renombrar_conversacion,
            conversaciones::archivar_conversacion,
            conversaciones::eliminar_conversacion,
            sesion::proveedores_disponibles,
            sesion::config_leer,
            sesion::config_guardar,
            workspaces::elegir_workspace,
            workspaces::elegir_carpeta_proyecto,
            workspaces::workspaces_listar,
            workspaces::workspace_crear_o_activar,
            workspaces::workspace_activar_por_ruta,
            workspaces::workspace_renombrar,
            workspaces::workspace_eliminar,
            workspaces::actualizar_meta,
            archivo::leer_archivo,
            filesystem::workspace_info,
            filesystem::workspace_listar_entrada,
            filesystem::workspace_leer_archivo,
            filesystem::workspace_abrir_con,
            filesystem::workspace_buscar,
            git::workspace_git_estado,
            navegador::comandos::navegador_abrir,
            navegador::comandos::navegador_navegar,
            navegador::comandos::navegador_cerrar,
            navegador::comandos::navegador_mostrar,
            navegador::comandos::navegador_posicionar,
            navegador::comandos::navegador_capturar,
            navegador::comandos::navegador_js,
            navegador::comandos::navegador_cdp,
            navegador::comandos::navegador_click,
            navegador::comandos::navegador_rellenar,
            navegador::comandos::navegador_snapshot,
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("[glory-harness-desktop] error fatal: {e}");
            std::process::exit(1);
        });
}
