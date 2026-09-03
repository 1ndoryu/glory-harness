/* [29-08-2026] Framework de tools del agente (plan-agente-ia-plugin, Fase 0).
 * OCP: las tools se registran en `AgentToolRegistry`; el runtime solo conoce el
 * trait. El LLM solo ve el JSON Schema; el runtime solo ve `ejecutar`.
 *
 * Portado a Glory Harness (plan 318A-13, Fase 1c): el contexto ya no lleva
 * tipos concretos de task (`PgPool`, `WebSearchService`, `LlmProviderService`)
 * sino puertos del núcleo. Las tools de dominio del consumidor (crear_tarea,
 * crear_habito, ...) reciben sus servicios por `dominio` (slot opaco que el
 * consumidor downcastea); el núcleo nunca lo interpreta (DIP). */

use crate::error::{Error, Result};
use crate::permiso::{permiso_efectivo, permiso_por_modo, Permiso};
use crate::ports::{AgentPersistence, ProviderPort, WebSearchProvider};
use crate::sandbox::SandboxArchivos;
use crate::todo::TodoCompartida;
use async_trait::async_trait;
use serde_json::Value;
use std::any::Any;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Contexto que recibe cada tool al ejecutarse. El núcleo solo expone puertos
/// (persistencia, búsqueda web, proveedor LLM) y el sandbox de archivos; los
/// servicios de dominio del consumidor viajan en `dominio` (opaco al núcleo).
pub struct AgentToolContext<'a> {
    pub user_id: Uuid,
    /// Puerto de persistencia (turnos, mensajes, memoria, skills, tareas
    /// programadas). El runtime audita las acciones por aquí.
    pub persistencia: &'a dyn AgentPersistence,
    /// Búsqueda web. `None` si el consumidor no aporta proveedor: las tools
    /// que la necesiten fallan con error claro (nunca falso éxito).
    pub web_search: Option<&'a dyn WebSearchProvider>,
    /// Proveedor LLM (para tools que necesiten generar texto). `None` igual.
    pub ai_provider: Option<&'a dyn ProviderPort>,
    /// Sandbox de archivos (Fase 2). `None` en producción: las tools de
    /// archivo no existen (fail-closed, ni siquiera admin).
    pub sandbox_archivos: Option<Arc<SandboxArchivos>>,
    /// Slot de extensión para tools de dominio del consumidor: task inyecta
    /// aquí sus servicios (p. ej. `&PgPool` + repos), y sus tools hacen
    /// `downcast_ref`. El núcleo no interpreta este tipo.
    pub dominio: Option<&'a (dyn Any + Send + Sync)>,
    /// Plan `todo` compartido del runtime (318A-15 F5). `None` si el runtime
    /// no registró la tool (no debería pasar: el runtime la crea siempre).
    pub todo: Option<TodoCompartida>,
}

/// Resultado de ejecutar una tool: texto legible para el LLM + estado.
#[derive(Debug, Clone)]
pub struct AgentToolResult {
    pub ok: bool,
    pub contenido: String,
    /// Resumen corto para auditoría (sin secretos, sin contenido largo).
    pub resumen: String,
    /// [31-08-2026] Fase 4: diff de líneas del cambio (file_write/file_patch)
    /// para mostrarlo en el front; `None` si no aplica.
    pub diff: Option<String>,
}

impl AgentToolResult {
    #[must_use]
    pub fn ok(contenido: impl Into<String>, resumen: impl Into<String>) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff: None,
        }
    }

    /// Resultado ok con diff de líneas (para tools que modifican archivos).
    #[must_use]
    pub fn ok_con_diff(
        contenido: impl Into<String>,
        resumen: impl Into<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff,
        }
    }

    #[must_use]
    pub fn error(contenido: impl Into<String>) -> Self {
        Self {
            ok: false,
            contenido: contenido.into(),
            resumen: "error".to_string(),
            diff: None,
        }
    }
}

