/* [03-09-2026] Tool `comando` + gestión de tareas de fondo (plan 318A-16,
 * F3). El núcleo define la tool y su contrato; el runner real (timeout,
 * truncado a 8 KB, background con log propio) lo aporta el consumidor vía el
 * puerto `EjecutorComando`. Fail-closed: el runtime solo registra estas tools
 * cuando el puerto está presente; sin runner, el modelo ni las ve. */

use crate::bash_clasificar::clasificar_comando;
use crate::error::{Error, Result};
use crate::evento::AgenteEvento;
use crate::ports::{ChunkConsola, EjecutorComando};
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

pub struct ToolComando {
    ejecutor: Arc<dyn EjecutorComando>,
}

#[async_trait]
impl AgentTool for ToolComando {
    fn id(&self) -> &'static str {
        "comando"
    }
    fn descripcion(&self) -> &'static str {
        "Ejecuta un comando del sistema con revisión previa de riesgo (seguro/bajo/medio/alto/crítico).\n\
         FORMATO DE SALIDA: 'código de salida N' + salida capturada (stdout+stderr), con marcador de truncado si supera 8 KB.\n\
         LÍMITES: timeout del runner (120 s); el comando se clasifica y puede requerir aprobación por su TIPO de riesgo.\n\
         CUÁNDO USARLA: compilar/testear, git, scripts de verificación. Para cambios de archivos usa file_write/file_patch.\n\
         ERRORES: timeout, código de salida ≠ 0 o salida truncada se reportan explícitamente, nunca éxito falso.\n\
         VARIANTES: fondo=true lanza en background y devuelve un id; comando_status lo consulta y comando_matar lo termina."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "comando": {"type": "string", "description": "Comando a ejecutar"},
                "fondo": {"type": "boolean", "description": "Ejecutar en background (devuelve id_fondo; consultar con comando_status)"}
            },
            "required": ["comando"]
        })
    }
    fn efecto(&self) -> bool {
        true
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let comando = argumentos
            .get("comando")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("comando requerido".into()))?
            .to_string();
        let fondo = argumentos
            .get("fondo")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let nivel = clasificar_comando(&comando);
        let resumen = format!(
            "comando [{}] {}",
            nivel.clave(),
            comando.chars().take(60).collect::<String>()
        );
        /* [209A-1 F1] El id se genera ANTES de arrancar para que
         * `ConsolaInicio` preceda a cualquier chunk (el runner lo usa como
         * clave de registro en fondo y lo devuelve tal cual). Sin canal en
         * el contexto (tests) no se emite nada: el resultado final intacto. */
        let id_ejecucion = Uuid::new_v4().to_string();
        let t0 = std::time::Instant::now();
        if let Some(tx) = &ctx.tx_eventos {
            let _ = tx
                .send(AgenteEvento::ConsolaInicio {
                    id_ejecucion: id_ejecucion.clone(),
                    comando: comando.clone(),
                    conversacion_id: ctx.conversacion_id,
                })
                .await;
        }
        let (tx_chunk, rx_chunk) =
            tokio::sync::mpsc::unbounded_channel::<ChunkConsola>();
        let tx_eventos = ctx.tx_eventos.clone();
        let id_reenvio = id_ejecucion.clone();
        /* Reenvío chunks → turno. Termina solo (el runner suelta su emisor
         * al acabar) o al cerrar el turno (el send falla y se corta; la UI
         * ya no escucha y no hay nada que acumular aquí). */
        let reenvio = tokio::spawn(async move {
            let mut rx_chunk = rx_chunk;
            while let Some(chunk) = rx_chunk.recv().await {
                let Some(tx) = &tx_eventos else { break };
                let evento = AgenteEvento::ConsolaChunk {
                    id_ejecucion: id_reenvio.clone(),
                    flujo: chunk.flujo,
                    linea: chunk.linea,
                };
                if tx.send(evento).await.is_err() {
                    break;
                }
            }
        });
        let resultado = match self
            .ejecutor
            .ejecutar_en_vivo(
                &id_ejecucion,
                &comando,
                ctx.conversacion_id,
                fondo,
                tx_chunk,
            )
            .await
        {
            Ok(r) => r,
            /* [209A-1 F2] Tope de vivas: no es un fallo del comando sino del
             * plan (demasiados fondos simultáneos). `ok:false` accionable con
             * la vía de escape (`comando_lista`) en vez de error opaco. */
            Err(Error::Limite(detalle)) => {
                if !fondo {
                    let _ = reenvio.await;
                }
                return Ok(AgentToolResult {
                    ok: false,
                    contenido: format!(
                        "límite de consolas simultáneas: {detalle}\nConsulta comando_lista, espera a que termine alguna o mata una con comando_matar."
                    ),
                    resumen: "comando rechazado por tope de consolas".to_string(),
                    diff: None,
                    evento_extra: None,
                    consola_id: None,
                });
            }
            Err(e) => return Err(e),
        };
        /* En fondo el reenvío queda detached (sigue bombeando mientras el
         * turno viva); en síncrono se reúne: el runner ya soltó su emisor y
         * solo quedan los chunks en cola, que preceden al `ToolResult`. */
        if !fondo {
            let _ = reenvio.await;
        }
        let contenido = match resultado.id_fondo {
            Some(id) => format!(
                "[FONDO id={id}] riesgo {}\nLanzado en background; consulta con comando_status (id={id}).",
                nivel.clave()
            ),
            None => {
                let mut s = format!(
                    "código de salida: {}\nriesgo clasificado: {}\n{}",
                    resultado
                        .codigo_salida
                        .map_or_else(|| "n/a".to_string(), |c| c.to_string()),
                    nivel.clave(),
                    resultado.salida
                );
                if resultado.truncada {
                    s.push_str("\n[AVISO: salida truncada a 8 KB]");
                }
                s
            }
        };
        /* En síncrono el fin viaja como `evento_extra` (el runtime lo emite
         * tras `ToolResult`, en orden). En fondo NO hay fin todavía: la
         * tarea sigue corriendo y `comando_status` conserva su consulta; F2
         * emitirá el fin al archivar el resultado. */
        let evento_extra = if fondo {
            None
        } else {
            Some(AgenteEvento::ConsolaFin {
                id_ejecucion: id_ejecucion.clone(),
                codigo: resultado.codigo_salida,
                truncada: resultado.truncada,
                duracion_ms: t0.elapsed().as_millis() as u64,
            })
        };
        Ok(AgentToolResult {
            ok: true,
            contenido,
            resumen,
            diff: None,
            evento_extra,
            consola_id: Some(id_ejecucion),
        })
    }
}

