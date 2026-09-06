//! Ejecutor real de comandos del CLI (318A-16 F3).
//!
//! Implementa el puerto `EjecutorComando` del núcleo con tokio `Command`:
//! shell del sistema (`cmd /C` en Windows, `sh -c` en el resto), timeout
//! acotado, truncado de salida a 8 KB y tareas de fondo identificadas por id
//! (`comando_status`/`comando_matar`). El núcleo queda agnóstico: solo ve este
//! trait; un consumidor sin runner (p. ej. PROYECTO TASKS, que deniega
//! comandos) no registra la tool en absoluto (fail-closed).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use glory_harness_core::error::Result;
use glory_harness_core::ports::{EjecutorComando, ResultadoEjecucionComando};

/// Límite de salida capturada por comando (8 KB, contrato del plan 318A-16).
const LIMITE_SALIDA: usize = 8 * 1024;
/// Timeout por comando síncrono: 120 s (un comando colgado no bloquea el turno).
const TIMEOUT_COMANDO: Duration = Duration::from_secs(120);

/// Handle compartido de una tarea de fondo: el spawner y `matar` compiten por
/// el `Child`; quien lo toma (o mata) lo deja en `None`.
type HandleTarea = Arc<Mutex<Option<Child>>>;

/// Implementación concreta del puerto para el CLI.
#[derive(Default)]
pub struct EjecutorCliente {
    tareas: Arc<Mutex<HashMap<String, HandleTarea>>>,
    resultados: Arc<Mutex<HashMap<String, ResultadoEjecucionComando>>>,
}

impl EjecutorCliente {
    #[must_use]
    pub fn nuevo() -> Self {
        Self::default()
    }

    fn construir_comando(comando: &str) -> Command {
        if cfg!(windows) {
            let mut c = Command::new("cmd");
            c.arg("/C").arg(comando);
            c
        } else {
            let mut c = Command::new("sh");
            c.arg("-c").arg(comando);
            c
        }
    }

    fn truncar(salida: &[u8]) -> (String, bool) {
        let texto = String::from_utf8_lossy(salida).into_owned();
        if texto.len() <= LIMITE_SALIDA {
            (texto, false)
        } else {
            let mut cortado: String = texto.chars().take(LIMITE_SALIDA).collect();
            cortado.push_str("\n…(salida truncada por límite del harness)");
            (cortado, true)
        }
    }
}

impl EjecutorCliente {
    /// Rama de fondo: lanza el comando detached y archiva su resultado en una
    /// tarea tokio; devuelve el id para `comando_status`/`comando_matar`.
    async fn ejecutar_fondo(&self, comando: &str) -> Result<ResultadoEjecucionComando> {
        let id = uuid::Uuid::new_v4().to_string();
        let handle: HandleTarea = Arc::new(Mutex::new(Some(
            Self::construir_comando(comando)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()?,
        )));
        self.tareas.lock().await.insert(id.clone(), handle.clone());
        let tareas = self.tareas.clone();
        let resultados = self.resultados.clone();
        let id_detach = id.clone();
        tokio::spawn(async move {
            // Tomamos el hijo (None si `matar` se adelantó y ya lo mató).
            let hijo = {
                let mut h = handle.lock().await;
                h.take()
            };
            let resultado = match hijo {
                None => ResultadoEjecucionComando {
                    codigo_salida: None,
                    salida: "(tarea de fondo terminada por comando_matar)".to_string(),
                    truncada: false,
                    fondo: true,
                    id_fondo: Some(id_detach.clone()),
                },
                Some(hijo) => match hijo.wait_with_output().await {
                    Ok(salida) => {
                        let mut bytes = salida.stdout;
                        bytes.extend_from_slice(&salida.stderr);
                        let (texto, truncada) = Self::truncar(&bytes);
                        ResultadoEjecucionComando {
                            codigo_salida: salida.status.code(),
                            salida: texto,
                            truncada,
                            fondo: true,
                            id_fondo: Some(id_detach.clone()),
                        }
                    }
                    Err(_) => ResultadoEjecucionComando {
                        codigo_salida: None,
                        salida: "(error capturando salida del proceso)".to_string(),
                        truncada: false,
                        fondo: true,
                        id_fondo: Some(id_detach.clone()),
                    },
                },
            };
            tareas.lock().await.remove(&id_detach);
            resultados.lock().await.insert(id_detach.clone(), resultado);
        });
        Ok(ResultadoEjecucionComando {
            codigo_salida: None,
            salida: "(comando lanzado en segundo plano; usa comando_status para consultar)"
                .to_string(),
            truncada: false,
            fondo: true,
            id_fondo: Some(id),
        })
    }

