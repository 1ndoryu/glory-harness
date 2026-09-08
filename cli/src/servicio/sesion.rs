//! Servicio de sesión independiente del transporte.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use glory_harness_core::llm::{AiMessage, LlavesProveedor};
use glory_harness_core::ports::{MensajePersistido, NavegadorPort};
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::{AgentPersistence, ProgramadorTareas};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    cargar_env_usuario, construir_harness_con, historial_desde_persistencia, InfoConversacion,
    OpcionesRun, PersistenciaSqlite, VENTANA_MINIMA,
};

/// Error de una operación del servicio común.
#[derive(Debug)]
pub enum Error {
    Persistencia(String),
    Configuracion(String),
    Sesion(String),
    Turno(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Persistencia(msg) => write!(f, "error de persistencia: {msg}"),
            Self::Configuracion(msg) => write!(f, "error de configuración: {msg}"),
            Self::Sesion(msg) => write!(f, "error de sesión: {msg}"),
            Self::Turno(msg) => write!(f, "error de turno: {msg}"),
        }
    }
}

impl std::error::Error for Error {}

/// Resultado de una apertura de sesión.
#[derive(Debug, Clone)]
pub struct Apertura {
    pub modelo: String,
    pub workspace: String,
    pub proveedores: Vec<ProveedorConteo>,
    pub conversacion: InfoConversacion,
    /// [069A-7] `true` si esta apertura auto-creó una conversación vacía
    /// "Nueva conversación" porque no existía ninguna disponible (el servicio
    /// conserva su invariante "siempre hay conversación actual" para
    /// CLI/TUI/daemon). Los transportes create-on-write (web y desktop) usan
    /// este flag para DESCARTAR esa fila y arrancar en borrador; `info()`
    /// (p. ej. reconfigurar) siempre reporta `false`.
    pub conv_autocreada: bool,
    pub aviso: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProveedorConteo {
    pub nombre: String,
    pub claves: usize,
}

/// Configuración externa para abrir una sesión.
#[derive(Clone)]
pub struct OpcionesSesion {
    pub provider: Option<String>,
    pub modelo: Option<String>,
    pub dir: Option<String>,
    pub modo: Option<String>,
    pub razonamiento: Option<String>,
    pub nueva_conversacion: bool,
    /// [069A-1 F5] Puerto del navegador interno. `None` → la tool no se
    /// registra (fail-closed). Solo el escritorio inyecta un valor real.
    pub navegador: Option<Arc<dyn NavegadorPort>>,
}

impl Default for OpcionesSesion {
    fn default() -> Self {
        Self {
            provider: None,
            modelo: None,
            dir: None,
            modo: None,
            razonamiento: None,
            nueva_conversacion: false,
            navegador: None,
        }
    }
}

impl std::fmt::Debug for OpcionesSesion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpcionesSesion")
            .field("provider", &self.provider)
            .field("modelo", &self.modelo)
            .field("dir", &self.dir)
            .field("modo", &self.modo)
            .field("razonamiento", &self.razonamiento)
            .field("nueva_conversacion", &self.nueva_conversacion)
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

/// Estado común que debe compartir cualquier transporte.
#[derive(Clone)]
pub struct SesionComun {
    pub runtime: Arc<AgentRuntime>,
    pub persistencia: Arc<PersistenciaSqlite>,
    pub user_id: Uuid,
    pub modelo: String,
    pub modo: String,
    pub workspace: String,
    /// [069A-1 F5] Puerto del navegador interno conservado entre
    /// reconstrucciones (cambio de modelo/workspace). `None` → la tool
    /// `navegador_reflejo` no se registra (fail-closed). Solo el escritorio
    /// inyecta un valor real; se conserva aquí para que reconfigurar/cambiar
    /// workspace no pierdan la tool al reconstruir el runtime.
    pub navegador: Option<Arc<dyn NavegadorPort>>,
}

/// Datos preparados para que el consumidor ejecute y transporte un turno.
/// El servicio no crea tareas ni canales: así Tauri, HTTP/SSE y CLI conservan
/// sus propios ciclos de vida y cancelación sin duplicar la política durable.
pub struct PreparacionTurno {
    pub turno_id: Uuid,
    pub conversacion_id: Uuid,
    pub historial: Vec<AiMessage>,
    pub mensaje_efectivo: String,
    pub runtime: Arc<AgentRuntime>,
}

impl SesionComun {
    /// Abre SQLite durable, resuelve configuración y construye el runtime.
    pub fn abrir(opciones: OpcionesSesion) -> Result<(Self, Apertura), Error> {
        cargar_env_usuario();
        let (persistencia, aviso) = match PersistenciaSqlite::ruta_bd_app() {
            Some(ruta) => match PersistenciaSqlite::abrir(&ruta) {
                Ok(p) => (p, None),
                Err(e) => (
                    PersistenciaSqlite::en_memoria()
                        .map_err(|e| Error::Persistencia(e.to_string()))?,
                    Some(format!("BD no disponible ({}): sesión en memoria", e)),
                ),
            },
            None => (
                PersistenciaSqlite::en_memoria().map_err(|e| Error::Persistencia(e.to_string()))?,
                Some("sin ruta de datos: sesión en memoria".to_string()),
            ),
        };
        Self::abrir_con_persistencia(opciones, persistencia, aviso)
    }