pub struct ToolComandoStatus {
    ejecutor: Arc<dyn EjecutorComando>,
}

#[async_trait]
impl AgentTool for ToolComandoStatus {
    fn id(&self) -> &'static str {
        "comando_status"
    }
    fn descripcion(&self) -> &'static str {
        "Consulta el estado de una tarea de fondo lanzada con comando (fondo=true).\n\
         FORMATO DE SALIDA: 'en ejecución' o el código de salida + salida capturada.\n\
         ERRORES: id desconocido → error claro."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Id de la tarea de fondo (devuelto por comando)"}
            },
            "required": ["id"]
        })
    }
    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let id = argumentos
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("id requerido".into()))?
            .to_string();
        let r = self.ejecutor.estado(&id).await?;
        let contenido = match r.codigo_salida {
            None => format!("[FONDO id={id}] en ejecución"),
            Some(c) => {
                let mut s = format!("[FONDO id={id}] terminado con código {c}\n{}", r.salida);
                if r.truncada {
                    s.push_str("\n[AVISO: salida truncada a 8 KB]");
                }
                s
            }
        };
        Ok(AgentToolResult::ok(
            contenido,
            format!("estado de fondo {id}"),
        ))
    }
}

pub struct ToolComandoMatar {
    ejecutor: Arc<dyn EjecutorComando>,
}

#[async_trait]
impl AgentTool for ToolComandoMatar {
    fn id(&self) -> &'static str {
        "comando_matar"
    }
    fn descripcion(&self) -> &'static str {
        "Termina una tarea de fondo lanzada con comando (fondo=true).\n\
         FORMATO DE SALIDA: confirmación del id terminado.\n\
         ERRORES: id desconocido o proceso ya finalizado → error claro."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string", "description": "Id de la tarea de fondo a terminar"}
            },
            "required": ["id"]
        })
    }
    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let id = argumentos
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("id requerido".into()))?
            .to_string();
        self.ejecutor.matar(&id).await?;
        Ok(AgentToolResult::ok(
            format!("[FONDO id={id}] terminado"),
            format!("matado fondo {id}"),
        ))
    }
}

