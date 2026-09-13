//! Ejecutor real de comandos del CLI (318A-16 F3).
//!
//! Implementa el puerto `EjecutorComando` del núcleo con tokio `Command`:
//! ejecución DIRECTA sin shell (jaula `infra::jaula`, 139A-8 F1/K1), timeout
//! acotado, truncado de salida a 8 KB y tareas de fondo identificadas por id
//! (`comando_status`/`comando_matar`). El núcleo queda agnóstico: solo ve este
//! trait; un consumidor sin runner (p. ej. PROYECTO TASKS, que deniega
//! comandos) no registra la tool en absoluto (fail-closed).
//!
//! [119A-7 F0] Jaula: el ejecutor puede fijar el directorio de arranque de
//! cada hijo (`en_raiz`). Los comandos heredan ese cwd, así que las rutas
//! relativas del modelo caen dentro del workspace del run. Límite honesto:
//! el cwd no contiene `..` absolutos; la contención total la dan la jaula
//! (sin shell + allowlist/denylist) + clasificación de riesgo
//! (`bash_clasificar`) + aprobación + supervisión. `nuevo()` (sin raíz,
//! hereda el cwd del proceso) queda solo para diagnósticos sin run.
//!
//! [139A-8 F1/K1] Sin shell: `ejecutar`/`ejecutar_fondo` construyen con
//! `jaula::construir_directo` (argv directo, builtins `cmd` con veto en
//! Windows). Tuberías/redirecciones/`&&`/`$()` se DENIEGAN con mensaje
//! claro (cambio de conducta documentado en `jaula.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use glory_harness_core::aplicar_entorno_minimo;
use glory_harness_core::error::{Error, Result};
use glory_harness_core::ports::{EjecutorComando, ResultadoEjecucionComando};

use super::jaula::construir_directo;

/// Límite de salida capturada por comando (8 KB, contrato del plan 318A-16).
const LIMITE_SALIDA: usize = 8 * 1024;
/// Timeout por comando síncrono: 120 s (un comando colgado no bloquea el turno).
const TIMEOUT_COMANDO: Duration = Duration::from_secs(120);

/// Handle compartido de una tarea de fondo: el spawner y `matar` compiten por
/// el `Child`; quien lo toma (o mata) lo deja en `None`.
type HandleTarea = Arc<Mutex<Option<Child>>>;

/// Implementación concreta del puerto para el CLI.
pub struct EjecutorCliente {
    tareas: Arc<Mutex<HashMap<String, HandleTarea>>>,
    resultados: Arc<Mutex<HashMap<String, ResultadoEjecucionComando>>>,
    /// [119A-7 F0] Raíz enjaulada: cwd de arranque de cada hijo.
    /// `None` = heredar el cwd del proceso (solo diagnósticos sin run).
    raiz: Option<PathBuf>,
}

impl EjecutorCliente {
    #[must_use]
    pub fn nuevo() -> Self {
        Self {
            tareas: Arc::default(),
            resultados: Arc::default(),
            raiz: None,
        }
    }

    /// Ejecutor enjaulado: cada comando arranca con cwd = `raiz`.
    #[must_use]
    pub fn en_raiz(raiz: PathBuf) -> Self {
        Self {
            tareas: Arc::default(),
            resultados: Arc::default(),
            raiz: Some(raiz),
        }
    }

    /// [139A-8 F1/K1] Construcción SIN shell vía `jaula::construir_directo`.
    /// La denegación de la jaula se traduce a `Error::Sandbox` (el sandbox
    /// bloqueó el comando) con el mensaje claro de la jaula.
    /// [139A-8 F3n/K2] Punto único de spawn del modelo: el hijo NO hereda el
    /// entorno del operador (claves LLM) — solo el subconjunto mínimo
    /// (`aplicar_entorno_minimo`). Cubre `ejecutar_sincrono` y
    /// `ejecutar_fondo` (ambos pasan por aquí).
    fn construir_comando(&self, comando: &str) -> Result<Command> {
        let mut construido = construir_directo(comando, self.raiz.as_ref())
            .map_err(|motivo| Error::Sandbox(format!("jaula: {motivo}")))?;
        aplicar_entorno_minimo(&mut construido);
        Ok(construido)
    }

