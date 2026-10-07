//! Flujo de salida del ejecutor (parte de `super`): construcción sin shell,
//! bomba de pipes por línea, anillo y truncado a 8 KB.
//!
//! Sin cambio de comportamiento: los bloques `impl` vivían en `ejecutor.rs`
//! (partición 309A-3; el archivo superaba el límite + god-object).

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::process::Command;

use glory_harness_core::aplicar_entorno_minimo;
use glory_harness_core::error::{Error, Result};
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::ChunkConsola;

use super::super::jaula::construir_directo;
use super::{
    ConsolaViva, EjecutorCliente, LIMITE_LINEA_STREAM, LIMITE_RING_BYTES, LIMITE_SALIDA,
    LIMITE_STREAM_BYTES,
};

impl EjecutorCliente {
    /// [139A-8 F1/K1] Construcción SIN shell vía `jaula::construir_directo`.
    /// La denegación de la jaula se traduce a `Error::Sandbox` (el sandbox
    /// bloqueó el comando) con el mensaje claro de la jaula.
    /// [139A-8 F3n/K2] Punto único de spawn del modelo: el hijo NO hereda el
    /// entorno del operador (claves LLM) — solo el subconjunto mínimo
    /// (`aplicar_entorno_minimo`). Cubre `ejecutar_sincrono` y
    /// `ejecutar_fondo` (ambos pasan por aquí).
    pub(super) fn construir_comando(&self, comando: &str) -> Result<Command> {
        let mut construido = construir_directo(comando, self.raiz.as_ref())
            .map_err(|motivo| Error::Sandbox(format!("jaula: {motivo}")))?;
        aplicar_entorno_minimo(&mut construido);
        Ok(construido)
    }

    /// [209A-1 F1] Bomba de un pipe: reenvía cada línea a `chunks` (hasta el
    /// tope compartido `presupuesto`) y devuelve los bytes CRUDOS para el
    /// resultado final. Conversión `lossy` como `truncar` (la salida OEM de
    /// Windows no es UTF-8 y `read_line` la cortaría en seco). Si el receptor
    /// se fue (turno cerrado) solo acumula. `None` = modo sin streaming.
    /// [209A-1 F2] `anillo`: la viva a cuyo ring se empuja cada línea
    /// (recortada igual que el stream); `None` en la rama síncrona (su
    /// resultado final ya es el transcript).
    pub(super) async fn bombear<T>(
        tubo: Option<T>,
        flujo: FlujoConsola,
        chunks: Option<tokio::sync::mpsc::UnboundedSender<ChunkConsola>>,
        presupuesto: Arc<AtomicUsize>,
        anillo: Option<Arc<ConsolaViva>>,
    ) -> Vec<u8>
    where
        T: AsyncRead + Unpin + Send + 'static,
    {
        let Some(tubo) = tubo else {
            return Vec::new();
        };
        let mut lector = BufReader::new(tubo);
        let mut crudo = Vec::new();
        let mut segmento = Vec::new();
        loop {
            segmento.clear();
            match lector.read_until(b'\n', &mut segmento).await {
                Ok(0) => break,
                Ok(_) => {
                    crudo.extend_from_slice(&segmento);
                    let texto = String::from_utf8_lossy(&segmento);
                    let linea = texto.trim_end_matches(['\r', '\n']);
                    let recorte: String =
                        linea.chars().take(LIMITE_LINEA_STREAM).collect();
                    let recortada = recorte.len() < linea.len();
                    let mut linea = recorte;
                    if recortada {
                        linea.push_str("…(línea recortada)");
                    }
                    // [219A-4] Sin `chunks` (consola propia: ningún turno la
                    // emite) no hay a quién enviar, pero el anillo SÍ se
                    // empuja: `salida` y la UI lo leen de ahí.
                    if let Some(tx) = &chunks {
                        // Turno cerrado o consola desacoplada: no se envía, pero
                        // la viva sigue visible en `comando_lista` vía su anillo.
                        let suelta = anillo
                            .as_ref()
                            .is_some_and(|v| v.suelta.load(Ordering::Relaxed));
                        if !tx.is_closed() && !suelta {
                            let reservado =
                                presupuesto.fetch_add(linea.len(), Ordering::Relaxed);
                            if reservado < LIMITE_STREAM_BYTES {
                                let _ = tx.send(ChunkConsola { flujo, linea: linea.clone() });
                            }
                        }
                    }
                    if let Some(v) = &anillo {
                        Self::empujar_anillo(v, flujo, &linea).await;
                    }
                }
                Err(_) => break,
            }
        }
        crudo
    }

    /// [209A-1 F2] Empuja una línea al anillo, descartando las más antiguas
    /// al superar `LIMITE_RING_BYTES` (contador para el fin).
    pub(super) async fn empujar_anillo(viva: &Arc<ConsolaViva>, flujo: FlujoConsola, linea: &str) {
        let mut anillo = viva.anillo.lock().await;
        let mut bytes = viva.bytes_anillo.load(Ordering::Relaxed);
        bytes += linea.len();
        anillo.push_back(ChunkConsola {
            flujo,
            linea: linea.to_string(),
        });
        while bytes > LIMITE_RING_BYTES {
            if let Some(vieja) = anillo.pop_front() {
                bytes = bytes.saturating_sub(vieja.linea.len());
                viva.bytes_descartados
                    .fetch_add(vieja.linea.len(), Ordering::Relaxed);
            } else {
                break;
            }
        }
        viva.bytes_anillo.store(bytes, Ordering::Relaxed);
    }

    /// [139A-8 F1] Truncado a 8 KB en BYTES (no en chars): la salida OEM
    /// (`dir`, `ping`…) llega con tildes/ñ que `from_utf8_lossy` convierte
    /// en `�` (3 bytes); contar chars dejaba pasar hasta 3× el límite y
    /// rompía el contrato de 8 KB. El corte respeta borde de char.
    pub(super) fn truncar(salida: &[u8]) -> (String, bool) {
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