/// [209A-1 F2] `comando_lista`: vivas + recientes para el tab Consola y para
/// recuperarse del tope (`Ocupado`). Sin argumentos; filtra por conversación
/// solo en la UI (el modelo ve todas: son sus propios fondos).
pub struct ToolComandoLista {
    ejecutor: Arc<dyn EjecutorComando>,
}

#[async_trait]
impl AgentTool for ToolComandoLista {
    fn id(&self) -> &'static str {
        "comando_lista"
    }
    fn descripcion(&self) -> &'static str {
        "Lista las ejecuciones de comando en segundo plano (vivas y recientes).\n\
         FORMATO DE SALIDA: una línea por consola 'VIVA id=comando…' o 'hecha id=… código=N'.\n\
         Úsala tras un rechazo por tope de consolas o para reanudar el seguimiento con comando_status."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }
    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        _argumentos: Value,
    ) -> Result<AgentToolResult> {
        let infos = self.ejecutor.lista().await?;
        if infos.is_empty() {
            return Ok(AgentToolResult::ok(
                "sin consolas: no hay ejecuciones en segundo plano.",
                "lista de consolas vacía",
            ));
        }
        let mut lineas = Vec::with_capacity(infos.len());
        for info in &infos {
            if info.viva {
                lineas.push(format!(
                    "VIVA id={} bytes={} {}",
                    info.id_ejecucion, info.bytes, info.comando
                ));
            } else {
                lineas.push(format!(
                    "hecha id={} código={} {}",
                    info.id_ejecucion,
                    info.codigo_salida
                        .map_or_else(|| "n/a".to_string(), |c| c.to_string()),
                    info.comando
                ));
            }
        }
        Ok(AgentToolResult::ok(
            lineas.join("\n"),
            format!("lista de {} consolas", infos.len()),
        ))
    }
}

