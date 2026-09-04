/* [03-09-2026] Tool `comando` + gestión de tareas de fondo (plan 318A-16,
 * F3). El núcleo define la tool y su contrato; el runner real (timeout,
 * truncado a 8 KB, background con log propio) lo aporta el consumidor vía el
 * puerto `EjecutorComando`. Fail-closed: el runtime solo registra estas tools
 * cuando el puerto está presente; sin runner, el modelo ni las ve. */

use crate::bash_clasificar::clasificar_comando;
use crate::error::{Error, Result};
use crate::ports::EjecutorComando;
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

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
        _ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let comando = argumentos
            .get("comando")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("comando requerido".into()))?
            .to_string();
        let fondo = argumentos.get("fondo").and_then(Value::as_bool).unwrap_or(false);
        let nivel = clasificar_comando(&comando);
        let resumen = format!(
            "comando [{}] {}",
            nivel.clave(),
            comando.chars().take(60).collect::<String>()
        );
        let resultado = self.ejecutor.ejecutar(&comando, fondo).await?;
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
        Ok(AgentToolResult {
            ok: true,
            contenido,
            resumen,
            diff: None,
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

/// Registra la tool `comando` y su gestión de fondo SOLO cuando hay runner
/// (fail-closed: sin `ejecutor`, el runtime no llama a esta función y el
/// modelo no ve la tool).
pub fn registrar_tools_comando(registry: &mut crate::tool::AgentToolRegistry, ejecutor: Arc<dyn EjecutorComando>) {
    registry.registrar(Box::new(ToolComando {
        ejecutor: Arc::clone(&ejecutor),
    }));
    registry.registrar(Box::new(ToolComandoStatus {
        ejecutor: Arc::clone(&ejecutor),
    }));
    registry.registrar(Box::new(ToolComandoMatar { ejecutor }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contrato_tests::PersistenciaMock;
    use crate::ports::ResultadoEjecucionComando;
    use std::sync::Mutex;
    use uuid::Uuid;

    /// Runner de pruebas: devuelve salida fija, registra lo ejecutado.
    struct EjecutorMock {
        llamado: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl EjecutorComando for EjecutorMock {
        async fn ejecutar(
            &self,
            comando: &str,
            fondo: bool,
        ) -> Result<ResultadoEjecucionComando> {
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
            })
        }
        async fn estado(&self, _id: &str) -> Result<ResultadoEjecucionComando> {
            Ok(ResultadoEjecucionComando {
                codigo_salida: Some(0),
                salida: "hecho".into(),
                truncada: false,
                fondo: true,
                id_fondo: Some("f-1".to_string()),
            })
        }
        async fn matar(&self, _id: &str) -> Result<()> {
            Ok(())
        }
    }

    fn ctx<'a>(persistencia: &'a PersistenciaMock) -> AgentToolContext<'a> {
        AgentToolContext {
            user_id: Uuid::new_v4(),
            persistencia,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
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
            .ejecutar(&ctx(&mock), json!({"comando": "cargo build", "fondo": true}))
            .await
            .unwrap();
        assert!(r.contenido.contains("[FONDO id=f-1]"));

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
    }
}