/// Contrato de una tool del agente. `schema` es JSON Schema (objeto con
/// `properties`/`required`); el runtime valida los argumentos contra él antes
/// de ejecutar.
#[async_trait]
pub trait AgentTool: Send + Sync {
    fn id(&self) -> &'static str;
    fn descripcion(&self) -> &'static str;
    fn schema(&self) -> Value;
    /// ¿Tiene efectos (escribe/borra)? Las tools con efecto en modo
    /// predeterminado requieren aprobación (diferenciado por la política de
    /// modos, sección 9.2). Por defecto false: la mayoría de las tools de
    /// dominio del v1 son de datos propios y se auditan, no se bloquean.
    fn efecto(&self) -> bool {
        false
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult>;
}

/// Registro de tools: registrar_tool() en el arranque; listar_schemas() para el
/// request al LLM; ejecutar() con validación de schema.
pub struct AgentToolRegistry {
    tools: HashMap<&'static str, Box<dyn AgentTool>>,
    /// Sandbox compartido (Fase 2). Se fija una vez por runtime; el runtime lo
    /// inyecta en el contexto al ejecutar tools.
    sandbox_archivos: Option<Arc<SandboxArchivos>>,
    /// Store del plan `todo` (318A-15 F5), mismo patrón que el sandbox.
    todo: Option<TodoCompartida>,
    /// [318A-15 F3] Overrides de permiso por conversación: `Arc` compartido
    /// (el runtime se clona el registro y ambos deben ver los mismos
    /// overrides). `None` (eliminado) → vuelve al default del modo.
    overrides: Arc<RwLock<HashMap<&'static str, Permiso>>>,
}

impl Default for AgentToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            sandbox_archivos: None,
            todo: None,
            overrides: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn registrar(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.insert(tool.id(), tool);
    }

    /// Fija el sandbox de archivos del runtime (solo AGENTE_MODO=local).
    pub fn registrar_sandbox(&mut self, sandbox: Arc<SandboxArchivos>) {
        self.sandbox_archivos = Some(sandbox);
    }

    #[must_use]
    pub fn sandbox(&self) -> Option<Arc<SandboxArchivos>> {
        self.sandbox_archivos.clone()
    }

    /// Fija la store compartida del plan `todo` del runtime (318A-15 F5).
    pub fn registrar_todo(&mut self, todo: TodoCompartida) {
        self.todo = Some(todo);
    }

    #[must_use]
    pub fn todo(&self) -> Option<TodoCompartida> {
        self.todo.clone()
    }

    #[must_use]
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = self.tools.keys().copied().collect();
        ids.sort_unstable();
        ids
    }

    /// Schemas en formato OpenAI `tools` para el request al LLM. [318A-15 F3]
    /// El `deny` silencioso se aplica AQUÍ: una tool denegada no aparece en el
    /// schema (el modelo no la ve; no solo policy). `solo_ids` filtra el
    /// subconjunto del turno (web/recordatorios apagados); el deny se aplica
    /// después, sobre el conjunto ya filtrado.
    #[must_use]
    pub fn schemas_openai(&self, solo_ids: Option<&[&str]>, modo: &str) -> Vec<Value> {
        let overrides = self
            .overrides
            .read()
            .unwrap_or_else(|p| p.into_inner());
        let mut schemas: Vec<Value> = self
            .tools
            .iter()
            .filter(|(id, _)| solo_ids.map(|ids| ids.contains(id)).unwrap_or(true))
            .filter(|(id, _)| {
                /* deny silencioso: override `deny` o modo meta con efecto. */
                let default = permiso_por_modo(modo, self.tools.get(*id).map(|t| t.efecto()).unwrap_or(false));
                permiso_efectivo(default, overrides.get(*id).copied()) != Permiso::Deny
            })
            .map(|(id, tool)| {
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": id,
                        "description": tool.descripcion(),
                        "parameters": tool.schema(),
                    }
                })
            })
            .collect();
        drop(overrides);
        schemas.sort_by(|a, b| {
            a["function"]["name"]
                .as_str()
                .unwrap_or("")
                .cmp(b["function"]["name"].as_str().unwrap_or(""))
        });
        schemas
    }

    /// ¿La tool tiene efectos (escribe/borra)? Para la política de modos.
    #[must_use]
    pub fn tiene_efecto(&self, tool_id: &str) -> bool {
        self.tools.get(tool_id).map(|t| t.efecto()).unwrap_or(false)
    }

    /* [318A-15 F3] Permisos por tool con herencia default-del-modo y override
     * por conversación. El override vive en un `Arc` compartido: el runtime
     * clona el registro en `nuevo()` y ambos comparten el mismo mapa, así la
     * conversación puede establecer overrides sin reconstruir el registro. */

    /// Override de permiso de una tool para esta conversación (F3).
    /// `Some(Permiso)` reemplaza al default del modo; `None` lo restaura.
    pub fn establecer_permiso(&self, tool_id: &'static str, permiso: Option<Permiso>) {
        let mut guard = self.overrides.write().unwrap_or_else(|p| p.into_inner());
        match permiso {
            Some(p) => {
                guard.insert(tool_id, p);
            }
            None => {
                guard.remove(tool_id);
            }
        }
    }

    /// Permiso efectivo de una tool para esta conversación: override si
    /// existe; si no, default del modo actual según tenga efecto o no.
    #[must_use]
    pub fn permiso_para(&self, tool_id: &str, modo: &str) -> Permiso {
        let override_conv = self
            .overrides
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(tool_id)
            .copied();
        permiso_efectivo(
            permiso_por_modo(modo, self.tiene_efecto(tool_id)),
            override_conv,
        )
    }

    /// ¿La tool está denegada (`deny`) en esta conversación? El runtime usa
    /// este gate tanto para retirarla del schema como para denegar si llega a
    /// proponerse.
    #[must_use]
    pub fn esta_denegada(&self, tool_id: &str, modo: &str) -> bool {
        self.permiso_para(tool_id, modo) == Permiso::Deny
    }

    pub async fn ejecutar(
        &self,
        tool_id: &str,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let tool = self
            .tools
            .get(tool_id)
            .ok_or_else(|| Error::ToolDesconocida(tool_id.to_string()))?;
        validar_contra_schema(tool.schema(), &argumentos)?;
        tool.ejecutar(ctx, argumentos).await
    }
}