/// Registra la tool `comando` y su gestión de fondo SOLO cuando hay runner
/// (fail-closed: sin `ejecutor`, el runtime no llama a esta función y el
/// modelo no ve la tool).
pub fn registrar_tools_comando(
    registry: &mut crate::tool::AgentToolRegistry,
    ejecutor: Arc<dyn EjecutorComando>,
) {
    registry.registrar(Box::new(ToolComando {
        ejecutor: Arc::clone(&ejecutor),
    }));
    registry.registrar(Box::new(ToolComandoStatus {
        ejecutor: Arc::clone(&ejecutor),
    }));
    registry.registrar(Box::new(ToolComandoMatar {
        ejecutor: Arc::clone(&ejecutor),
    }));
    registry.registrar(Box::new(ToolComandoLista { ejecutor }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contrato_tests::PersistenciaMock;
    use crate::evento::FlujoConsola;
    use crate::ports::{ChunkConsola, InfoConsola, OrigenConsola, ResultadoEjecucionComando};
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Runner de pruebas: devuelve salida fija, registra lo ejecutado.
    struct EjecutorMock {
        llamado: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl EjecutorComando for EjecutorMock {
        async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando> {
            self.llamado.lock().unwrap().push(comando.to_string());
            Ok(ResultadoEjecucionComando {
                codigo_salida: Some(if fondo { 0 } else { 7 }),
                salida: if fondo {
                    String::new()
                } else {
                    "salida de prueba".into()
                },
                truncada: false,
                fondo,
                id_fondo: fondo.then(|| "f-1".to_string()),
                comando: comando.to_string(),
                id_ejecucion: "f-1".to_string(),
            })
        }
        /* Eco del id como un runner real (contrato de `ejecutar_en_vivo`):
         * la tool genera el id y el runner lo devuelve tal cual. */
        async fn ejecutar_en_vivo(
            &self,
            id: &str,
            comando: &str,
            _conversacion_id: Uuid,
            fondo: bool,
            _chunks: tokio::sync::mpsc::UnboundedSender<ChunkConsola>,
        ) -> Result<ResultadoEjecucionComando> {
            let mut r = self.ejecutar(comando, fondo).await?;
            r.id_fondo = fondo.then(|| id.to_string());
            r.id_ejecucion = id.to_string();
            Ok(r)
        }
        async fn estado(&self, _id: &str) -> Result<ResultadoEjecucionComando> {
            Ok(ResultadoEjecucionComando {
                codigo_salida: Some(0),
                salida: "hecho".into(),
                truncada: false,
                fondo: true,
                id_fondo: Some("f-1".to_string()),
                comando: String::new(),
                id_ejecucion: "f-1".to_string(),
            })
        }
        async fn matar(&self, _id: &str) -> Result<()> {
            Ok(())
        }
    }

    /// Runner que además bombea dos líneas en vivo antes de devolver.
    struct EjecutorVivo {
        base: EjecutorMock,
    }

    #[async_trait]
    impl EjecutorComando for EjecutorVivo {
        async fn ejecutar(&self, comando: &str, fondo: bool) -> Result<ResultadoEjecucionComando> {
            self.base.ejecutar(comando, fondo).await
        }
        async fn ejecutar_en_vivo(
            &self,
            id: &str,
            comando: &str,
            conversacion_id: Uuid,
            fondo: bool,
            chunks: tokio::sync::mpsc::UnboundedSender<ChunkConsola>,
        ) -> Result<ResultadoEjecucionComando> {
            let _ = chunks.send(ChunkConsola {
                flujo: FlujoConsola::Stdout,
                linea: "línea uno".into(),
            });
            let _ = chunks.send(ChunkConsola {
                flujo: FlujoConsola::Stderr,
                linea: "aviso dos".into(),
            });
            self.base.ejecutar_en_vivo(id, comando, conversacion_id, fondo, chunks).await
        }
        async fn estado(&self, id: &str) -> Result<ResultadoEjecucionComando> {
            self.base.estado(id).await
        }
        async fn matar(&self, id: &str) -> Result<()> {
            self.base.matar(id).await
        }
    }

    fn ctx<'a>(persistencia: &'a PersistenciaMock) -> AgentToolContext<'a> {
        AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: Uuid::new_v4(),
            persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
            conversacion_id: Uuid::new_v4(),
            tx_eventos: None,
        }
    }

    fn ctx_vivo<'a>(
        persistencia: &'a PersistenciaMock,
        tx: tokio::sync::mpsc::Sender<AgenteEvento>,
    ) -> AgentToolContext<'a> {
        AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: Uuid::new_v4(),
            persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
            conversacion_id: Uuid::new_v4(),
            tx_eventos: Some(tx),
        }
    }

    #[tokio::test]
    async fn ejecuta_y_reporte_codigo() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorMock {
            llamado: Mutex::new(Vec::new()),
        });
        let tool = ToolComando {
            ejecutor: Arc::clone(&ejecutor) as Arc<dyn EjecutorComando>,
        };
        let r = tool
            .ejecutar(&ctx(&mock), json!({"comando": "cargo check"}))
            .await
            .unwrap();
        assert!(r.contenido.contains("código de salida: 7"));
        assert!(r.contenido.contains("salida de prueba"));
        assert!(r.resumen.contains("bajo"));
        assert_eq!(ejecutor.llamado.lock().unwrap().len(), 1);
    }

    /* [209A-1 F1] Streaming: con canal en el contexto, la ejecución
     * síncrona emite `ConsolaInicio` (comando verbatim) + chunks en orden y
     * devuelve `ConsolaFin` como evento_extra con el mismo id. */
    #[tokio::test]
    async fn sincrono_emite_inicio_chunks_y_fin_en_orden() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorVivo {
            base: EjecutorMock {
                llamado: Mutex::new(Vec::new()),
            },
        });
        let tool = ToolComando {
            ejecutor: Arc::clone(&ejecutor) as Arc<dyn EjecutorComando>,
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let r = tool
            .ejecutar(
                &ctx_vivo(&mock, tx),
                json!({"comando": "cargo test --lib con banderas --largas para el verbatim"}),
            )
            .await
            .unwrap();
        let id = r.consola_id.clone().expect("consola_id");
        assert!(r.contenido.contains("código de salida: 7"));

        let mut eventos = Vec::new();
        while let Ok(ev) =
            tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await
        {
            let Some(ev) = ev else { break };
            eventos.push(ev);
            if eventos.len() >= 3 {
                break;
            }
        }
        assert_eq!(eventos.len(), 3, "eventos: {eventos:?}");
        match &eventos[0] {
            AgenteEvento::ConsolaInicio {
                id_ejecucion,
                comando,
                ..
            } => {
                assert_eq!(id_ejecucion, &id);
                assert_eq!(comando, "cargo test --lib con banderas --largas para el verbatim");
            }
            otro => panic!("primero debe ser inicio: {otro:?}"),
        }
        match &eventos[1] {
            AgenteEvento::ConsolaChunk {
                id_ejecucion,
                flujo,
                linea,
            } => {
                assert_eq!(id_ejecucion, &id);
                assert_eq!(*flujo, FlujoConsola::Stdout);
                assert_eq!(linea, "línea uno");
            }
            otro => panic!("segundo debe ser chunk stdout: {otro:?}"),
        }
        match &eventos[2] {
            AgenteEvento::ConsolaChunk {
                id_ejecucion,
                flujo,
                linea,
            } => {
                assert_eq!(id_ejecucion, &id);
                assert_eq!(*flujo, FlujoConsola::Stderr);
                assert_eq!(linea, "aviso dos");
            }
            otro => panic!("tercero debe ser chunk stderr: {otro:?}"),
        }
        match r.evento_extra {
            Some(AgenteEvento::ConsolaFin {
                id_ejecucion,
                codigo,
                ..
            }) => {
                assert_eq!(id_ejecucion, id);
                assert_eq!(codigo, Some(7));
            }
            otro => panic!("evento_extra debe ser fin: {otro:?}"),
        }
    }

    #[tokio::test]
    async fn fondo_devuelve_id_y_status_lo_consulta() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorMock {
            llamado: Mutex::new(Vec::new()),
        });
        let tool = ToolComando {
            ejecutor: Arc::clone(&ejecutor) as Arc<dyn EjecutorComando>,
        };
        let r = tool
            .ejecutar(
                &ctx(&mock),
                json!({"comando": "cargo build", "fondo": true}),
            )
            .await
            .unwrap();
        /* El mock hace eco del id generado por la tool: el contenido muestra
         * ese mismo id y `consola_id` lo porta para el evento `ToolResult`. */
        let id = r.consola_id.clone().expect("fondo con consola_id");
        assert!(r.contenido.contains(&format!("[FONDO id={id}]")));
        assert!(r.evento_extra.is_none(), "en fondo no hay fin todavía");

        let status = ToolComandoStatus {
            ejecutor: Arc::clone(&ejecutor) as Arc<dyn EjecutorComando>,
        };
        let s = status
            .ejecutar(&ctx(&mock), json!({"id": "f-1"}))
            .await
            .unwrap();
        assert!(s.contenido.contains("terminado con código 0"));
    }

    #[tokio::test]
    async fn argumentos_invalidos_dan_error() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorMock {
            llamado: Mutex::new(Vec::new()),
        });
        let tool = ToolComando {
            ejecutor: Arc::clone(&ejecutor) as Arc<dyn EjecutorComando>,
        };
        let err = tool.ejecutar(&ctx(&mock), json!({})).await.unwrap_err();
        assert!(err.to_string().contains("comando requerido"));
    }

    /* [318A-16 F3] Fail-closed de registro: `registrar_tools_comando` expone
     * `comando`, `comando_status` y `comando_matar`; el runtime solo la llama
     * cuando hay runner (`PuertosHarness.ejecutor_comando: Some`), así que sin
     * runner el modelo nunca ve la tool. */
    #[test]
    fn registrar_expone_herramientas_comando() {
        let mut registry = crate::tool::AgentToolRegistry::new();
        let ejecutor = Arc::new(EjecutorMock {
            llamado: Mutex::new(Vec::new()),
        }) as Arc<dyn EjecutorComando>;
        registrar_tools_comando(&mut registry, ejecutor);
        let schemas = registry.schemas_openai(None, "predeterminado");
        let nombres: Vec<&str> = schemas
            .iter()
            .filter_map(|s| {
                s["function"]["name"]
                    .as_str()
                    .or_else(|| s["name"].as_str())
            })
            .collect();
        assert!(nombres.contains(&"comando"), "nombres: {nombres:?}");
        assert!(nombres.contains(&"comando_status"));
        assert!(nombres.contains(&"comando_matar"));
        assert!(nombres.contains(&"comando_lista"), "nombres: {nombres:?}");
    }

    /// [209A-1 F2] Runner fijo: lista dada + fallo `Limite` opcional (tope).
    struct EjecutorF2 {
        infos: Vec<InfoConsola>,
        limite: bool,
    }

    #[async_trait]
    impl EjecutorComando for EjecutorF2 {
        async fn ejecutar(
            &self,
            comando: &str,
            fondo: bool,
        ) -> Result<ResultadoEjecucionComando> {
            Ok(ResultadoEjecucionComando {
                codigo_salida: Some(0),
                salida: String::new(),
                truncada: false,
                fondo,
                id_fondo: fondo.then(|| "x".to_string()),
                comando: comando.to_string(),
                id_ejecucion: "x".to_string(),
            })
        }
        async fn ejecutar_en_vivo(
            &self,
            _id: &str,
            comando: &str,
            _conversacion_id: Uuid,
            fondo: bool,
            _chunks: tokio::sync::mpsc::UnboundedSender<ChunkConsola>,
        ) -> Result<ResultadoEjecucionComando> {
            if self.limite {
                return Err(Error::Limite(
                    "límite de consolas vivas alcanzado (4)".to_string(),
                ));
            }
            self.ejecutar(comando, fondo).await
        }
        async fn estado(&self, _id: &str) -> Result<ResultadoEjecucionComando> {
            self.ejecutar("", true).await
        }
        async fn matar(&self, _id: &str) -> Result<()> {
            Ok(())
        }
        async fn lista(&self) -> Result<Vec<InfoConsola>> {
            Ok(self.infos.clone())
        }
    }

    fn info_viva(id: &str) -> InfoConsola {
        InfoConsola {
            id_ejecucion: id.to_string(),
            comando: "cargo build".to_string(),
            conversacion_id: Uuid::nil(),
            viva: true,
            codigo_salida: None,
            bytes: 128,
            origen: OrigenConsola::Agente,
        }
    }

    fn info_hecha(id: &str) -> InfoConsola {
        InfoConsola {
            id_ejecucion: id.to_string(),
            comando: "cargo test".to_string(),
            conversacion_id: Uuid::nil(),
            viva: false,
            codigo_salida: Some(0),
            bytes: 512,
            origen: OrigenConsola::Agente,
        }
    }

    #[tokio::test]
    async fn lista_muestra_vivas_y_hechas() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorF2 {
            infos: vec![info_viva("v-1"), info_hecha("h-1")],
            limite: false,
        });
        let tool = ToolComandoLista {
            ejecutor: ejecutor as Arc<dyn EjecutorComando>,
        };
        let r = tool.ejecutar(&ctx(&mock), json!({})).await.unwrap();
        assert!(r.ok);
        assert!(
            r.contenido.contains("VIVA id=v-1"),
            "contenido: {}",
            r.contenido
        );
        assert!(
            r.contenido.contains("hecha id=h-1 código=0"),
            "contenido: {}",
            r.contenido
        );
    }

    #[tokio::test]
    async fn lista_vacia_dice_sin_consolas() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorF2 {
            infos: Vec::new(),
            limite: false,
        });
        let tool = ToolComandoLista {
            ejecutor: ejecutor as Arc<dyn EjecutorComando>,
        };
        let r = tool.ejecutar(&ctx(&mock), json!({})).await.unwrap();
        assert!(r.ok);
        assert!(r.contenido.contains("sin consolas"), "contenido: {}", r.contenido);
    }

    #[tokio::test]
    async fn tope_de_vivas_da_ok_false_con_via_de_escape() {
        let mock = PersistenciaMock::default();
        let ejecutor = Arc::new(EjecutorF2 {
            infos: Vec::new(),
            limite: true,
        });
        let tool = ToolComando {
            ejecutor: ejecutor as Arc<dyn EjecutorComando>,
        };
        let r = tool
            .ejecutar(
                &ctx(&mock),
                json!({"comando": "cargo build", "fondo": true}),
            )
            .await
            .unwrap();
        assert!(!r.ok, "el tope no es éxito: {}", r.contenido);
        assert!(r.contenido.contains("comando_lista"), "contenido: {}", r.contenido);
        assert!(r.consola_id.is_none(), "sin consola que seguir");
        assert!(r.evento_extra.is_none(), "sin fin que emitir");
    }
}