    /// Variante inyectable para tests y consumidores que ya abrieron la BD.
    pub fn abrir_con_persistencia(
        opciones: OpcionesSesion,
        persistencia: PersistenciaSqlite,
        aviso: Option<String>,
    ) -> Result<(Self, Apertura), Error> {
        let opciones_run = resolver_opciones(&persistencia, &opciones)?;
        let user_id = usuario_estable(&persistencia)?;
        persistencia.con_skills_base(user_id);

        let conv_id = match if !opciones.nueva_conversacion {
            persistencia
                .conversaciones_listar(user_id)
                .map_err(|e| Error::Persistencia(e.to_string()))?
                .into_iter()
                .find(|c| !c.archivada)
                .map(|c| (c.id, false))
        } else {
            None
        } {
            Some(par) => par,
            None => {
                /* [069A-7] Auto-creación del servicio: se conserva
                 * (invariante de sesión) pero se REPORTa en `conv_autocreada`
                 * para que los transportes create-on-write (web/desktop)
                 * puedan descartar la fila vacía y arrancar en borrador sin
                 * fila persistida. */
                let id = persistencia
                    .conversacion_crear(user_id, "Nueva conversación")
                    .map_err(|e| Error::Persistencia(e.to_string()))?;
                (id, true)
            }
        };
        let (conv_id, conv_autocreada) = conv_id;

        let persistencia = Arc::new(persistencia);
        let harness = construir_harness_con(
            &opciones_run,
            Arc::clone(&persistencia) as Arc<dyn AgentPersistence>,
            Arc::clone(&persistencia) as Arc<dyn ProgramadorTareas>,
            user_id,
        );
        let existe_conversacion = persistencia
            .conversaciones_listar(user_id)
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .into_iter()
            .any(|c| c.id == conv_id);
        if !existe_conversacion {
            return Err(Error::Sesion("conversación inicial no encontrada".into()));
        }

        if persistencia
            .config_leer("workspace")
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .is_none()
        {
            if let Some(ws) = harness.workspace.as_ref() {
                persistencia
                    .config_guardar("workspace", &ws.to_string_lossy())
                    .map_err(|e| Error::Persistencia(e.to_string()))?;
            }
        }

        let workspace = harness
            .workspace
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<desconocido>".into());
        let sesion = Self {
            runtime: harness.runtime,
            persistencia,
            user_id,
            modelo: format!("{}/{}", harness.config.provider, harness.config.modelo),
            modo: harness.config.modo,
            workspace,
            /* [069A-1 F5] El puerto de navegador vive en la sesión: se
             * conserva para que `reconfigurar`/`cambiar_workspace` lo
             * reinyecten al reconstruir el runtime. */
            navegador: opciones_run.navegador.clone(),
        };
        /* [069A-7] `info()` reporta `conv_autocreada:false`; la apertura real
         * propaga si el servicio auto-creó la fila vacía (ver struct). */
        let mut apertura = sesion.info(conv_id, aviso)?;
        apertura.conv_autocreada = conv_autocreada;
        Ok((sesion, apertura))
    }

