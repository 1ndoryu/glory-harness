//! Servicio de sesión independiente del transporte.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::{DateTime, SecondsFormat, Utc};
use glory_harness_core::llm::{AiMessage, LlavesProveedor};
use glory_harness_core::ports::{MensajePersistido, NavegadorPort};
use glory_harness_core::runtime::{AgentRuntime, CompactarManual};
use glory_harness_core::{AgentPersistence, ProgramadorTareas};
use serde::Serialize;
use uuid::Uuid;

use crate::{
    cargar_env_usuario, construir_harness_con, historial_desde_persistencia, InfoConversacion,
    OpcionesRun, PersistenciaSqlite,
};

use super::meta::{resolver_meta, ComandoMeta, ErrorMeta, EstadoMeta, ResultadoMeta};
use super::sesion_config::{leer_gancho_pre_compact, leer_max_ventana, resolver_opciones};

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
#[derive(Clone, Default)]
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
            gancho_pre_compact: leer_gancho_pre_compact(&self.persistencia)
                .map_err(Error::Persistencia)?,
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
            gancho_pre_compact: leer_gancho_pre_compact(&self.persistencia)
                .map_err(Error::Persistencia)?,
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

    /// Lee el estado de meta durable de una conversación propia ([109A-5 F1]).
    ///
    /// Distingue "sin meta" (`EstadoMeta::default`) de "conversación
    /// inexistente", que es error: un id borrado o ajeno no debe degradar a un
    /// estado vacío que parezca legítimo.
    pub fn meta_leer(&self, conversacion_id: Uuid) -> Result<EstadoMeta, ErrorMeta> {
        let crudo = self
            .persistencia
            .conversacion_meta_leer(self.user_id, conversacion_id)
            .map_err(|e| ErrorMeta::Persistencia(e.to_string()))?;
        match crudo {
            Some(valor) => EstadoMeta::desde_persistida(valor),
            None => Err(ErrorMeta::ConversacionInexistente),
        }
    }

    /// Aplica un comando de ciclo de vida y persiste el resultado.
    ///
    /// `&mut self` es intencional: obliga al llamador a retener el mutex de la
    /// sesión durante la lectura y la escritura, así dos transiciones del mismo
    /// proceso no pueden intercalarse y perder una actualización. Si la
    /// escritura falla, el estado en disco queda como estaba (una sola
    /// sentencia) y el error se propaga sin mutar la copia en memoria.
    pub fn meta_aplicar(
        &mut self,
        conversacion_id: Uuid,
        comando: ComandoMeta,
    ) -> Result<ResultadoMeta, ErrorMeta> {
        let estado = self.meta_leer(conversacion_id)?;
        let resultado = resolver_meta(comando, &estado, Utc::now())?;
        let fila = resultado.estado.a_persistida()?;
        let guardado = self
            .persistencia
            .conversacion_meta_guardar(self.user_id, conversacion_id, &fila)
            .map_err(|e| ErrorMeta::Persistencia(e.to_string()))?;
        if !guardado {
            return Err(ErrorMeta::ConversacionInexistente);
        }
        Ok(resultado)
    }

    /// Persiste el mensaje y devuelve los datos necesarios para ejecutar el turno.
    ///
    /// [109A-5 F1] La meta es de la CONVERSACIÓN, no de la sesión: se lee de
    /// disco y solo si esa conversación no tiene meta vigente se usa
    /// `meta_borrador` (que existe porque el panel global puede fijar una meta
    /// antes de que la fila se cree en el primer mensaje).
    pub async fn preparar_turno(
        &self,
        conversacion_id: Uuid,
        mensaje: String,
        meta_borrador: Option<String>,
    ) -> Result<PreparacionTurno, Error> {
        if mensaje.trim().is_empty() {
            return Err(Error::Turno("mensaje vacío".into()));
        }
        /* Se lee ANTES de escribir nada: si el estado durable estuviera
         * corrupto o la conversación no existiera, el turno falla sin haber
         * persistido el mensaje ni renombrado la conversación. */
        let meta_efectiva = match self.meta_leer(conversacion_id) {
            Ok(estado) => estado.activa.map(|activa| activa.texto).or(meta_borrador),
            Err(e) => return Err(Error::Turno(format!("meta de la conversación: {e}"))),
        };
        let mensajes_previos = self
            .persistencia
            .listar_mensajes(conversacion_id)
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let habia_historial = !mensajes_previos.is_empty();
        let historial = match self.punto_de_compactacion(conversacion_id)? {
            /* [109A-4 F3] Compactación manual previa: el modelo arranca del
             * resumen persistido y solo ve los mensajes posteriores a la marca
             * (verbatim). Nada se borra en disco: el historial visible, el
             * rewind y la auditoría siguen completos. */
            Some((cuando, resumen)) => {
                /* `>=` y no `>`: `PersistenciaSqlite` guarda las fechas de
                 * mensaje con precisión de SEGUNDOS (`SecondsFormat::Secs`) y el
                 * punto usa la misma, así que un mensaje del mismo segundo que
                 * la compactación debe entrar (duplicar algo ya resumido es
                 * inofensivo; perder un turno no lo es). */
                let posteriores: Vec<MensajePersistido> = mensajes_previos
                    .into_iter()
                    .filter(|m| m.creado_en >= cuando)
                    .collect();
                let mut contexto = vec![AiMessage::texto("system", resumen)];
                contexto.extend(historial_desde_persistencia(posteriores));
                contexto
            }
            None => historial_desde_persistencia(mensajes_previos),
        };

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

        let mensaje_efectivo = match (self.modo.as_str(), meta_efectiva.as_deref()) {
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

    /// [109A-4 F3] Compactación pedida por el usuario (`/compactar`).
    ///
    /// El historial se reconstruye desde la persistencia (igual que un turno) y
    /// se compacta con el runtime de la sesión: mismo gestor de contexto y
    /// mismos ganchos `PreCompact`/`PostCompact` que la pasada automática. Si
    /// compacta, el resumen queda PERSISTIDO como punto de compactación de la
    /// conversación y los turnos siguientes arrancan de él; los mensajes no se
    /// borran (historial visible y rewind intactos). Si no hay material, no se
    /// escribe nada y el resultado lo explica con `motivo`.
    pub async fn compactar_conversacion(
        &self,
        conversacion_id: Uuid,
        instruccion: Option<String>,
    ) -> Result<CompactarManual, Error> {
        let mensajes = self
            .persistencia
            .listar_mensajes(conversacion_id)
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let historial = historial_desde_persistencia(mensajes);
        let instruccion = instruccion
            .map(|texto| texto.trim().to_owned())
            .filter(|texto| !texto.is_empty());
        let resultado = self
            .runtime
            .compactar_manual(&historial, instruccion.as_deref())
            .await;
        if let Some(resumen) = resultado.resumen.as_deref() {
            let guardado = self
                .persistencia
                .conversacion_compactar(
                    self.user_id,
                    conversacion_id,
                    /* Misma precisión que `PersistenciaSqlite` usa para los
                     * mensajes: comparar nano segundos contra segundos haría
                     * perder los mensajes del mismo segundo. */
                    &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                    resumen,
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            if !guardado {
                return Err(Error::Sesion(
                    "la conversación no existe o no es del usuario".into(),
                ));
            }
        }
        Ok(resultado)
    }

    /// [109A-4 F3] Punto de compactación vigente, resuelto a (instante,
    /// resumen). Una marca de tiempo ilegible se ignora con aviso en vez de
    /// romper el turno: enviar el historial completo es la degradación segura.
    fn punto_de_compactacion(
        &self,
        conversacion_id: Uuid,
    ) -> Result<Option<(DateTime<Utc>, String)>, Error> {
        let punto = self
            .persistencia
            .conversacion_compactacion(self.user_id, conversacion_id)
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let Some(punto) = punto else {
            return Ok(None);
        };
        match DateTime::parse_from_rfc3339(&punto.compactado_en) {
            Ok(cuando) => Ok(Some((cuando.with_timezone(&Utc), punto.resumen))),
            Err(e) => {
                tracing::warn!(error = %e, "compactación con fecha inválida; se envía el historial completo");
                Ok(None)
            }
        }
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

    /// [109A-5 F1] La meta es de la conversación: la de A no se filtra a B y
    /// un id inexistente es error, no un estado vacío silencioso.
    #[tokio::test]
    async fn meta_durable_es_por_conversacion() {
        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (mut sesion, apertura) = SesionComun::abrir_con_persistencia(
            OpcionesSesion {
                modo: Some("meta".into()),
                ..OpcionesSesion::default()
            },
            persistencia,
            None,
        )
        .expect("abrir sesión meta");
        let conv_a = apertura.conversacion.id;
        let conv_b = sesion
            .persistencia
            .conversacion_crear(sesion.user_id, "otra")
            .expect("crear segunda conversación");
        sesion
            .meta_aplicar(
                conv_a,
                ComandoMeta::Fijar {
                    texto: "meta A".into(),
                },
            )
            .expect("fija A");

        let turno_a = sesion
            .preparar_turno(conv_a, "hola A".into(), None)
            .await
            .expect("turno A");
        assert_eq!(turno_a.mensaje_efectivo, "[META: meta A]\nhola A");
        let turno_b = sesion
            .preparar_turno(conv_b, "hola B".into(), None)
            .await
            .expect("turno B");
        assert_eq!(turno_b.mensaje_efectivo, "hola B");
        assert!(sesion.meta_leer(conv_b).expect("lee B").activa.is_none());
        assert_eq!(
            sesion.meta_leer(Uuid::new_v4()).unwrap_err(),
            ErrorMeta::ConversacionInexistente
        );
    }

    /// [109A-5 F1] Ciclo completo durable: fallos cerrados sin mutar, logro con
    /// turno anclado e historial que sobrevive a reabrir la sesión.
    #[tokio::test]
    async fn meta_durable_registra_logro_y_sobrevive() {
        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (mut sesion, apertura) = SesionComun::abrir_con_persistencia(
            OpcionesSesion::default(),
            persistencia,
            None,
        )
        .expect("abrir sesión");
        let conv = apertura.conversacion.id;
        assert_eq!(
            sesion.meta_aplicar(conv, ComandoMeta::Pausar).unwrap_err(),
            ErrorMeta::SinMetaActiva
        );
        sesion
            .meta_aplicar(
                conv,
                ComandoMeta::Fijar {
                    texto: "meta durable".into(),
                },
            )
            .expect("fija");
        sesion.meta_aplicar(conv, ComandoMeta::Pausar).expect("pausa");
        assert_eq!(
            sesion.meta_aplicar(conv, ComandoMeta::Pausar).unwrap_err(),
            ErrorMeta::YaPausada
        );
        let turno = Uuid::new_v4();
        let resultado = sesion
            .meta_aplicar(conv, ComandoMeta::Lograr { turno_id: turno })
            .expect("logra");
        assert_eq!(resultado.logro.as_ref().expect("logro").turno_id, turno);
        let estado = sesion.meta_leer(conv).expect("lee meta");
        assert!(estado.activa.is_none());
        assert_eq!(estado.logros.len(), 1);

        let compartida = (*sesion.persistencia).clone();
        let (sesion2, _) = SesionComun::abrir_con_persistencia(
            OpcionesSesion::default(),
            compartida,
            None,
        )
        .expect("reabrir sesión");
        assert_eq!(
            sesion2.meta_leer(conv).expect("lee tras reabrir").logros.len(),
            1
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

    /// [109A-4 F3] Siembra un historial que supera la cola verbatim mínima
    /// (10 000 tokens: aquí ~21 000) con instrucciones reconocibles, para que
    /// la compactación algorítmica tenga tramo real que resumir sin proveedor.
    async fn sembrar_historial_largo(sesion: &SesionComun, conv: Uuid, pares: usize) {
        let base = Utc::now() - chrono::Duration::hours(1);
        for i in 0..pares {
            let peticion = format!(
                "Revisa el archivo src/modulo_{i}.rs y corrige el aviso del gate; mantén el estilo del proyecto y no toques otros módulos. {}",
                "detalle ".repeat(60)
            );
            let respuesta = format!(
                "Ajusté src/modulo_{i}.rs y pasé las pruebas del bloque {i}. {}",
                "nota ".repeat(60)
            );
            for (rol, contenido) in [("user", peticion), ("assistant", respuesta)] {
                sesion
                    .persistencia
                    .guardar_mensaje(&MensajePersistido {
                        id: Uuid::new_v4(),
                        conversacion_id: conv,
                        rol: rol.into(),
                        contenido,
                        creado_en: base + chrono::Duration::seconds(i as i64),
                    })
                    .await
                    .expect("sembrar mensaje");
            }
        }
    }

    /// [109A-4 F3] Compactación por demanda: el resumen queda persistido como
    /// punto de la conversación y el turno siguiente arranca de él en vez de
    /// arrastrar el historial entero. Los mensajes NO se borran: el historial
    /// visible y el rewind siguen completos, y lo posterior a la marca viaja
    /// verbatim.
    #[tokio::test]
    async fn compactar_conversacion_persiste_el_punto_y_el_turno_arranca_del_resumen() {
        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (sesion, apertura) = SesionComun::abrir_con_persistencia(
            OpcionesSesion::default(),
            persistencia,
            None,
        )
        .expect("abrir sesión");
        let conv = apertura.conversacion.id;
        sembrar_historial_largo(&sesion, conv, 60).await;

        let resultado = sesion
            .compactar_conversacion(conv, Some("prioriza los pendientes".into()))
            .await
            .expect("compactar");
        assert!(resultado.compactado, "motivo: {:?}", resultado.motivo);
        assert!(resultado.tokens_despues < resultado.tokens_antes);

        let resumen = sesion
            .persistencia
            .conversacion_compactacion(sesion.user_id, conv)
            .expect("leer punto")
            .expect("punto persistido")
            .resumen;
        assert!(!resumen.trim().is_empty(), "el resumen no puede quedar vacío");

        let turno = sesion
            .preparar_turno(conv, "sigue con el siguiente archivo".into(), None)
            .await
            .expect("preparar turno");
        assert_eq!(turno.historial.len(), 1, "solo el resumen debe viajar");
        assert_eq!(turno.historial[0].role, "system");
        assert_eq!(turno.historial[0].content.as_str().expect("texto"), resumen);

        let siguiente = sesion
            .preparar_turno(conv, "y ahora el otro".into(), None)
            .await
            .expect("preparar turno 2");
        assert_eq!(siguiente.historial.len(), 2);
        assert_eq!(siguiente.historial[1].role, "user");
        assert!(siguiente.historial[1]
            .content
            .as_str()
            .expect("texto")
            .contains("sigue con el siguiente archivo"));
    }

    /// [109A-4 F3] Sin material no se compacta ni se persiste nada: repetir
    /// `/compactar` dos veces seguidas con el mismo historial no debe fingir
    /// trabajo ni dejar un punto huérfano.
    #[tokio::test]
    async fn compactar_conversacion_sin_material_no_persiste() {
        let persistencia = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (sesion, apertura) =
            SesionComun::abrir_con_persistencia(OpcionesSesion::default(), persistencia, None)
                .expect("abrir sesión");
        let conv = apertura.conversacion.id;
        sesion
            .persistencia
            .guardar_mensaje(&MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: "user".into(),
                contenido: "hola".into(),
                creado_en: Utc::now(),
            })
            .await
            .expect("sembrar mensaje");

        let resultado = sesion
            .compactar_conversacion(conv, None)
            .await
            .expect("compactar");
        assert!(!resultado.compactado);
        assert_eq!(
            resultado.motivo.as_deref(),
            Some("no hay material nuevo que resumir")
        );
        assert!(resultado.resumen.is_none());
        assert!(sesion
            .persistencia
            .conversacion_compactacion(sesion.user_id, conv)
            .expect("leer punto")
            .is_none());
    }
}
