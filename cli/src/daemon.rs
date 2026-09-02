//! Subcomando `glory-harness daemon` (Fase 3): proceso de fondo que atiende
//! turnos de agentes en loopback, multi-sesión.
//!
//! Transporte: **TCP en `127.0.0.1` con NDJSON** (una línea JSON por mensaje),
//! el contrato de eventos es el mismo `AgenteEvento` de H3 que el daemon de
//! task emitiría por SSE. El plan §6.7 permite "SSE/JSON en loopback"; se
//! elige NDJSON sobre TCP para no arrastrar un servidor HTTP pesado al
//! binario standalone y para mantener un protocolo acotado y testeable.
//!
//! Autorización (R2 del plan): solo escucha en loopback y exige un **token de
//! sesión**. Sin token válido no se abren sesiones ni se ejecutan turnos.
//!
//! Protocolo (líneas NDJSON, el cliente envía y recibe):
//! - req `{"tipo":"sesion_abrir","token":"..."}` → res `{"tipo":"sesion_abierta","session_id":"..."}`
//! - req `{"tipo":"turno","session_id":"...","mensaje":"..."}` → stream de
//!   líneas `AgenteEvento` serializado + `{"tipo":"done","turno_id":"..."}`
//! - req `{"tipo":"sesion_cerrar","session_id":"..."}` → res `{"tipo":"sesion_cerrada"}`
//!
//! Multi-sesión: el gestor mantiene un runtime por `session_id` (lock por
//! sesión dentro del proceso), de modo que varios consumidores pueden
//! compartir el mismo daemon sin pisarse (requisito §6.7).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::ErrorKind;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;
use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::llm::{LlmProviderService, LlavesProveedor};
use glory_harness_core::runtime::{AgentRuntime, PuertosHarness, TurnoConfig};
use glory_harness_core::tool::AgentToolRegistry;
use glory_harness_core::AgentPersistence;

use crate::persistencia::PersistenciaMemoria;

/// Peticiones NDJSON del cliente al daemon.
#[derive(Debug, Deserialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum Peticion {
    SesionAbrir { token: String },
    Turno { session_id: String, mensaje: String },
    SesionCerrar { session_id: String },
}

/// Respuestas NDJSON del daemon (las de turno usan [`AgenteEvento`] + done).
#[derive(Debug, Serialize)]
#[serde(tag = "tipo", rename_all = "snake_case")]
enum Respuesta {
    SesionAbierta { session_id: String },
    SesionCerrada { session_id: String },
    TurnoDone { turno_id: Uuid },
    Error { mensaje: String },
}

/// Una sesión activa del daemon: cada una tiene su propio runtime (persistencia
/// en memoria + proveedor LLM común) y su lock (las sesiones no se cruzan).
struct Sesion {
    runtime: Arc<AgentRuntime>,
    user_id: Uuid,
    conversacion_id: Uuid,
    _lock: Mutex<()>,
}

impl Sesion {
    fn nueva(runtime: AgentRuntime) -> Self {
        Self {
            runtime: Arc::new(runtime),
            user_id: Uuid::new_v4(),
            conversacion_id: Uuid::new_v4(),
            _lock: Mutex::new(()),
        }
    }
}

/// El daemon: token de autorización + mapa de sesiones.
#[derive(Clone)]
struct Daemon {
    token: Arc<String>,
    sesiones: Arc<Mutex<HashMap<String, Arc<Sesion>>>>,
}