    pub fn info(&self, conversacion_id: Uuid, aviso: Option<String>) -> Result<Apertura, Error> {
        let conversacion = self
            .persistencia
            .conversaciones_listar(self.user_id)
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .into_iter()
            .find(|c| c.id == conversacion_id)
            .ok_or_else(|| Error::Sesion("conversación actual no encontrada".into()))?;
        Ok(Apertura {
            modelo: self.modelo.clone(),
            workspace: self.workspace.clone(),
            proveedores: conteos(&LlavesProveedor::from_env()),
            conversacion,
            /* [069A-7] `info()` es un reporte de una conversación EXISTENTE
             * (p. ej. tras reconfigurar): nunca es una auto-creación. */
            conv_autocreada: false,
            aviso,
        })
    }

    pub fn reconfigurar(
        &mut self,
        provider: Option<String>,
        modelo: Option<String>,
        modo: Option<String>,
        razonamiento: Option<String>,
    ) -> Result<(), Error> {
        cargar_env_usuario();
        let cfg = &self.runtime.turno_config;
        let opciones = OpcionesRun {
            provider: provider.or_else(|| Some(cfg.provider.clone())),
            modelo: modelo.or_else(|| Some(cfg.modelo.clone())),
            dir: Some(PathBuf::from(&self.workspace)),
            modo: modo.or_else(|| Some(cfg.modo.clone())),
            max_ventana: leer_max_ventana(&self.persistencia)?,
            razonamiento: razonamiento.or_else(|| cfg.nivel_razonamiento.clone()),
            notificar: false,
            /* [069A-1 F5] El puerto de navegador se conserva en la sesión y
             * se reinyecta al reconstruir: cambiar de modelo NO debe perder
             * la tool `navegador_reflejo` (el hardware/COM vive en la sesión
             * actual y se preserva entre reconstrucciones). */
            navegador: self.navegador.clone(),
        };
        self.reconstruir(opciones)
    }

    /// [069A-2 F3] Cambia el workspace a una ruta absoluta validada y
    /// reconstruye el runtime sobre ella, conservando proveedor, modelo,
    /// modo, razonamiento y puerto de navegador vigentes. El llamador debe
    /// garantizar que no hay turno activo (el runtime en curso conserva el
    /// workspace viejo). Nota: las sesiones web nunca tienen puerto de
    /// navegador (`OpcionesSesion::default`), así que no hay nada que
    /// preservar ahí.
    pub fn cambiar_workspace(&mut self, ruta: PathBuf) -> Result<(), Error> {
        if !ruta.is_absolute() {
            return Err(Error::Configuracion("la ruta debe ser absoluta".into()));
        }
        if !ruta.is_dir() {
            return Err(Error::Configuracion(
                "la ruta no existe o no es un directorio".into(),
            ));
        }
        cargar_env_usuario();
        self.persistencia
            .config_guardar("workspace", &ruta.to_string_lossy())
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let cfg = &self.runtime.turno_config;
        let opciones = OpcionesRun {
            provider: Some(cfg.provider.clone()),
            modelo: Some(cfg.modelo.clone()),
            dir: Some(ruta),
            modo: Some(cfg.modo.clone()),
            max_ventana: leer_max_ventana(&self.persistencia)?,
            razonamiento: cfg.nivel_razonamiento.clone(),
            notificar: false,
            /* [069A-1 F5] Se conserva el puerto de navegador de la sesión:
             * cambiar el workspace no debe perder la tool. */
            navegador: self.navegador.clone(),
        };
        self.reconstruir(opciones)
    }