/// Validación mínima de JSON Schema (object + properties + required). Suficiente
/// para el contrato declarativo de v1; se puede ampliar sin romper el trait.
fn validar_contra_schema(schema: Value, argumentos: &Value) -> Result<()> {
    if !argumentos.is_object() {
        return Err(Error::Argumentos(
            "Los argumentos de la tool deben ser un objeto JSON".into(),
        ));
    }
    if let Some(requeridos) = schema.get("required").and_then(Value::as_array) {
        for requerido in requeridos {
            if let Some(nombre) = requerido.as_str() {
                if argumentos.get(nombre).is_none() {
                    return Err(Error::Argumentos(format!(
                        "Falta el argumento requerido: {nombre}"
                    )));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /* [318A-15 F5] Contratos ricos: toda tool del núcleo documenta formato de
     * salida, límites y errores. Verifica que las descripciones son
     * multilínea REAL (sin escapes rotos: ningún backslash literal, cada
     * `\n` renderizado) y que write/patch anuncian la regla de cuándo usar
     * cada una. */
    #[test]
    fn descripciones_ricas_sin_escapes_rotos() {
        let descripciones: [(&str, &'static str); 6] = [
            ("file_read", crate::tools_archivo::ToolFileRead.descripcion()),
            ("file_write", crate::tools_archivo::ToolFileWrite.descripcion()),
            ("file_patch", crate::tools_archivo::ToolFilePatch.descripcion()),
            ("file_search", crate::tools_archivo::ToolFileSearch.descripcion()),
            ("web_search", crate::tools_web::ToolWebSearch.descripcion()),
            ("todo", crate::todo::ToolTodo.descripcion()),
        ];
        for (nombre, d) in descripciones {
            assert!(
                !d.contains('\\'),
                "{nombre}: backslash literal = escape roto en la descripción"
            );
            assert!(d.contains('\n'), "{nombre}: descripción debe ser multilínea");
            assert!(
                d.contains("FORMATO DE SALIDA"),
                "{nombre}: documenta el formato de salida"
            );
            assert!(
                d.contains("ERRORES"),
                "{nombre}: documenta los errores esperados"
            );
        }
        let escribir = crate::tools_archivo::ToolFileWrite.descripcion();
        assert!(
            escribir.contains("file_patch"),
            "file_write remite a file_patch para cambios puntuales"
        );
        let parche = crate::tools_archivo::ToolFilePatch.descripcion();
        assert!(
            parche.contains("ÚNICO") && parche.contains("file_write"),
            "file_patch exige old único y remite a file_write si es ambiguo"
        );
    }

    struct ToolEcho;

    #[async_trait]
    impl AgentTool for ToolEcho {
        fn id(&self) -> &'static str {
            "echo"
        }
        fn descripcion(&self) -> &'static str {
            "Devuelve el texto recibido"
        }
        fn schema(&self) -> Value {
            json!({
                "type": "object",
                "properties": {"texto": {"type": "string"}},
                "required": ["texto"]
            })
        }
        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            argumentos: Value,
        ) -> Result<AgentToolResult> {
            let texto = argumentos["texto"].as_str().unwrap_or("").to_string();
            Ok(AgentToolResult::ok(texto.clone(), "echo"))
        }
    }

    #[tokio::test]
    async fn registra_y_lista_schemas() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(ToolEcho));
        assert_eq!(registry.ids(), vec!["echo"]);
        let schemas = registry.schemas_openai(None, "predeterminado");
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0]["function"]["name"], "echo");
    }

    /* [318A-15 F3] Permisos por tool: herencia default→conversación,
     * deny fuera del schema. */
    struct ToolEfecto;

    #[async_trait]
    impl AgentTool for ToolEfecto {
        fn id(&self) -> &'static str {
            "escribir_demo"
        }
        fn descripcion(&self) -> &'static str {
            "Escribe algo (demo con efecto)"
        }
        fn schema(&self) -> Value {
            json!({ "type": "object", "properties": {} })
        }
        fn efecto(&self) -> bool {
            true
        }
        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            _argumentos: Value,
        ) -> Result<AgentToolResult> {
            Ok(AgentToolResult::ok("escrito", "escribir_demo"))
        }
    }

    #[test]
    fn f3_default_del_modo_por_tool_segun_efecto() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(ToolEfecto));
        registry.registrar(Box::new(ToolEcho));
        assert_eq!(
            registry.permiso_para("escribir_demo", "predeterminado"),
            Permiso::Ask,
            "efecto en predeterminado → ask"
        );
        assert_eq!(
            registry.permiso_para("echo", "predeterminado"),
            Permiso::Allow,
            "sin efecto en predeterminado → allow"
        );
        assert_eq!(
            registry.permiso_para("escribir_demo", "meta"),
            Permiso::Deny,
            "efecto en meta → deny"
        );
        assert_eq!(
            registry.permiso_para("escribir_demo", "autonomo"),
            Permiso::Allow,
            "efecto en autonomo → allow"
        );
    }

    #[test]
    fn f3_override_de_conversacion_gana_al_default_y_se_puede_restaurar() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(ToolEfecto));
        /* predeterminado → ask; la conversación lo fuerza a deny. */
        assert_eq!(
            registry.permiso_para("escribir_demo", "predeterminado"),
            Permiso::Ask
        );
        registry.establecer_permiso("escribir_demo", Some(Permiso::Deny));
        assert_eq!(
            registry.permiso_para("escribir_demo", "predeterminado"),
            Permiso::Deny
        );
        /* Restaurar (None) vuelve al default del modo. */
        registry.establecer_permiso("escribir_demo", None);
        assert_eq!(
            registry.permiso_para("escribir_demo", "predeterminado"),
            Permiso::Ask
        );
        /* Allow explícito gana incluso al deny del modo meta. */
        registry.establecer_permiso("escribir_demo", Some(Permiso::Allow));
        assert_eq!(registry.permiso_para("escribir_demo", "meta"), Permiso::Allow);
    }

    #[test]
    fn f3_deny_silencioso_quita_la_tool_del_schema() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(ToolEfecto));
        registry.registrar(Box::new(ToolEcho));
        let nombres = |schemas: &[Value]| -> Vec<String> {
            schemas
                .iter()
                .filter_map(|s| s["function"]["name"].as_str().map(String::from))
                .collect()
        };
        /* En modo meta la tool con efecto no se ofrece (no solo policy). */
        let schemas_meta = registry.schemas_openai(None, "meta");
        assert!(!nombres(&schemas_meta).contains(&"escribir_demo".to_string()));
        assert!(nombres(&schemas_meta).contains(&"echo".to_string()));
        /* En predeterminado sí aparece (ask) pero con deny por override
         * desaparece. */
        assert!(nombres(&registry.schemas_openai(None, "predeterminado"))
            .contains(&"escribir_demo".to_string()));
        registry.establecer_permiso("escribir_demo", Some(Permiso::Deny));
        let schemas = registry.schemas_openai(None, "predeterminado");
        assert!(!nombres(&schemas).contains(&"escribir_demo".to_string()));
        assert!(nombres(&schemas).contains(&"echo".to_string()));
    }

    #[test]
    fn valida_argumentos_requeridos() {
        let schema = json!({
            "type": "object",
            "properties": {"texto": {"type": "string"}},
            "required": ["texto"]
        });
        let err = validar_contra_schema(schema.clone(), &json!({})).unwrap_err();
        assert!(err.to_string().contains("requerido"));
        // Con el argumento presente, pasa.
        assert!(validar_contra_schema(schema, &json!({ "texto": "hola" })).is_ok());
        // No-objeto rechazado.
        assert!(validar_contra_schema(json!({}), &json!([1, 2])).is_err());
    }

}