impl Daemon {
    fn nuevo(token: String) -> Self {
        Self {
            token: Arc::new(token),
            sesiones: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Asegura un token: usa el de la env `GLORY_HARNESS_DAEMON_TOKEN` o genera
    /// uno aleatorio (se muestra en el arranque).
    fn token_desde_env() -> String {
        match std::env::var("GLORY_HARNESS_DAEMON_TOKEN") {
            Ok(t) if !t.trim().is_empty() => t,
            _ => Uuid::new_v4().to_string(),
        }
    }

    fn autorizado(&self, token: &str) -> bool {
        token == self.token.as_str()
    }

    /// Abre una sesión (tras validar token) y devuelve su id serializable.
    async fn abrir_sesion(&self) -> Uuid {
        let persistencia = Arc::new(PersistenciaMemoria::nuevo());
        let user_id = Uuid::new_v4();
        persistencia.con_skills_base(user_id);

        let llm = Arc::new(LlmProviderService::new(LlavesProveedor::from_env()));
        let persistencia_port: Arc<dyn AgentPersistence> = persistencia.clone();
        let puertos = PuertosHarness {
            persistencia: persistencia_port,
            llm,
            web_search: None,
            dominio: None,
        };
        let runtime = AgentRuntime::nuevo(AgentToolRegistry::new(), puertos, TurnoConfig::default());
        let sesion = Arc::new(Sesion::nueva(runtime));

        let session_id = Uuid::new_v4();
        let mut sesiones = self.sesiones.lock().await;
        sesiones.insert(session_id.to_string(), sesion);
        session_id
    }

    async fn cerrar_sesion(&self, session_id: &str) -> bool {
        let mut sesiones = self.sesiones.lock().await;
        sesiones.remove(session_id).is_some()
    }

    /// Ejecuta un turno de la sesión, emitiendo cada `AgenteEvento` por `w`.
    async fn ejecutar_turno(
        &self,
        session_id: &str,
        mensaje: String,
        w: &mut WriteHalf<TcpStream>,
    ) -> Result<(), String> {
        let sesion = {
            let sesiones = self.sesiones.lock().await;
            sesiones.get(session_id).cloned().ok_or_else(|| {
                "sesión no encontrada (abre sesion_abrir primero)".to_string()
            })?
        };
        let _guard = sesion._lock.lock().await;

        let turno_id = Uuid::new_v4();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<AgenteEvento>(64);
        let runtime = Arc::clone(&sesion.runtime);
        let user_id = sesion.user_id;
        let conversacion_id = sesion.conversacion_id;
        let handle = tokio::spawn(async move {
            runtime
                .ejecutar_turno(user_id, turno_id, conversacion_id, Vec::new(), mensaje, &tx)
                .await
        });

        while let Some(evento) = rx.recv().await {
            let linea = serde_json::to_string(&evento).map_err(|e| e.to_string())?;
            escribir_linea(w, &linea).await?;
        }
        let resultado = handle.await;
        match resultado {
            Ok(Ok(())) => {
                let done = Respuesta::TurnoDone { turno_id };
                let linea = serde_json::to_string(&done).map_err(|e| e.to_string())?;
                escribir_linea(w, &linea).await
            }
            Ok(Err(err)) => {
                // No fallo silencioso: si el runtime devolvió error (proveedor/
                // red) sin evento Error previo, el cliente recibe un error en
                // vez de un turno_done engañoso.
                let error = Respuesta::Error {
                    mensaje: err.to_string(),
                };
                let linea = serde_json::to_string(&error).map_err(|e| e.to_string())?;
                escribir_linea(w, &linea).await
            }
            Err(err) => {
                let error = Respuesta::Error {
                    mensaje: format!("el turno abortó con pánico: {err}"),
                };
                let linea = serde_json::to_string(&error).map_err(|e| e.to_string())?;
                escribir_linea(w, &linea).await
            }
        }
    }
}

/// Escribe una línea NDJSON (con salto de línea y flush) a la mitad de escritura.
async fn escribir_linea(w: &mut WriteHalf<TcpStream>, linea: &str) -> Result<(), String> {
    w.write_all(linea.as_bytes()).await.map_err(|e| e.to_string())?;
    w.write_all(b"\n").await.map_err(|e| e.to_string())?;
    w.flush().await.map_err(|e| e.to_string())?;
    Ok(())
}

/// Atiende una conexión TCP del daemon, línea a línea (NDJSON).
async fn atender(daemon: Daemon, stream: TcpStream) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);
    let mut linea = String::new();
    loop {
        linea.clear();
        match reader.read_line(&mut linea).await {
            Ok(0) => break, // cliente cerró
            Ok(_) => {
                let texto = linea.trim();
                if texto.is_empty() {
                    continue;
                }
                let Ok(peticion) = serde_json::from_str::<Peticion>(texto) else {
                    let r = Respuesta::Error {
                        mensaje: "petición inválida (JSON NDJSON esperado)".into(),
                    };
                    let _ = escribir_linea(
                        &mut writer,
                        &serde_json::to_string(&r).unwrap_or_default(),
                    )
                    .await;
                    continue;
                };
                respond(daemon.clone(), peticion, &mut writer).await;
            }
            Err(e) if e.kind() == ErrorKind::ConnectionAborted => break,
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => break,
            Err(e) => {
                tracing::warn!(%e, "error leyendo conexión del daemon");
                break;
            }
        }
    }
}

/// Atiende una petición NDJSON y escribe la/s respuesta/s correspondiente/s.
async fn respond(daemon: Daemon, peticion: Peticion, writer: &mut WriteHalf<TcpStream>) {
    let respuesta: Option<Respuesta> = match peticion {
        Peticion::SesionAbrir { token } => {
            if !daemon.autorizado(&token) {
                Some(Respuesta::Error {
                    mensaje: "token no autorizado".into(),
                })
            } else {
                let id = daemon.abrir_sesion().await;
                Some(Respuesta::SesionAbierta {
                    session_id: id.to_string(),
                })
            }
        }
        Peticion::Turno { session_id, mensaje } => {
            let res = daemon.ejecutar_turno(&session_id, mensaje, writer).await;
            if let Err(err) = res {
                Some(Respuesta::Error { mensaje: err })
            } else {
                None // ejecutar_turno ya escribió el stream + done
            }
        }
        Peticion::SesionCerrar { session_id } => {
            daemon.cerrar_sesion(&session_id).await;
            Some(Respuesta::SesionCerrada { session_id })
        }
    };
    if let Some(r) = respuesta {
        let _ = escribir_linea(writer, &serde_json::to_string(&r).unwrap_or_default()).await;
    }
}

/// Arranca el daemon en `127.0.0.1:<puerto>`. Devuelve `ExitCode::SUCCESS`
/// solo cuando el proceso se detiene limpiamente (Ctrl+C).
pub async fn run(puerto: u16, mostrar_token: bool) -> std::process::ExitCode {
    let token = Daemon::token_desde_env();
    if mostrar_token {
        eprintln!("[glory-harness] daemon token: {token}");
    }
    let listener = match TcpListener::bind(("127.0.0.1", puerto)).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[glory-harness] no se pudo escuchar en 127.0.0.1:{puerto}: {e}");
            return std::process::ExitCode::from(1);
        }
    };
    eprintln!("[glory-harness] daemon escuchando en 127.0.0.1:{puerto} (NDJSON, token obligatorio)");

    let daemon = Daemon::nuevo(token);
    loop {
        tokio::select! {
            resultado = listener.accept() => {
                let (stream, _addr) = match resultado {
                    Ok(par) => par,
                    Err(e) => {
                        tracing::warn!(%e, "error aceptando conexión del daemon");
                        continue;
                    }
                };
                let daemon = daemon.clone();
                tokio::spawn(async move { atender(daemon, stream).await });
            }
            _ = tokio::signal::ctrl_c() => {
                eprintln!("\n[glory-harness] daemon detenido");
                return std::process::ExitCode::SUCCESS;
            }
        }
    }
}