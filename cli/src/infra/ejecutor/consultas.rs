//! Consultas y control del ejecutor (parte de `super`): implementación del
//! puerto `EjecutorComando` (lista/estado/salida/escribir/matar/desacoplar)
//! más el reap por conversación y global.
//!
//! Sin cambio de comportamiento: los bloques `impl` vivían en `ejecutor.rs`
//! (partición 309A-3; el archivo superaba el límite + god-object).

use std::sync::Arc;

use async_trait::async_trait;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc::UnboundedSender;
use uuid::Uuid;

use glory_harness_core::error::{Error, Result};
use glory_harness_core::evento::FlujoConsola;
use glory_harness_core::ports::{
    ChunkConsola, EjecutorComando, InfoConsola, OrigenConsola, ResultadoEjecucionComando,
    TranscriptConsola,
};

use super::{ConsolaViva, EjecutorCliente, MAX_ESCRITURA_STDIN, MAX_LINEAS_TRANSCRIPT};

impl EjecutorCliente {
    /// Mata el hijo de una viva SIN tocar `vivas`: el pump es el único que
    /// retira y archiva (idempotente ante la carrera matar↔salida natural).
    /// [209A-1 F4] Cubre las dos ventanas: handle aún en `tareas` (kill
    /// directo) o ya tomado por el pump (señal `matar`: el pump lo mata él
    /// mismo). Devuelve `true` si había algo que matar.
    async fn matar_handle(&self, id: &str) -> bool {
        {
            let tareas = self.tareas.lock().await;
            if let Some(handle) = tareas.get(id) {
                let mut h = handle.lock().await;
                if let Some(hijo) = h.as_mut() {
                    let _ = hijo.kill().await;
                    let _ = hijo.wait().await;
                    return true;
                }
            }
        }
        if let Some(viva) = self.registro.lock().await.vivas.get(id) {
            viva.matar.notify_one();
            return true;
        }
        false
    }

    /// [209A-1 F4] Reap por conversación: mata las vivas de `conv` (las de
    /// otras conversaciones siguen). El pump de cada una retira su viva y
    /// archiva el transcript acotado. Devuelve cuántas mató. Sin vivas
    /// coincide: devuelve 0, sin error.
    pub async fn matar_por_conversacion(&self, conv: Uuid) -> usize {
        let ids: Vec<String> = {
            self.registro
                .lock()
                .await
                .vivas
                .iter()
                .filter(|(_, v)| v.conversacion_id == conv)
                .map(|(id, _)| id.clone())
                .collect()
        };
        let mut matadas = 0;
        for id in ids {
            if self.matar_handle(&id).await {
                matadas += 1;
            }
        }
        matadas
    }

    /// [209A-1 F4] Reap global (cierre de app): mata TODAS las vivas, de
    /// cualquier conversación. Cada pump retira y archiva; devuelve el
    /// conteo. Lo usa el cierre ordenado web (`apagado_ordenado` en
    /// `comandos/web/mod.rs`: Ctrl+C mata antes de soltar el listener).
    pub async fn matar_todas(&self) -> usize {
        let ids: Vec<String> = {
            self.registro.lock().await.vivas.keys().cloned().collect()
        };
        let mut matadas = 0;
        for id in ids {
            if self.matar_handle(&id).await {
                matadas += 1;
            }
        }
        matadas
    }
}

#[async_trait]
impl EjecutorComando for EjecutorCliente {
    async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando> {
        // Sin streaming: el id solo rellena los campos nuevos del resultado.
        // Sin conversación conocida: `nil` (diagnósticos sin run).
        let id = uuid::Uuid::new_v4().to_string();
        if fondo {
            self.ejecutar_fondo_con_id(&id, comando, Uuid::nil(), None, OrigenConsola::Agente)
                .await
        } else {
            self.ejecutar_sincrono_en_vivo(&id, comando, None).await
        }
    }

    async fn ejecutar_en_vivo(
        &self,
        id: &str,
        comando: &str,
        conversacion_id: Uuid,
        fondo: bool,
        chunks: UnboundedSender<ChunkConsola>,
    ) -> Result<ResultadoEjecucionComando> {
        if fondo {
            self.ejecutar_fondo_con_id(
                id,
                comando,
                conversacion_id,
                Some(chunks),
                OrigenConsola::Agente,
            )
            .await
        } else {
            self.ejecutar_sincrono_en_vivo(id, comando, Some(chunks))
                .await
        }
    }

    async fn desacoplar(&self, id: &str) -> Result<()> {
        let reg = self.registro.lock().await;
        let viva = reg
            .vivas
            .get(id)
            .ok_or_else(|| Error::NoEncontrado(format!("consola desconocida: {id}")))?;
        viva.suelta.store(true, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }

    /// [219A-4] Consola propia del operador (ver `ejecutar_fondo_propio`).
    async fn ejecutar_propia(&self, comando: Option<&str>) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        self.ejecutar_fondo_propio(&id, comando).await?;
        Ok(id)
    }

