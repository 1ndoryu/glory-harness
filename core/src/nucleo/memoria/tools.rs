//! Tools `memoria_*` del agente (diseño §1 + fase 5).

use async_trait::async_trait;
use serde_json::{json, Value};

use super::proveedor::puntuar_y_formatear;
use super::sanitize::sanitize_para_memoria;
use crate::error::{Error, Result};
use crate::ports::MemoriaEntrada;
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};

/// Tool `memoria_guardar`: alta explícita de un recuerdo (con sanitizado;
/// los secretos se rechazan con error claro, nunca se guardan).
pub struct ToolMemoriaGuardar;

#[async_trait]
impl AgentTool for ToolMemoriaGuardar {
    fn id(&self) -> &str {
        "memoria_guardar"
    }
    fn descripcion(&self) -> &str {
        "Guarda un recuerdo persistente del usuario para futuros turnos.\
        \nQUÉ HACE: alta (o sustitución) de una entrada clave → contenido con fecha y origen.\
        \nCUÁNDO USARLA: cuando el usuario pide recordar algo explícitamente ('recuerda que...', 'a partir de ahora...') o hay un dato claramente reutilizable (preferencias, rutas, nombres).\
        \nFORMATO DE SALIDA: confirma la clave guardada.\
        \nERRORES: clave o contenido vacíos; contenido que parece credencial (no se guarda nunca)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "clave": { "type": "string", "description": "Identificador corto (p. ej. 'color-favorito')" },
                "contenido": { "type": "string", "description": "Lo que hay que recordar (sin secretos)" }
            },
            "required": ["clave", "contenido"]
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
        let clave = argumentos
            .get("clave")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_guardar: clave requerida".into()))?;
        let contenido = argumentos
            .get("contenido")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("memoria_guardar: contenido requerido".into()))?;
        let Some(limpio) = sanitize_para_memoria(contenido) else {
            return Err(Error::Validacion(
                "memoria_guardar: el contenido parece una credencial o está vacío; no se guarda"
                    .into(),
            ));
        };
        let entrada =
            MemoriaEntrada::nueva(clave.to_string(), limpio, "tool:memoria_guardar".into());
        ctx.persistencia
            .memoria_upsert(ctx.user_id, &entrada)
            .await?;
        Ok(AgentToolResult::ok(
            format!("recuerdo '{}' guardado", entrada.clave),
            format!("memoria_guardar: {}", entrada.clave),
        ))
    }
}

/// Tool `memoria_recordar`: búsqueda explícita en los recuerdos (solo
/// lectura; el prefetch automático ya inyecta lo relevante sin pedirlo).
pub struct ToolMemoriaRecordar;

#[async_trait]
impl AgentTool for ToolMemoriaRecordar {
    fn id(&self) -> &str {
        "memoria_recordar"
    }
    fn descripcion(&self) -> &str {
        "Busca en los recuerdos persistentes del usuario.\
        \nQUÉ HACE: devuelve las entradas que solapan con la consulta (hasta el límite).\
        \nCUÁNDO USARLA: cuando necesitas un dato del usuario que el contexto automático no trajo.\
        \nFORMATO DE SALIDA: una línea por recuerdo ('- clave: contenido'); vacío si no hay coincidencias."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "consulta": { "type": "string", "description": "Qué buscar (p. ej. 'color favorito')" },
                "limite": { "type": "integer", "description": "Tope de caracteres (default 2000, máx 8000)" }
            },
            "required": ["consulta"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let consulta = argumentos
            .get("consulta")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_recordar: consulta requerida".into()))?;
        let limite = argumentos
            .get("limite")
            .and_then(Value::as_u64)
            .map(|l| (l as usize).clamp(1, 8000))
            .unwrap_or(2000);
        let entradas = ctx.persistencia.memoria_listar(ctx.user_id).await?;
        let (bloque, claves) = puntuar_y_formatear(&entradas, consulta, limite);
        if bloque.is_empty() {
            return Ok(AgentToolResult::ok(
                "(sin recuerdos coincidentes)",
                "memoria_recordar: sin resultados".to_string(),
            ));
        }
        Ok(AgentToolResult::ok(
            bloque.clone(),
            format!("memoria_recordar: {} ({})", claves.len(), claves.join(", ")),
        ))
    }
}