    /// Reconstruye el runtime con las opciones dadas y actualiza los
    /// campos derivados (modelo/modo/workspace visibles).
    fn reconstruir(&mut self, opciones: OpcionesRun) -> Result<(), Error> {
        let harness = construir_harness_con(
            &opciones,
            Arc::clone(&self.persistencia) as Arc<dyn AgentPersistence>,
            Arc::clone(&self.persistencia) as Arc<dyn ProgramadorTareas>,
            self.user_id,
        );
        self.runtime = harness.runtime;
        self.modelo = format!("{}/{}", harness.config.provider, harness.config.modelo);
        self.modo = harness.config.modo;
        self.workspace = harness
            .workspace
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_else(|| "<desconocido>".into());
        Ok(())
    }

    /// Persiste el mensaje y devuelve los datos necesarios para ejecutar el turno.
    pub async fn preparar_turno(
        &self,
        conversacion_id: Uuid,
        mensaje: String,
        meta: Option<String>,
    ) -> Result<PreparacionTurno, Error> {
        if mensaje.trim().is_empty() {
            return Err(Error::Turno("mensaje vacío".into()));
        }
        let mensajes_previos = self
            .persistencia
            .listar_mensajes(conversacion_id)
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let habia_historial = !mensajes_previos.is_empty();
        let historial = historial_desde_persistencia(mensajes_previos);

        /* [039A-1 04-09 H5] Auto-nombre tras el primer mensaje: solo cuando
         * el título sigue siendo el default "Nueva conversación" y no había
         * historial previo (evita pisar renombres manuales). */
        if !habia_historial {
            let es_default = self
                .persistencia
                .conversaciones_listar(self.user_id)
                .map_err(|e| Error::Persistencia(e.to_string()))?
                .into_iter()
                .find(|c| c.id == conversacion_id)
                .map(|c| c.titulo == "Nueva conversación")
                .unwrap_or(false);
            if es_default {
                let nuevo = titulo_auto_desde_mensaje(&mensaje);
                self.persistencia
                    .conversacion_renombrar(conversacion_id, self.user_id, &nuevo)
                    .map_err(|e| Error::Persistencia(e.to_string()))?;
            }
        }
        self.persistencia
            .guardar_mensaje(&MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id,
                rol: "user".into(),
                contenido: mensaje.clone(),
                creado_en: Utc::now(),
            })
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        self.persistencia
            .conversacion_tocar(conversacion_id)
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;

        let mensaje_efectivo = match (self.modo.as_str(), meta) {
            ("meta", Some(m)) if !m.trim().is_empty() => {
                format!("[META: {}]\n{}", m.trim(), mensaje)
            }
            _ => mensaje.clone(),
        };
        Ok(PreparacionTurno {
            turno_id: Uuid::new_v4(),
            conversacion_id,
            historial,
            mensaje_efectivo,
            runtime: Arc::clone(&self.runtime),
        })
    }

    pub async fn cancelar_turno(&self, turno_id: Uuid) -> Result<(), Error> {
        self.persistencia
            .finalizar_turno(turno_id, "cancelado", Some("abortado por el usuario"))
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))
    }
}

