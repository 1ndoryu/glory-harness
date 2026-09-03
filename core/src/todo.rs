/* [03-09-2026] Tool `todo` del núcleo (plan 318A-15, F5): plan visible de la
 * tarea para turnos de varios pasos (paridad opencode `todo`).
 *
 * Estado: `ListaTodo` vive en una store compartida por runtime
 * (`TodoCompartida = Arc<tokio::sync::Mutex<ListaTodo>>`) que el registro
 * guarda y el runtime inyecta en el contexto de cada ejecución (mismo patrón
 * que el sandbox de archivos). El resultado de cada acción devuelve el plan
 * actualizado completo, así el siguiente turno del loop lo ve en contexto.
 *
 * Agnóstica al consumidor: no toca persistencia (el plan es efímero del
 * runtime/conversación, no se escribe en BD en v1). */

use crate::error::{Error, Result};
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult, AgentToolRegistry};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Ítem del plan de la tarea.
#[derive(Debug, Clone)]
pub struct ItemTodo {
    /// ID estable dentro de la lista (1-based, asignado al crear).
    pub id: usize,
    pub texto: String,
    pub completado: bool,
}

/// Lista ordenada de ítems del plan (estado efímero del runtime).
#[derive(Debug, Clone, Default)]
pub struct ListaTodo {
    items: Vec<ItemTodo>,
}

impl ListaTodo {
    #[must_use]
    pub fn nueva() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn items(&self) -> &[ItemTodo] {
        &self.items
    }

    /// Añade un ítem pendiente; devuelve su ID.
    pub fn crear(&mut self, texto: &str) -> usize {
        let texto = texto.trim();
        let id = self.items.len() + 1;
        self.items.push(ItemTodo {
            id,
            texto: texto.to_string(),
            completado: false,
        });
        id
    }

    /// Reemplaza el texto de un ítem existente.
    pub fn actualizar(&mut self, id: usize, texto: &str) -> Result<()> {
        let texto = texto.trim();
        if texto.is_empty() {
            return Err(Error::Argumentos(
                "todo: el texto nuevo no puede estar vacío".into(),
            ));
        }
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::NoEncontrado(format!("todo: no existe el ítem {id}")))?;
        item.texto = texto.to_string();
        Ok(())
    }

    /// Marca un ítem como completado.
    pub fn completar(&mut self, id: usize) -> Result<()> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::NoEncontrado(format!("todo: no existe el ítem {id}")))?;
        item.completado = true;
        Ok(())
    }

    /// Representación textual del plan para el contexto del modelo.
    #[must_use]
    pub fn a_texto(&self) -> String {
        if self.items.is_empty() {
            return "Plan actual: vacío (sin ítems).".to_string();
        }
        let mut lineas: Vec<String> = Vec::with_capacity(self.items.len());
        for item in &self.items {
            let marca = if item.completado { "[x]" } else { "[ ]" };
            lineas.push(format!("{}. {marca} {}", item.id, item.texto));
        }
        format!("Plan actual (tool todo):\n{}", lineas.join("\n"))
    }
}

/// Store compartida del plan: una por runtime (o por turno en consumidores que
/// construyen runtime por turno), nunca global entre conversaciones.
pub type TodoCompartida = Arc<Mutex<ListaTodo>>;

/// Aplica una acción `todo { crear|actualizar|completar }` sobre la lista y
/// devuelve el plan actualizado (se refleja así en el contexto del modelo).
fn aplicar_todo(lista: &mut ListaTodo, argumentos: &Value) -> Result<String> {
    let accion = argumentos
        .get("accion")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Argumentos("todo: accion requerida (crear|actualizar|completar)".into()))?;
    match accion {
        "crear" => {
            let texto = argumentos
                .get("texto")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Argumentos("todo: texto requerido para crear".into()))?;
            if texto.trim().is_empty() {
                return Err(Error::Argumentos("todo: el texto no puede estar vacío".into()));
            }
            lista.crear(texto);
        }
        "actualizar" => {
            let id = argumentos
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Argumentos("todo: id requerido para actualizar".into()))? as usize;
            let texto = argumentos
                .get("texto")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Argumentos("todo: texto requerido para actualizar".into()))?;
            lista.actualizar(id, texto)?;
        }
        "completar" => {
            let id = argumentos
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Argumentos("todo: id requerido para completar".into()))? as usize;
            lista.completar(id)?;
        }
        otra => {
            return Err(Error::Argumentos(format!(
                "todo: accion desconocida '{otra}' (crear|actualizar|completar)"
            )));
        }
    }
    Ok(lista.a_texto())
}

pub struct ToolTodo;