/// Tool `memoria_borrar`: elimina un recuerdo por clave (reversible solo si
/// el operador lo recuerda: pedir confirmación en lenguaje natural antes).
pub struct ToolMemoriaBorrar;

#[async_trait]
impl AgentTool for ToolMemoriaBorrar {
    fn id(&self) -> &str {
        "memoria_borrar"
    }
    fn descripcion(&self) -> &str {
        "Borra un recuerdo persistente por su clave.\
        \nQUÉ HACE: elimina la entrada (el curador nunca la recuperará).\
        \nCUÁNDO USARLA: cuando el usuario pide olvidar algo explícitamente.\
        \nFORMATO DE SALIDA: confirma la clave borrada (aunque no existiera, para no filtrar qué hay guardado)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "clave": { "type": "string", "description": "Clave del recuerdo a borrar" }
            },
            "required": ["clave"]
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
        let clave = argumentos
            .get("clave")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_borrar: clave requerida".into()))?;
        ctx.persistencia.memoria_borrar(ctx.user_id, clave).await?;
        Ok(AgentToolResult::ok(
            format!("recuerdo '{clave}' borrado"),
            format!("memoria_borrar: {clave}"),
        ))
    }
}

/// Registra las tres tools de memoria (siempre: la persistencia es puerto
/// obligatorio, nunca `None`; el sanitizado protege la escritura).
pub fn registrar_tools_memoria(registry: &mut AgentToolRegistry) {
    registry.registrar(Box::new(ToolMemoriaGuardar));
    registry.registrar(Box::new(ToolMemoriaRecordar));
    registry.registrar(Box::new(ToolMemoriaBorrar));
}

#[cfg(test)]
mod pruebas {
    //! [069A-4] Tools con tienda observable en memoria.
    use super::*;
    use crate::memoria::soporte::TiendaPrueba;
    use serde_json::json;
    use uuid::Uuid;

    fn contexto<'a>(tienda: &'a TiendaPrueba, user_id: Uuid) -> AgentToolContext<'a> {
        AgentToolContext {
            user_id,
            persistencia: tienda,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
        }
    }

    #[tokio::test]
    async fn tool_guardar_y_recordar() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        let ctx = contexto(&tienda, user_id);
        let r = ToolMemoriaGuardar
            .ejecutar(
                &ctx,
                json!({"clave": "color", "contenido": "prefiere el azul"}),
            )
            .await
            .expect("guarda");
        assert!(r.contenido.contains("color"));
        let r = ToolMemoriaRecordar
            .ejecutar(&ctx, json!({"consulta": "qué color prefiere"}))
            .await
            .expect("recuerda");
        assert!(r.contenido.contains("azul"));
    }

    #[tokio::test]
    async fn tool_guardar_rechaza_secretos() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        let ctx = contexto(&tienda, user_id);
        let err = ToolMemoriaGuardar
            .ejecutar(&ctx, json!({"clave": "k", "contenido": "mi api_key = abc"}))
            .await
            .expect_err("el secreto no se guarda");
        assert!(err.to_string().contains("credencial"));
        assert!(tienda.leer(user_id, "k").is_none());
    }

    #[tokio::test]
    async fn tool_borrar_elimina() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        tienda.sembrar(
            user_id,
            vec![MemoriaEntrada::nueva(
                "viejo".into(),
                "dato".into(),
                "t".into(),
            )],
        );
        let ctx = contexto(&tienda, user_id);
        ToolMemoriaBorrar
            .ejecutar(&ctx, json!({"clave": "viejo"}))
            .await
            .expect("borra");
        assert!(tienda.leer(user_id, "viejo").is_none());
    }
}
