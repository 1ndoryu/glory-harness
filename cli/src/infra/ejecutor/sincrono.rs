//! Rama síncrona del ejecutor (parte de `super`): corre con timeout y
//! devuelve la salida truncada a 8 KB.
//!
//! Sin cambio de comportamiento: el bloque `impl` vivía en `ejecutor.rs`
//! (partición 309A-3; el archivo superaba el límite + god-object).

use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use tokio::sync::mpsc::UnboundedSender;

use glory_harness_core::error::Result;
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::{ChunkConsola, ResultadoEjecucionComando};

use super::{EjecutorCliente, TIMEOUT_COMANDO};

impl EjecutorCliente {
    /// Rama síncrona: corre con timeout y devuelve la salida truncada a 8 KB.
    /// [209A-1 F1] Las lectoras ahora son bombas por línea (`bombear`): sin
    /// streaming (`chunks=None`) se comportan como el drenaje anterior.
    pub(super) async fn ejecutar_sincrono_en_vivo(
        &self,
        id: &str,
        comando: &str,
        chunks: Option<UnboundedSender<ChunkConsola>>,
    ) -> Result<ResultadoEjecucionComando> {
        let mut child = self
            .construir_comando(comando)?
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // [209A-1 F4] Cancelación del turno a mitad de `wait()`: soltar
            // el `Child` mata al hijo en vez de dejarlo huérfano.
            .kill_on_drop(true)
            .spawn()?;
        // Tomamos los pipes antes de `wait()` (que solo presta `child`, así el
        // timeout puede matarlo después sin mover el valor).
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        // [139A-8 F1] Drenaje CONCURRENTE de los pipes: si el hijo escribe
        // más que el buffer del pipe (~64 KB) y nadie lee, se bloquea y el
        // `wait()` muere por timeout aunque el comando sea instantáneo
        // (`dir System32` = 299 KB lo demostró). Las bombas son dueñas de
        // los pipes; el `wait()` solo presta `child`.
        let presupuesto = Arc::new(AtomicUsize::new(0));
        let drenar_out = tokio::spawn(Self::bombear(
            stdout,
            FlujoConsola::Stdout,
            chunks.clone(),
            Arc::clone(&presupuesto),
            None,
        ));
        let drenar_err = tokio::spawn(Self::bombear(
            stderr,
            FlujoConsola::Stderr,
            chunks,
            presupuesto,
            None,
        ));

        let estado = tokio::time::timeout(TIMEOUT_COMANDO, child.wait()).await;
        if estado.is_err() {
            // Timeout: matar PRIMERO para que los pipes lleguen a EOF y las
            // bombas terminen; solo después se reúnen. (Al revés se cuelga:
            // el hijo vivo retiene la escritura y el `await` no vuelve.)
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        // Tras el `wait` (o el kill por timeout) los pipes llegan a EOF: las
        // bombas siempre terminan y se pueden reunir sin timeout extra.
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
                    comando: comando.to_string(),
                    id_ejecucion: id.to_string(),
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
                    comando: comando.to_string(),
                    id_ejecucion: id.to_string(),
                })
            }
        }
    }
}
