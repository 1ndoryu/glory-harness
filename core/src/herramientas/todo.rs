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
 * runtime/conversación, no se escribe en BD en v1).
 *
 * [109A-5 F2] El plan gana el estado `en_curso` y una vista serializable
 * (`TareaVisible`) para que el runtime lo emita como evento `TareasActualizadas`
 * y la UI lo pinte en vivo. La lista sigue siendo UNA por runtime, así que
 * sobrevive entre turnos de la misma conversación (resume) y desaparece al
 * cerrar la meta (`AgentRuntime::olvidar_tareas`). */

use crate::contrato::evento::{EstadoTareaVisible, TareaVisible};

use crate::error::{Error, Result};
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::sync::Mutex;

/// Estado de un ítem del plan visible.
///
/// [109A-5 F2] Tres estados (paridad `normalizeRuntimeTaskStatus`: pendiente /
/// en curso / completada). `Pendiente` es el estado inicial de todo ítem nuevo;
/// no hay estado "cancelado" porque un paso descartado se actualiza o se queda
/// en pendiente, sin inventar un cuarto valor que la UI tendría que aprender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EstadoTodo {
    Pendiente,
    EnCurso,
    Completada,
}

impl EstadoTodo {
    /// Marca textual del estado. Contrato visible: la usa `a_texto` (lo que ve
    /// el modelo) y la UI replica las MISMAS marcas, así que un cambio aquí es
    /// un cambio de contrato para los dos.
    #[must_use]
    pub fn marca(&self) -> &'static str {
        match self {
            EstadoTodo::Pendiente => "[ ]",
            EstadoTodo::EnCurso => "[/]",
            EstadoTodo::Completada => "[x]",
        }
    }

    /// ¿El ítem está cerrado? (para validaciones y tests de consumidores).
    #[must_use]
    pub fn completada(&self) -> bool {
        matches!(self, EstadoTodo::Completada)
    }
}

/// Ítem del plan de la tarea.
#[derive(Debug, Clone)]
pub struct ItemTodo {
    /// ID estable dentro de la lista (1-based, asignado al crear).
    pub id: usize,
    pub texto: String,
    pub estado: EstadoTodo,
}

impl ItemTodo {
    /// Vista de contrato del ítem (la que viaja en `TareasActualizadas`).
    #[must_use]
    pub fn visible(&self) -> TareaVisible {
        TareaVisible {
            id: self.id as u32,
            texto: self.texto.clone(),
            estado: match self.estado {
                EstadoTodo::Pendiente => EstadoTareaVisible::Pendiente,
                EstadoTodo::EnCurso => EstadoTareaVisible::EnCurso,
                EstadoTodo::Completada => EstadoTareaVisible::Completada,
            },
        }
    }
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
            estado: EstadoTodo::Pendiente,
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
        self.mutar_estado(id, EstadoTodo::Completada)
    }

    /// Marca un ítem como EN CURSO y devuelve a pendiente cualquier otro ítem
    /// que estuviera en curso.
    ///
    /// Invariante: como mucho un ítem en curso a la vez. Sin él, la UI puede
    /// mostrar dos pasos "en curso" simultáneos y el plan deja de ser un plan
    /// (paridad con la task-list de Synara, donde el estado es único).
    pub fn en_curso(&mut self, id: usize) -> Result<()> {
        self.mutar_estado(id, EstadoTodo::EnCurso)?;
        for item in &mut self.items {
            if item.id != id && item.estado == EstadoTodo::EnCurso {
                item.estado = EstadoTodo::Pendiente;
            }
        }
        Ok(())
    }

    /// Aplica un estado a un ítem existente (una sola búsqueda y un solo
    /// mensaje de error para las tres transiciones).
    fn mutar_estado(&mut self, id: usize, estado: EstadoTodo) -> Result<()> {
        let item = self
            .items
            .iter_mut()
            .find(|item| item.id == id)
            .ok_or_else(|| Error::NoEncontrado(format!("todo: no existe el ítem {id}")))?;
        item.estado = estado;
        Ok(())
    }

    /// ¿La lista está vacía? (el runtime la usa para decidir si emitir el plan
    /// visible al arrancar un turno).
    #[must_use]
    pub fn vacia(&self) -> bool {
        self.items.is_empty()
    }

    /// Vista de contrato de la lista completa (evento `TareasActualizadas`).
    #[must_use]
    pub fn visibles(&self) -> Vec<TareaVisible> {
        self.items.iter().map(ItemTodo::visible).collect()
    }

    /// Vacía la lista (la meta se cerró: el plan ya no persigue nada).
    pub fn vaciar(&mut self) {
        self.items.clear();
    }

    /// Representación textual del plan para el contexto del modelo.
    #[must_use]
    pub fn a_texto(&self) -> String {
        if self.items.is_empty() {
            return "Plan actual: vacío (sin ítems).".to_string();
        }
        let mut lineas: Vec<String> = Vec::with_capacity(self.items.len());
        for item in &self.items {
            lineas.push(format!(
                "{}. {} {}",
                item.id,
                item.estado.marca(),
                item.texto
            ));
        }
        format!("Plan actual (tool todo):\n{}", lineas.join("\n"))
    }
}

