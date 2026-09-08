//! [079A-1 F4] Tests del dominio navegador (partidos de herramientas/navegador.rs).

//! Módulo solo-test (también declarado tras `#[cfg(test)]`): el atributo
//! interno marca el fichero para analizadores por fichero.
#![cfg(test)]

use super::*;

use crate::error::Result;
use crate::ports::NavegadorPort;
use crate::tool::{AgentTool, AgentToolContext};
use async_trait::async_trait;
use serde_json::json;

struct StubNavegador {
    abierto: std::sync::Mutex<bool>,
}

#[async_trait]

impl NavegadorPort for StubNavegador {
    async fn abrir(&self, _url: &str) -> Result<()> {
        *self.abierto.lock().unwrap() = true;

        Ok(())
    }

    async fn navegar(&self, _url: &str) -> Result<()> {
        Ok(())
    }

    async fn capturar(&self) -> Result<String> {
        Ok("stub_captura_base64".into())
    }

    async fn js(&self, _codigo: &str) -> Result<String> {
        Ok("stub_resultado".into())
    }

    async fn cdp(&self, _metodo: &str, _parametros: &str) -> Result<String> {
        Ok("{}".into())
    }

    async fn click(&self, _selector: &str) -> Result<()> {
        Ok(())
    }

    async fn rellenar(&self, _selector: &str, _valor: &str) -> Result<()> {
        Ok(())
    }

    async fn snapshot(&self, _selector: &str) -> Result<String> {
        Ok("<html/>".into())
    }

    async fn cerrar(&self) -> Result<()> {
        *self.abierto.lock().unwrap() = false;

        Ok(())
    }
}

fn ctx_con_navegador() -> AgentToolContext<'static> {
    let persistencia: &'static crate::contrato_tests::PersistenciaMock =
        Box::leak(Box::new(crate::contrato_tests::PersistenciaMock::default()));

    let stub: &'static StubNavegador = Box::leak(Box::new(StubNavegador {
        abierto: std::sync::Mutex::new(false),
    }));

    AgentToolContext {
        user_id: uuid::Uuid::nil(),

        persistencia,

        web_search: None,

        web_fetch: None,

        ai_provider: None,

        sandbox_archivos: None,

        dominio: None,

        todo: None,

        plan: None,

        navegador: Some(stub),
    }
}

fn ctx_sin_navegador() -> AgentToolContext<'static> {
    let persistencia: &'static crate::contrato_tests::PersistenciaMock =
        Box::leak(Box::new(crate::contrato_tests::PersistenciaMock::default()));

    AgentToolContext {
        user_id: uuid::Uuid::new_v4(),

        persistencia,

        web_search: None,

        web_fetch: None,

        ai_provider: None,

        sandbox_archivos: None,

        dominio: None,

        todo: None,

        plan: None,

        navegador: None,
    }
}

#[tokio::test]

async fn t01_abrir_exitoso() {
    let tool = ToolNavegadorReflejo;

    let args = json!({"operacion": "abrir", "url": "https://example.com"});

    let r = tool.ejecutar(&ctx_con_navegador(), args).await.unwrap();

    assert!(r.ok);

    assert!(r.contenido.contains("example.com"));
}

#[tokio::test]

async fn t02_falla_sin_navegador() {
    let tool = ToolNavegadorReflejo;

    let args = json!({"operacion": "abrir", "url": "https://example.com"});

    let e = tool.ejecutar(&ctx_sin_navegador(), args).await;

    assert!(e.is_err());

    let msg = e.unwrap_err().to_string();

    assert!(msg.contains("no está disponible") || msg.contains("disponible"));
}

#[tokio::test]

async fn t03_operacion_invalida() {
    let tool = ToolNavegadorReflejo;

    let args = json!({"operacion": "volar"});

    let e = tool.ejecutar(&ctx_con_navegador(), args).await;

    assert!(e.is_err());
}

#[tokio::test]

async fn t04_capturar_con_navegador() {
    let tool = ToolNavegadorReflejo;

    let args = json!({"operacion": "capturar"});

    let r = tool.ejecutar(&ctx_con_navegador(), args).await.unwrap();

    assert!(r.ok);

    assert!(r.contenido.contains("stub_captura"));
}

#[tokio::test]

async fn t05_cierra_y_reabre_stub() {
    let nav = StubNavegador {
        abierto: std::sync::Mutex::new(false),
    };

    nav.abrir("https://ej.com").await.unwrap();

    assert!(*nav.abierto.lock().unwrap());

    nav.cerrar().await.unwrap();

    assert!(!*nav.abierto.lock().unwrap());
}

#[tokio::test]

async fn t06_id_y_schema() {
    let tool = ToolNavegadorReflejo;

    assert_eq!(tool.id(), "navegador_reflejo");

    let s = tool.schema();

    assert!(s
        .get("properties")
        .and_then(|p| p.get("operacion"))
        .is_some());
}