    /// Rama síncrona: corre con timeout y devuelve la salida truncada a 8 KB.
    async fn ejecutar_sincrono(&self, comando: &str) -> Result<ResultadoEjecucionComando> {
        let mut child = Self::construir_comando(comando)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        // Tomamos los pipes antes de `wait()` (que solo presta `child`, así el
        // timeout puede matarlo después sin mover el valor).
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        let mut capturado = Vec::new();

        let estado = tokio::time::timeout(TIMEOUT_COMANDO, child.wait()).await;
        match estado {
            Ok(Ok(status)) => {
                if let Some(o) = stdout.as_mut() {
                    let _ = o.read_to_end(&mut capturado).await;
                }
                if let Some(e) = stderr.as_mut() {
                    let _ = e.read_to_end(&mut capturado).await;
                }
                let (texto, truncada) = Self::truncar(&capturado);
                Ok(ResultadoEjecucionComando {
                    codigo_salida: status.code(),
                    salida: texto,
                    truncada,
                    fondo: false,
                    id_fondo: None,
                })
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                // Timeout: matamos el proceso y devolvemos lo capturado hasta ahora.
                let _ = child.kill().await;
                let _ = child.wait().await;
                if let Some(o) = stdout.as_mut() {
                    let _ = o.read_to_end(&mut capturado).await;
                }
                if let Some(e) = stderr.as_mut() {
                    let _ = e.read_to_end(&mut capturado).await;
                }
                let mut texto = format!(
                    "⏱ el comando excedió el límite de {} s y fue terminado\n",
                    TIMEOUT_COMANDO.as_secs()
                );
                texto.push_str(&String::from_utf8_lossy(&capturado));
                Ok(ResultadoEjecucionComando {
                    codigo_salida: None,
                    salida: texto,
                    truncada: true,
                    fondo: false,
                    id_fondo: None,
                })
            }
        }
    }
}

#[async_trait]
impl EjecutorComando for EjecutorCliente {
    async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando> {
        if fondo {
            self.ejecutar_fondo(comando).await
        } else {
            self.ejecutar_sincrono(comando).await
        }
    }

    async fn estado(&self, id_fondo: &str) -> Result<ResultadoEjecucionComando> {
        if let Some(r) = self.resultados.lock().await.get(id_fondo) {
            return Ok(r.clone());
        }
        if self.tareas.lock().await.contains_key(id_fondo) {
            return Ok(ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(aún en ejecución)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id_fondo.to_string()),
            });
        }
        Ok(ResultadoEjecucionComando {
            codigo_salida: None,
            salida: format!("(tarea de fondo desconocida: {id_fondo})"),
            truncada: false,
            fondo: true,
            id_fondo: Some(id_fondo.to_string()),
        })
    }

    async fn matar(&self, id_fondo: &str) -> Result<()> {
        let tareas = self.tareas.lock().await;
        if let Some(handle) = tareas.get(id_fondo) {
            let mut h = handle.lock().await;
            if let Some(hijo) = h.as_mut() {
                let _ = hijo.kill().await;
                let _ = hijo.wait().await;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn comando_lento(segundos: u64) -> String {
        if cfg!(windows) {
            // `ping -n N` en Windows espera ~N-1 segundos y sale con código 0.
            format!("ping -n {} 127.0.0.1 >nul", segundos + 1)
        } else {
            format!("sleep {segundos}")
        }
    }

    fn comando_mucho_eco() -> String {
        if cfg!(windows) {
            // ~10 KB de salida (900 líneas × 10 chars + CRLF) → fuerza el truncado.
            "for /L %i in (1,1,900) do @echo xxxxxxxxxx".to_string()
        } else {
            "head -c 9000 /dev/zero | tr '\\0' 'x'".to_string()
        }
    }

    #[tokio::test]
    async fn sincrono_devuelve_salida_y_codigo() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar("echo hola-harness", false).await.unwrap();
        assert!(!r.fondo);
        assert_eq!(r.codigo_salida, Some(0));
        assert!(r.salida.contains("hola-harness"), "salida: {}", r.salida);
        assert!(!r.truncada);
    }

    #[tokio::test]
    async fn salida_larga_se_trunca_a_8kb() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_mucho_eco(), false).await.unwrap();
        assert_eq!(r.codigo_salida, Some(0));
        assert!(
            r.truncada,
            "salida inesperadamente corta: {} bytes",
            r.salida.len()
        );
        assert!(
            r.salida.len() <= LIMITE_SALIDA + 64,
            "longitud: {}",
            r.salida.len()
        );
        assert!(r.salida.contains("truncada"));
    }

    #[tokio::test]
    async fn fondo_devuelve_id_y_status_espera_resultado() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(3), true).await.unwrap();
        assert!(r.fondo);
        let id = r.id_fondo.expect("fondo debe devolver id");

        // Poll hasta que la tarea termine (máx. 15 s) y quede archivada.
        let mut resultado = None;
        for _ in 0..30 {
            let s = e.estado(&id).await.unwrap();
            if s.codigo_salida.is_some() {
                resultado = Some(s);
                break;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let resultado = resultado.expect("la tarea de fondo debió terminar");
        assert_eq!(resultado.codigo_salida, Some(0));
        assert_eq!(resultado.id_fondo.as_deref(), Some(id.as_str()));
    }

    #[tokio::test]
    async fn matar_termina_la_tarea_de_fondo() {
        let e = EjecutorCliente::nuevo();
        let r = e.ejecutar(&comando_lento(60), true).await.unwrap();
        let id = r.id_fondo.expect("fondo debe devolver id");
        // Estado inmediato: debe seguir en ejecución (el comando dura ~60 s).
        let s = e.estado(&id).await.unwrap();
        assert!(
            s.codigo_salida.is_none(),
            "aún corriendo, salida: {}",
            s.salida
        );
        e.matar(&id).await.unwrap();
        // Tras matar, la tarea deja de estar "en ejecución" en ≤ 5 s.
        for _ in 0..10 {
            let s = e.estado(&id).await.unwrap();
            if !s.salida.contains("aún en ejecución") {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        panic!("la tarea siguió reportando ejecución tras matar");
    }
}