    async fn lista(&self) -> Result<Vec<InfoConsola>> {
        // [219A-5 F2] Foto bajo UN solo lock: vivas (ordenadas por inicio) +
        // archivadas (en `orden` = fin real). Sin `Instant::now()` ni
        // ventana entre mapas.
        // [219A-5 F3] La archivada trae su dueño retenido.
        let reg = self.registro.lock().await;
        let mut vivas: Vec<(&String, &Arc<ConsolaViva>)> = reg.vivas.iter().collect();
        vivas.sort_by_key(|(_, v)| v.inicio);
        let mut infos = Vec::with_capacity(reg.vivas.len() + reg.archivadas.len());
        for (id, viva) in vivas {
            infos.push(InfoConsola {
                id_ejecucion: id.clone(),
                comando: viva.comando.clone(),
                conversacion_id: viva.conversacion_id,
                viva: true,
                codigo_salida: None,
                bytes: viva.bytes_anillo.load(std::sync::atomic::Ordering::Relaxed),
                origen: viva.origen,
            });
        }
        for id in reg.orden.iter() {
            if let Some(a) = reg.archivadas.get(id) {
                infos.push(InfoConsola {
                    id_ejecucion: id.clone(),
                    comando: a.resultado.comando.clone(),
                    conversacion_id: Uuid::nil(),
                    viva: false,
                    codigo_salida: a.resultado.codigo_salida,
                    bytes: a.resultado.salida.len(),
                    origen: a.origen,
                });
            }
        }
        Ok(infos)
    }

    async fn estado(&self, id_fondo: &str) -> Result<ResultadoEjecucionComando> {
        let reg = self.registro.lock().await;
        if let Some(a) = reg.archivadas.get(id_fondo) {
            return Ok(a.resultado.clone());
        }
        // Viva: informar el comando real (F2; antes `String::new()`).
        if let Some(viva) = reg.vivas.get(id_fondo) {
            return Ok(ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(aún en ejecución)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id_fondo.to_string()),
                comando: viva.comando.clone(),
                id_ejecucion: id_fondo.to_string(),
            });
        }
        if self.tareas.lock().await.contains_key(id_fondo) {
            return Ok(ResultadoEjecucionComando {
                codigo_salida: None,
                salida: "(aún en ejecución)".to_string(),
                truncada: false,
                fondo: true,
                id_fondo: Some(id_fondo.to_string()),
                comando: String::new(),
                id_ejecucion: id_fondo.to_string(),
            });
        }
        Ok(ResultadoEjecucionComando {
            codigo_salida: None,
            salida: format!("(tarea de fondo desconocida: {id_fondo})"),
            truncada: false,
            fondo: true,
            id_fondo: Some(id_fondo.to_string()),
            comando: String::new(),
            id_ejecucion: id_fondo.to_string(),
        })
    }

    async fn matar(&self, id_fondo: &str) -> Result<()> {
        // `matar` NO toca `vivas` a propósito: el pump es el único que
        // retira y archiva (idempotente ante la carrera matar↔salida).
        self.matar_handle(id_fondo).await;
        Ok(())
    }

    async fn escribir(&self, id: &str, datos: &[u8]) -> Result<usize> {
        // [219A-3] Solo vivas de fondo: las transitorias síncronas no
        // retienen stdin y una terminada ya no tiene tubería (ambas →
        // `NoEncontrado`, la UI muestra el error honesto).
        if datos.len() > MAX_ESCRITURA_STDIN {
            return Err(Error::Limite(format!(
                "escritura a consola limitada a {MAX_ESCRITURA_STDIN} bytes por llamada"
            )));
        }
        let viva = self
            .registro
            .lock()
            .await
            .vivas
            .get(id)
            .cloned()
            .ok_or_else(|| {
                Error::NoEncontrado(format!("consola desconocida o terminada: {id}"))
            })?;
        let mut guardia = viva.stdin.lock().await;
        let stdin = guardia.as_mut().ok_or_else(|| {
            Error::NoEncontrado(format!("consola sin stdin (ya terminó): {id}"))
        })?;
        stdin.write_all(datos).await.map_err(|_| {
            Error::NoEncontrado(format!("la consola ya terminó (tubería rota): {id}"))
        })?;
        stdin.flush().await.map_err(|_| {
            Error::NoEncontrado(format!("la consola ya terminó (tubería rota): {id}"))
        })?;
        Ok(datos.len())
    }

    async fn salida(&self, id: &str) -> Result<TranscriptConsola> {
        // [219A-3] Backfill de la UI: viva = volcado del anillo (con flujo);
        // archivada = líneas del resultado guardado (flujo stdout: el archivo
        // mezcla ambos). Acotado a `MAX_LINEAS_TRANSCRIPT`.
        if let Some(viva) = self.registro.lock().await.vivas.get(id) {
            let anillo = viva.anillo.lock().await;
            let total = anillo.len();
            let desde = total.saturating_sub(MAX_LINEAS_TRANSCRIPT);
            return Ok(TranscriptConsola {
                id_ejecucion: id.to_string(),
                comando: viva.comando.clone(),
                viva: true,
                codigo_salida: None,
                lineas: anillo.iter().skip(desde).cloned().collect(),
                origen: viva.origen,
            });
        }
        if let Some(a) = self.registro.lock().await.archivadas.get(id) {
            let mut lineas: Vec<ChunkConsola> = a
                .resultado
                .salida
                .lines()
                .rev()
                .take(MAX_LINEAS_TRANSCRIPT)
                .map(|l| ChunkConsola {
                    flujo: FlujoConsola::Stdout,
                    linea: l.to_string(),
                })
                .collect();
            lineas.reverse();
            return Ok(TranscriptConsola {
                id_ejecucion: id.to_string(),
                comando: a.resultado.comando.clone(),
                viva: false,
                codigo_salida: a.resultado.codigo_salida,
                lineas,
                // [219A-5 F3] Dueño retenido al archivar (ya no `Agente` a
                // la fuerza: las propias siguen siendo propias al terminar).
                origen: a.origen,
            });
        }
        Err(Error::NoEncontrado(format!(
            "consola desconocida (el runner ya no la retiene): {id}"
        )))
    }
}