#[async_trait]
impl AgentTool for ToolTodo {
    fn id(&self) -> &'static str {
        "todo"
    }
    fn descripcion(&self) -> &'static str {
        "Mantiene el plan visible de la tarea actual (lista de pasos).\
\nQUÉ HACE: crea, actualiza o completa ítems de un plan de varios pasos.\
\nFORMATO DE SALIDA: devuelve el plan completo actualizado, cada línea \
'ID. [ ] texto' o 'ID. [x] texto'.\
\nCUÁNDO USARLA: al recibir una tarea con 2+ pasos o ediciones, crea el plan \
antes de tocar archivos; completa cada ítem al terminarlo; actualiza el texto \
si el paso cambia. Para tareas de un solo paso no hace falta.\
\nERRORES: accion desconocida, texto vacío o id inexistente."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "accion": {
                    "type": "string",
                    "enum": ["crear", "actualizar", "completar"],
                    "description": "Operación: crear (nuevo paso), actualizar (cambiar texto), completar (marcar hecho)"
                },
                "texto": {
                    "type": "string",
                    "description": "Descripción del paso (crear/actualizar)"
                },
                "id": {
                    "type": "integer",
                    "description": "ID del ítem (actualizar/completar); se obtiene del plan devuelto"
                }
            },
            "required": ["accion"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let store = ctx.todo.clone().ok_or_else(|| {
            Error::Validacion("todo no está disponible en este runtime".into())
        })?;
        let mut lista = store.lock().await;
        let contenido = aplicar_todo(&mut lista, &argumentos)?;
        let resumen = argumentos
            .get("accion")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string();
        Ok(AgentToolResult::ok(
            contenido.clone(),
            format!("todo: {resumen} — {} ítems en el plan", lista.items().len()),
        ))
    }
}

/// Registra la store compartida + la tool `todo` en el registry. Siempre
/// disponible (agnóstica): el runtime la llama al construirse.
pub fn registrar_tool_todo(registry: &mut AgentToolRegistry) {
    registry.registrar_todo(Arc::new(Mutex::new(ListaTodo::nueva())));
    registry.registrar(Box::new(ToolTodo));
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn ctx_con_todo(store: TodoCompartida) -> AgentToolContext<'static> {
        let persistencia: &'static crate::contrato_tests::PersistenciaMock =
            Box::leak(Box::new(crate::contrato_tests::PersistenciaMock::default()));
        AgentToolContext {
            user_id: Uuid::new_v4(),
            persistencia,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: Some(store),
        }
    }

    #[test]
    fn lista_crea_actualiza_completa_y_serializa() {
        let mut lista = ListaTodo::nueva();
        assert_eq!(lista.a_texto(), "Plan actual: vacío (sin ítems).");
        let id = lista.crear("Arreglar el botón");
        assert_eq!(id, 1);
        assert_eq!(lista.crear("Probar en el preview"), 2);
        lista.actualizar(1, "Arreglar el botón de guardar").expect("actualiza");
        assert!(lista.actualizar(99, "x").is_err(), "id inexistente falla");
        lista.completar(2).expect("completa");
        assert!(lista.completar(99).is_err());
        let texto = lista.a_texto();
        assert!(texto.contains("1. [ ] Arreglar el botón de guardar"));
        assert!(texto.contains("2. [x] Probar en el preview"));
    }

    #[tokio::test]
    async fn tool_todo_muta_la_lista_y_devuelve_el_plan() {
        let store = Arc::new(Mutex::new(ListaTodo::nueva()));
        let ctx = ctx_con_todo(store.clone());
        let r1 = ToolTodo
            .ejecutar(&ctx, json!({"accion": "crear", "texto": "Paso uno"}))
            .await
            .expect("crear");
        assert!(r1.ok);
        assert!(r1.contenido.contains("[ ] Paso uno"), "el plan se refleja en el resultado: {}", r1.contenido);
        let r2 = ToolTodo
            .ejecutar(&ctx, json!({"accion": "completar", "id": 1}))
            .await
            .expect("completar");
        assert!(r2.contenido.contains("[x] Paso uno"));
        let lista = store.lock().await;
        assert!(lista.items()[0].completado);
    }

    #[tokio::test]
    async fn tool_todo_rechaza_accion_mala_y_texto_vacio() {
        let store = Arc::new(Mutex::new(ListaTodo::nueva()));
        let ctx = ctx_con_todo(store);
        let err = ToolTodo
            .ejecutar(&ctx, json!({"accion": "borrar"}))
            .await
            .expect_err("acción desconocida");
        assert!(err.to_string().contains("desconocida"));
        let err = ToolTodo
            .ejecutar(&ctx, json!({"accion": "crear", "texto": "  "}))
            .await
            .expect_err("texto vacío");
        assert!(err.to_string().contains("vacío"));
    }

    #[tokio::test]
    async fn registro_expone_todo_sin_efecto() {
        let mut registry = AgentToolRegistry::new();
        registrar_tool_todo(&mut registry);
        assert!(registry.ids().contains(&"todo"));
        assert!(!registry.tiene_efecto("todo"), "todo no requiere aprobación");
        assert!(registry.todo().is_some(), "la store queda registrada");
        let schemas = registry.schemas_openai(None);
        assert!(schemas.iter().any(|s| s["function"]["name"] == "todo"));
    }
}