fn usuario_estable(persistencia: &PersistenciaSqlite) -> Result<Uuid, Error> {
    match persistencia
        .config_leer("user_id")
        .map_err(|e| Error::Persistencia(e.to_string()))?
    {
        Some(guardado) => Uuid::parse_str(guardado.trim())
            .map_err(|_| Error::Configuracion("user_id guardado corrupto".into())),
        None => {
            let nuevo = Uuid::new_v4();
            persistencia
                .config_guardar("user_id", &nuevo.to_string())
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(nuevo)
        }
    }
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

fn leer_max_ventana(persistencia: &PersistenciaSqlite) -> Result<Option<u32>, Error> {
    const DEFAULT: u32 = 150_000;
    match persistencia
        .config_leer("contexto_max_ventana")
        .map_err(|e| Error::Persistencia(e.to_string()))?
    {
        Some(txt) => match txt.trim().parse::<u32>() {
            Ok(v) if v >= VENTANA_MINIMA => Ok(Some(v)),
            _ => Ok(Some(DEFAULT)),
        },
        None => Ok(Some(DEFAULT)),
    }
}

fn resolver_opciones(
    persistencia: &PersistenciaSqlite,
    opciones: &OpcionesSesion,
) -> Result<OpcionesRun, Error> {
    let leer = |clave| {
        persistencia
            .config_leer(clave)
            .map_err(|e| Error::Persistencia(e.to_string()))
    };
    let dir = match opciones.dir.clone() {
        Some(dir) => Some(dir),
        None => leer("workspace")?.filter(|d| !d.trim().is_empty()),
    };
    let provider = match opciones.provider.clone() {
        Some(p) => Some(p),
        None => leer("proveedor")?,
    };
    let modelo = match opciones.modelo.clone() {
        Some(m) => Some(m),
        None => leer("modelo")?,
    };
    let modo = match opciones.modo.clone() {
        Some(m) => Some(m),
        None => leer("modo")?,
    };
    let razonamiento = match opciones.razonamiento.clone() {
        Some(valor) => Some(valor),
        None => leer("nivelRazonamiento")?,
    };
    Ok(OpcionesRun {
        provider,
        modelo,
        dir: dir.map(PathBuf::from),
        modo,
        razonamiento,
        max_ventana: leer_max_ventana(persistencia)?,
        notificar: false,
        navegador: opciones.navegador.clone(),
    })
}

/// [039A-1 04-09 H5] Nombre breve de conversación desde el primer mensaje del
/// usuario: primeras ~4 palabras (o ~42 caracteres), una sola línea, sin
/// prefijos de modo (`[META: …]`). Si no hay palabras, "Conversación".
fn titulo_auto_desde_mensaje(mensaje: &str) -> String {
    let limpio = mensaje.trim().lines().next().unwrap_or("").trim();
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn meta_solo_se_aplica_en_modo_meta() {
        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (sesion_autonoma, apertura_autonoma) = SesionComun::abrir_con_persistencia(
            OpcionesSesion {
                modo: Some("autonomo".into()),
                ..OpcionesSesion::default()
            },
            persistencia,
            None,
        )
        .expect("abrir sesión autónoma");
        let turno_autonomo = sesion_autonoma
            .preparar_turno(
                apertura_autonoma.conversacion.id,
                "mensaje de prueba".into(),
                Some("no debe aplicarse".into()),
            )
            .await
            .expect("preparar turno autónomo");
        assert_eq!(turno_autonomo.mensaje_efectivo, "mensaje de prueba");

        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (sesion_meta, apertura_meta) = SesionComun::abrir_con_persistencia(
            OpcionesSesion {
                modo: Some("meta".into()),
                ..OpcionesSesion::default()
            },
            persistencia,
            None,
        )
        .expect("abrir sesión meta");
        let turno_meta = sesion_meta
            .preparar_turno(
                apertura_meta.conversacion.id,
                "mensaje de prueba".into(),
                Some("sí debe aplicarse".into()),
            )
            .await
            .expect("preparar turno meta");
        assert_eq!(
            turno_meta.mensaje_efectivo,
            "[META: sí debe aplicarse]\nmensaje de prueba"
        );
    }

    /// [079A-1 post-F7] Ventana del desktop (default 150k, sin tocar el
    /// default del core 128k): valor persistido o default ante
    /// basura/bajo-piso; antes duplicada en el desktop (`pruebas.rs`,
    /// eliminada: 0 llamadas prod, la vía real es `resolver_opciones`).
    #[test]
    fn ventana_sin_config_usa_default_desktop() {
        let p = PersistenciaSqlite::en_memoria().expect("bd en memoria");
        assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
    }

    #[test]
    fn ventana_respeta_valor_persistido() {
        let p = PersistenciaSqlite::en_memoria().expect("bd en memoria");
        p.config_guardar("contexto_max_ventana", "200000")
            .expect("guarda");
        assert_eq!(leer_max_ventana(&p).expect("lee"), Some(200_000));
    }

    #[test]
    fn ventana_basura_o_bajo_piso_cae_al_default() {
        let p = PersistenciaSqlite::en_memoria().expect("bd en memoria");
        p.config_guardar("contexto_max_ventana", "no-numero")
            .expect("guarda");
        assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
        p.config_guardar("contexto_max_ventana", "5")
            .expect("guarda");
        assert_eq!(leer_max_ventana(&p).expect("lee"), Some(150_000));
    }
}