/// Store compartida del plan: una por runtime (o por turno en consumidores que
/// construyen runtime por turno). El runtime atiende a varias conversaciones,
/// así que solo deja cargada aquí la de la conversación ACTIVA y guarda las
/// demás en `AgentRuntime::planes`: el plan nunca es global entre
/// conversaciones (ver `cargar_plan_de`).
pub type TodoCompartida = Arc<Mutex<ListaTodo>>;

/// Aplica una acción `todo { crear|actualizar|en_curso|completar }` sobre la
/// lista y devuelve el plan actualizado (se refleja así en el contexto del
/// modelo).
fn aplicar_todo(lista: &mut ListaTodo, argumentos: &Value) -> Result<String> {
    let accion = argumentos
        .get("accion")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Argumentos(
                "todo: accion requerida (crear|actualizar|en_curso|completar)".into(),
            )
        })?;
    match accion {
        "crear" => {
            let texto = argumentos
                .get("texto")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Argumentos("todo: texto requerido para crear".into()))?;
            if texto.trim().is_empty() {
                return Err(Error::Argumentos(
                    "todo: el texto no puede estar vacío".into(),
                ));
            }
            lista.crear(texto);
        }
        "actualizar" => {
            let id = argumentos
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Argumentos("todo: id requerido para actualizar".into()))?
                as usize;
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
                .ok_or_else(|| Error::Argumentos("todo: id requerido para completar".into()))?
                as usize;
            lista.completar(id)?;
        }
        "en_curso" => {
            let id = argumentos
                .get("id")
                .and_then(Value::as_u64)
                .ok_or_else(|| Error::Argumentos("todo: id requerido para en_curso".into()))?
                as usize;
            lista.en_curso(id)?;
        }
        otra => {
            return Err(Error::Argumentos(format!(
                "todo: accion desconocida '{otra}' (crear|actualizar|en_curso|completar)"
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
\nQUÉ HACE: crea, actualiza o cambia de estado ítems de un plan de varios pasos.\
\nFORMATO DE SALIDA: devuelve el plan completo actualizado, cada línea \
'ID. [ ] texto' pendiente, 'ID. [/] texto' en curso o 'ID. [x] texto' completada.\
\nCUÁNDO USARLA: al recibir una tarea con 2+ pasos o ediciones, crea el plan \
antes de tocar archivos; con una meta activa SIEMPRE (el usuario debe ver el \
plan); marca 'en_curso' el paso que estás haciendo ahora (uno solo a la vez) y \
'completar' cada uno al terminarlo; actualiza el texto si el paso cambia. Para \
tareas de un solo paso sin meta no hace falta.\
\nERRORES: accion desconocida, texto vacío o id inexistente."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "accion": {
                    "type": "string",
                    "enum": ["crear", "actualizar", "en_curso", "completar"],
                    "description": "Operación: crear (nuevo paso), actualizar (cambiar texto), en_curso (estoy trabajando en él), completar (marcar hecho)"
                },
                "texto": {
                    "type": "string",
                    "description": "Descripción del paso (crear/actualizar)"
                },
                "id": {
                    "type": "integer",
                    "description": "ID del ítem (actualizar/en_curso/completar); se obtiene del plan devuelto"
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
        let store = ctx
            .todo
            .clone()
            .ok_or_else(|| Error::Validacion("todo no está disponible en este runtime".into()))?;
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
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: Uuid::new_v4(),
            persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: Some(store),
            plan: None,
            navegador: None,
        }
    }

    #[test]
    fn lista_crea_actualiza_completa_y_serializa() {
        let mut lista = ListaTodo::nueva();
        assert_eq!(lista.a_texto(), "Plan actual: vacío (sin ítems).");
        let id = lista.crear("Arreglar el botón");
        assert_eq!(id, 1);
        assert_eq!(lista.crear("Probar en el preview"), 2);
        lista
            .actualizar(1, "Arreglar el botón de guardar")
            .expect("actualiza");
        assert!(lista.actualizar(99, "x").is_err(), "id inexistente falla");
        lista.completar(2).expect("completa");
        assert!(lista.completar(99).is_err());
        let texto = lista.a_texto();
        assert!(texto.contains("1. [ ] Arreglar el botón de guardar"));
        assert!(texto.contains("2. [x] Probar en el preview"));
    }

    #[test]
    fn en_curso_es_unico_y_las_marcas_lo_reflejan() {
        let mut lista = ListaTodo::nueva();
        lista.crear("Primero");
        lista.crear("Segundo");
        lista.en_curso(1).expect("en curso");
        assert!(lista.a_texto().contains("1. [/] Primero"));
        // Solo un ítem en curso: marcar el segundo devuelve el primero a [ ].
        lista.en_curso(2).expect("en curso");
        let texto = lista.a_texto();
        assert!(texto.contains("1. [ ] Primero"), "texto: {texto}");
        assert!(texto.contains("2. [/] Segundo"), "texto: {texto}");
        assert!(lista.en_curso(99).is_err(), "id inexistente falla");
    }

    #[test]
    fn visibles_expone_estado_serializable_y_vaciar_limpia() {
        let mut lista = ListaTodo::nueva();
        lista.crear("Uno");
        lista.en_curso(1).expect("en curso");
        let visibles = lista.visibles();
        assert_eq!(visibles.len(), 1);
        assert_eq!(visibles[0].id, 1);
        assert_eq!(visibles[0].estado, EstadoTareaVisible::EnCurso);
        lista.completar(1).expect("completa");
        assert_eq!(lista.visibles()[0].estado, EstadoTareaVisible::Completada);
        assert!(!lista.vacia());
        lista.vaciar();
        assert!(lista.vacia());
        assert_eq!(lista.a_texto(), "Plan actual: vacío (sin ítems).");
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
        assert!(
            r1.contenido.contains("[ ] Paso uno"),
            "el plan se refleja en el resultado: {}",
            r1.contenido
        );
        let r2 = ToolTodo
            .ejecutar(&ctx, json!({"accion": "completar", "id": 1}))
            .await
            .expect("completar");
        assert!(r2.contenido.contains("[x] Paso uno"));
        let lista = store.lock().await;
        assert!(lista.items()[0].estado.completada());
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
        assert!(
            !registry.tiene_efecto("todo"),
            "todo no requiere aprobación"
        );
        assert!(registry.todo().is_some(), "la store queda registrada");
        let schemas = registry.schemas_openai(None, "predeterminado");
        assert!(schemas.iter().any(|s| s["function"]["name"] == "todo"));
    }
}
