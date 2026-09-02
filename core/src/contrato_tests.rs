//! Pruebas de contrato de Fase 1a: los tres puertos son implementables desde
//! un consumidor ajeno (mock in-memory, sin sqlx, sin AppState, sin tablas).
//! Fijan que el núcleo no filtra tipos de task y que el serde del contrato
//! (snake_case, igual que el API de task) se mantiene estable.

use tokio::sync::mpsc;

use crate::evento::TokenStream;
use crate::ports::*;

// ---------------------------------------------------------------------------
// Mocks in-memory del consumidor
// ---------------------------------------------------------------------------

/// Persistencia en memoria: sola para tests, demuestra que un consumidor
/// puede implementar `AgentPersistence` sin conocer la base de task.
#[derive(Default)]
pub struct PersistenciaMock {
    mensajes: Vec<MensajePersistido>,
    memoria: Vec<(uuid::Uuid, String, String)>,
    skills: Vec<SkillEntrada>,
    tareas: Vec<TareaProgramadaPendiente>,
}

#[async_trait::async_trait]
impl AgentPersistence for PersistenciaMock {
    async fn guardar_turno(&self, _turno: &TurnoPersistido) -> crate::error::Result<()> {
        Ok(())
    }
    async fn finalizar_turno(
        &self,
        _turno_id: uuid::Uuid,
        _estado: &str,
        _resumen: Option<&str>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> crate::error::Result<()> {
        // Mutex interior no hace falta en test: se usa con `&self` para
        // respetar el trait, pero el test solo comprueba compilación.
        let _ = mensaje;
        Ok(())
    }
    async fn listar_mensajes(
        &self,
        _conversacion_id: uuid::Uuid,
    ) -> crate::error::Result<Vec<MensajePersistido>> {
        Ok(self.mensajes.clone())
    }
    async fn conversacion_tocar(&self, _conversacion_id: uuid::Uuid) -> crate::error::Result<()> {
        Ok(())
    }
    async fn registrar_accion(&self, accion: &AccionAuditable) -> crate::error::Result<()> {
        let _ = accion.tool.as_str();
        Ok(())
    }
    async fn memoria_listar(
        &self,
        _user_id: uuid::Uuid,
    ) -> crate::error::Result<Vec<MemoriaEntrada>> {
        Ok(self
            .memoria
            .iter()
            .map(|(_, clave, contenido)| MemoriaEntrada {
                clave: clave.clone(),
                contenido: contenido.clone(),
            })
            .collect())
    }
    async fn memoria_upsert(
        &self,
        _user_id: uuid::Uuid,
        _entrada: &MemoriaEntrada,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn memoria_borrar(&self, _user_id: uuid::Uuid, _clave: &str) -> crate::error::Result<()> {
        Ok(())
    }
    async fn skills_listar(&self, _user_id: uuid::Uuid) -> crate::error::Result<Vec<SkillEntrada>> {
        Ok(self.skills.clone())
    }
    async fn tareas_recuperar_interrumpidas(&self) -> crate::error::Result<u64> {
        Ok(0)
    }
    async fn tareas_pendientes(
        &self,
        _limite: u32,
    ) -> crate::error::Result<Vec<TareaProgramadaPendiente>> {
        Ok(self.tareas.clone())
    }
    async fn tarea_tomar(&self, _id: uuid::Uuid) -> crate::error::Result<bool> {
        Ok(true)
    }
    async fn tarea_finalizar(
        &self,
        _id: uuid::Uuid,
        _ok: bool,
        _resumen: Option<&str>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
    async fn tarea_reprogramar(
        &self,
        _id: uuid::Uuid,
        _user_id: uuid::Uuid,
        _proxima: Option<chrono::DateTime<chrono::Utc>>,
    ) -> crate::error::Result<()> {
        Ok(())
    }
}

/// Proveedor LLM falso: emite dos tokens y un fin.
struct ProveedorMock;

#[async_trait::async_trait]
impl ProviderPort for ProveedorMock {
    async fn chat_stream(&self, request: ChatRequest) -> crate::error::Result<TokenStream> {
        let (tx, rx) = mpsc::unbounded_channel();
        assert!(!request.mensajes.is_empty(), "el request debe llevar mensajes");
        let _ = tx.send(EventoTurno::Token {
            texto: "hola".into(),
        });
        let _ = tx.send(EventoTurno::Fin { motivo: "stop".into() });
        Ok(rx)
    }
}

/// Búsqueda web falsa: devuelve un resultado fijo.
pub struct WebMock;

#[async_trait::async_trait]
impl WebSearchProvider for WebMock {
    async fn buscar(&self, query: &str, limite: usize) -> crate::error::Result<Vec<ResultadoWeb>> {
        Ok(vec![ResultadoWeb {
            titulo: query.to_string(),
            url: format!("https://ejemplo.test/{limite}"),
            fragmento: "fragmento".into(),
        }])
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn puertos_implementables_por_consumidor_ajeno() {
    let persistencia = PersistenciaMock::default();
    let _ = persistencia
        .listar_mensajes(uuid::Uuid::new_v4())
        .await
        .expect("puerto de persistencia responde");

    let proveedor = ProveedorMock;
    let mut stream = proveedor
        .chat_stream(ChatRequest {
            system: "sys".into(),
            mensajes: vec![ChatMensaje {
                rol: "user".into(),
                contenido: "hola".into(),
            }],
            modelo: "commandcode".into(),
            temperatura: None,
            max_tokens: None,
            sesion_id: None,
            extra: Default::default(),
        })
        .await
        .expect("puerto de proveedor responde");
    while let Some(evento) = stream.recv().await {
        if let EventoTurno::Fin { .. } = evento {
            break;
        }
    }

    let web = WebMock;
    let resultados = web.buscar("prueba", 3).await.expect("puerto web responde");
    assert_eq!(resultados.len(), 1);
}

#[test]
fn contrato_serde_snake_case_estable() {
    let mensaje = MensajePersistido {
        id: uuid::Uuid::new_v4(),
        conversacion_id: uuid::Uuid::new_v4(),
        rol: "user".into(),
        contenido: "hola".into(),
        creado_en: chrono::Utc::now(),
    };
    let json = serde_json::to_value(&mensaje).expect("serializa");
    let objeto = json.as_object().expect("objeto");
    // Claves en snake_case, igual que el API de task (contrato estable).
    for clave in ["id", "conversacion_id", "rol", "contenido", "creado_en"] {
        assert!(objeto.contains_key(clave), "falta clave {clave}: {json}");
    }
    // Y vuelve a deserializar intacto (ida y vuelta).
    let recuperado: MensajePersistido = serde_json::from_value(json).expect("deserializa");
    assert_eq!(recuperado.contenido, "hola");
}

#[test]
fn contrato_skill_serde_con_activa() {
    let skill = SkillEntrada {
        id: uuid::Uuid::new_v4(),
        nombre: "revisar".into(),
        descripcion: "Revisa código".into(),
        instrucciones: "Busca bugs".into(),
        activa: true,
    };
    let json = serde_json::to_value(&skill).expect("serializa");
    assert!(json["activa"].as_bool().unwrap_or(false));
    assert_eq!(json["nombre"], "revisar");
}