    /// [139A-8 F1] Truncado a 8 KB en BYTES (no en chars): la salida OEM
    /// (`dir`, `ping`…) llega con tildes/ñ que `from_utf8_lossy` convierte
    /// en `�` (3 bytes); contar chars dejaba pasar hasta 3× el límite y
    /// rompía el contrato de 8 KB. El corte respeta borde de char.
    fn truncar(salida: &[u8]) -> (String, bool) {
        let texto = String::from_utf8_lossy(salida);
        if texto.len() <= LIMITE_SALIDA {
            (texto.into_owned(), false)
        } else {
            let mut fin = LIMITE_SALIDA;
            while !texto.is_char_boundary(fin) {
                fin -= 1;
            }
            let mut cortado: String = texto[..fin].to_string();
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
            self.construir_comando(comando)?
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
        let mut child = self
            .construir_comando(comando)?
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        // Tomamos los pipes antes de `wait()` (que solo presta `child`, así el
        // timeout puede matarlo después sin mover el valor).
        let mut stdout = child.stdout.take();
        let mut stderr = child.stderr.take();
        // [139A-8 F1] Drenaje CONCURRENTE de los pipes: si el hijo escribe
        // más que el buffer del pipe (~64 KB) y nadie lee, se bloquea y el
        // `wait()` muere por timeout aunque el comando sea instantáneo
        // (`dir System32` = 299 KB lo demostró). Las lectoras son dueñas de
        // los pipes; el `wait()` solo presta `child`.
        let drenar_out = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(o) = stdout.as_mut() {
                let _ = o.read_to_end(&mut buf).await;
            }
            buf
        });
        let drenar_err = tokio::spawn(async move {
            let mut buf = Vec::new();
            if let Some(e) = stderr.as_mut() {
                let _ = e.read_to_end(&mut buf).await;
            }
            buf
        });

        let estado = tokio::time::timeout(TIMEOUT_COMANDO, child.wait()).await;
        if estado.is_err() {
            // Timeout: matar PRIMERO para que los pipes lleguen a EOF y las
            // lectoras terminen; solo después se reúnen. (Al revés se cuelga:
            // el hijo vivo retiene la escritura y el `await` no vuelve.)
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        // Tras el `wait` (o el kill por timeout) los pipes llegan a EOF: las
        // lectoras siempre terminan y se pueden reunir sin timeout extra.
        let mut capturado = drenar_out.await.unwrap_or_default();
        capturado.extend_from_slice(&drenar_err.await.unwrap_or_default());
        match estado {
            Ok(Ok(status)) => {
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
                // Timeout: devolvemos lo capturado hasta el kill.
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
            // [139A-8 F1/K1] Sin `>nul`: la redirección la deniega la jaula
            // (la salida capturada la archiva el ejecutor igualmente).
            format!("ping -n {} 127.0.0.1", segundos + 1)
        } else {
            format!("sleep {segundos}")
        }
    }

    fn comando_mucho_eco() -> String {
        if cfg!(windows) {
            // [139A-8 F1/K1] El `for /L … do @echo` es sintaxis cmd y la
            // jaula lo deniega: `dir` de System32 (~5000 entradas, >200 KB)
            // fuerza el truncado en segundos sin shell ni redirección.
            r"dir C:\Windows\System32".to_string()
        } else {
            // Sin tubería (la jaula la deniega): 90 KB de NULes bastan para
            // forzar el truncado a 8 KB.
            "head -c 90000 /dev/zero".to_string()
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
    async fn matar_termina_la_tarea_de_fondo() {        let e = EjecutorCliente::nuevo();
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

    /// [119A-7 F0] Humo de la jaula: el hijo arranca con cwd = la raíz
    /// enjaulada (`en_raiz`), no con el cwd del proceso de test.
    #[tokio::test]
    async fn en_raiz_arranca_los_comandos_en_la_jaula() {
        let jaula = std::env::temp_dir().join(format!(
            "gh-jaula-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("reloj")
                .as_nanos()
        ));
        std::fs::create_dir_all(&jaula).expect("crear jaula");
        let canonica = jaula.canonicalize().expect("canonizar jaula");
        let e = EjecutorCliente::en_raiz(jaula.clone());
        // `cd` (Windows) / `pwd` (unix) reportan el cwd del hijo.
        let sonda = if cfg!(windows) { "cd" } else { "pwd" };
        let r = e.ejecutar(sonda, false).await.expect("sonda cwd");
        assert_eq!(r.codigo_salida, Some(0));
        // Windows: `canonicalize` devuelve ruta verbatim (`\\?\C:\...`)
        // mientras `cd` imprime `C:\...`; además el FS no distingue
        // mayúsculas. Se normaliza por ambos lados antes de comparar.
        let normalizar = |s: &str| {
            s.strip_prefix(r"\\?\")
                .unwrap_or(s)
                .replace('/', "\\")
                .to_lowercase()
        };
        let salida = normalizar(r.salida.trim());
        let esperada = normalizar(&canonica.to_string_lossy());
        assert!(
            salida.contains(&esperada),
            "el hijo arranca en la jaula '{esperada}', salida: {}",
            r.salida.trim()
        );
        std::fs::remove_dir_all(&jaula).ok();
    }
}
