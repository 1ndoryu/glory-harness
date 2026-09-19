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
 * cerrar la meta (`AgentRuntime::olvidar_tareas`).
 *
 * [109A-5 F4] El plan gana el BLOQUEO declarado (`bloquear`/`desbloquear`) con
 * su motivo y el contador de turnos cerrados con ese mismo motivo. El contador
 * vive aquí, y no en la meta durable, porque el motivo es estado efímero del
 * runtime: si el plan muere al reiniciar, contar turnos huérfanos no
 * significaría nada. Lo suma el runtime (una vez por turno cerrado), no la
 * tool: dos declaraciones en el mismo turno no deben contar doble. */

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

/// Motivo máximo aceptado en un bloqueo declarado. Acotado porque el motivo
/// viaja al contexto del modelo en cada turno posterior y al aviso de la UI.
pub const MAX_MOTIVO_BLOQUEO: usize = 300;

/// [109A-5 F4] Bloqueo declarado por el agente sobre el plan actual.
///
/// `turnos` es el número de turnos CERRADOS con este mismo motivo vigente; el
/// consumidor pausa la meta al llegar al umbral (decisión del servicio, que es
/// quien tiene el reloj). Repetir el mismo motivo no reinicia el contador: el
/// agente sigue atascado en lo mismo. Un motivo distinto sí lo reinicia.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BloqueoPlan {
    pub motivo: String,
    pub turnos: u32,
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
    bloqueo: Option<BloqueoPlan>,
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

    /// [109A-5 F4] Bloqueo vigente, si el agente declaró que no puede avanzar.
    #[must_use]
    pub fn bloqueo(&self) -> Option<&BloqueoPlan> {
        self.bloqueo.as_ref()
    }

    /// [109A-5 F4] Declara el plan bloqueado con un motivo concreto.
    ///
    /// El motivo vacío se rechaza (un bloqueo sin causa no es información) y el
    /// excesivamente largo también: viaja al contexto de cada turno posterior y
    /// al aviso de la UI.
    pub fn bloquear(&mut self, motivo: &str) -> Result<()> {
        let motivo = motivo.trim();
        if motivo.is_empty() {
            return Err(Error::Argumentos(
                "todo: el motivo del bloqueo no puede estar vacío".into(),
            ));
        }
        if motivo.chars().count() > MAX_MOTIVO_BLOQUEO {
            return Err(Error::Argumentos(format!(
                "todo: el motivo del bloqueo supera los {MAX_MOTIVO_BLOQUEO} caracteres"
            )));
        }
        let turnos = match &self.bloqueo {
            Some(previo) if previo.motivo == motivo => previo.turnos,
            _ => 0,
        };
        self.bloqueo = Some(BloqueoPlan {
            motivo: motivo.to_string(),
            turnos,
        });
        Ok(())
    }

    /// [109A-5 F4] Levanta el bloqueo: el agente ya puede seguir.
    pub fn desbloquear(&mut self) {
        self.bloqueo = None;
    }

    /// [109A-5 F4] Suma un turno CERRADO con el bloqueo vigente. Lo llama el
    /// runtime una sola vez por turno; sin bloqueo declarado no hay nada que
    /// contar (y un turno que avanzó ya lo limpió con `avanza`).
    pub fn contar_turno_bloqueado(&mut self) {
        if let Some(bloqueo) = self.bloqueo.as_mut() {
            bloqueo.turnos = bloqueo.turnos.saturating_add(1);
        }
    }

    /// El plan avanzó (crear/actualizar/en_curso/completar): el bloqueo caduca.
    /// Sin esta limpieza, un agente que se desatasca sin llamar `desbloquear`
    /// seguiría contando turnos bloqueados y la meta se pausaría en falso.
    fn avanza(&mut self) {
        self.bloqueo = None;
    }

    /// Añade un ítem pendiente; devuelve su ID.
    pub fn crear(&mut self, texto: &str) -> usize {
        self.avanza();
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
        self.avanza();
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
        self.avanza();
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
        self.bloqueo = None;
    }

    /// Representación textual del plan para el contexto del modelo.
    ///
    /// [109A-5 F4] El bloqueo se incluye SIEMPRE que esté vigente, también con
    /// la lista vacía: es el dato que explica por qué el agente no avanza, y el
    /// modelo lo recibe en cada turno (no depende de que recuerde haberlo
    /// declarado).
    #[must_use]
    pub fn a_texto(&self) -> String {
        let mut texto = if self.items.is_empty() {
            "Plan actual: vacío (sin ítems).".to_string()
        } else {
            let lineas: Vec<String> = self
                .items
                .iter()
                .map(|item| format!("{}. {} {}", item.id, item.estado.marca(), item.texto))
                .collect();
            format!("Plan actual (tool todo):\n{}", lineas.join("\n"))
        };
        if let Some(bloqueo) = &self.bloqueo {
            texto.push_str(&format!(
                "\nBLOQUEADO (turno {} con este motivo): {}",
                bloqueo.turnos, bloqueo.motivo
            ));
        }
        texto
    }
}

/// Store compartida del plan: una por runtime (o por turno en consumidores que
/// construyen runtime por turno). El runtime atiende a varias conversaciones,
/// así que solo deja cargada aquí la de la conversación ACTIVA y guarda las
/// demás en `AgentRuntime::planes`: el plan nunca es global entre
/// conversaciones (ver `cargar_plan_de`).
pub type TodoCompartida = Arc<Mutex<ListaTodo>>;

/// Aplica una acción
/// `todo { crear|actualizar|en_curso|completar|bloquear|desbloquear }` sobre la
/// lista y devuelve el plan actualizado (se refleja así en el contexto del
/// modelo).
fn aplicar_todo(lista: &mut ListaTodo, argumentos: &Value) -> Result<String> {
    let accion = argumentos
        .get("accion")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            Error::Argumentos(
                "todo: accion requerida (crear|actualizar|en_curso|completar|bloquear|desbloquear)"
                    .into(),
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
        "bloquear" => {
            let motivo = argumentos
                .get("motivo")
                .and_then(Value::as_str)
                .ok_or_else(|| Error::Argumentos("todo: motivo requerido para bloquear".into()))?;
            lista.bloquear(motivo)?;
        }
        "desbloquear" => lista.desbloquear(),
        otra => {
            return Err(Error::Argumentos(format!(
                "todo: accion desconocida '{otra}' \
(crear|actualizar|en_curso|completar|bloquear|desbloquear)"
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
\nQUÉ HACE: crea, actualiza o cambia de estado ítems de un plan de varios pasos, \
y declara el bloqueo del plan cuando no puedes avanzar.\
\nFORMATO DE SALIDA: devuelve el plan completo actualizado, cada línea \
'ID. [ ] texto' pendiente, 'ID. [/] texto' en curso o 'ID. [x] texto' completada.\
\nCUÁNDO USARLA: al recibir una tarea con 2+ pasos o ediciones, crea el plan \
antes de tocar archivos; con una meta activa SIEMPRE (el usuario debe ver el \
plan); marca 'en_curso' el paso que estás haciendo ahora (uno solo a la vez) y \
'completar' cada uno al terminarlo; actualiza el texto si el paso cambia. Para \
tareas de un solo paso sin meta no hace falta.\
\nBLOQUEO: usa 'bloquear' con un motivo CONCRETO solo si no puedes seguir (falta \
un dato, una credencial o un permiso del usuario; una dependencia inaccesible). \
'Difícil', 'no lo entiendo' o 'me falta tiempo' NO son bloqueos: son trabajo \
pendiente, y declararlos es un error. Un bloqueo vigente que dura 3 turnos \
seguidos hace que el backend pause la meta y avise al usuario. En cuanto puedas \
seguir, 'desbloquear' (cualquier avance del plan también lo levanta).\
\nERRORES: accion desconocida, texto o motivo vacíos, motivo demasiado largo o \
id inexistente."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "accion": {
                    "type": "string",
                    "enum": ["crear", "actualizar", "en_curso", "completar", "bloquear", "desbloquear"],
                    "description": "Operación: crear (nuevo paso), actualizar (cambiar texto), en_curso (estoy trabajando en él), completar (marcar hecho), bloquear (no puedo avanzar: exige motivo), desbloquear (ya puedo seguir)"
                },
                "texto": {
                    "type": "string",
                    "description": "Descripción del paso (crear/actualizar)"
                },
                "id": {
                    "type": "integer",
                    "description": "ID del ítem (actualizar/en_curso/completar); se obtiene del plan devuelto"
                },
                "motivo": {
                    "type": "string",
                    "description": "Causa concreta del bloqueo (bloquear): qué dato, permiso o dependencia falta. No vale 'difícil' ni 'incompleto'"
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
        let accion = argumentos
            .get("accion")
            .and_then(Value::as_str)
            .unwrap_or("?");
        let resumen = match lista.bloqueo() {
            Some(bloqueo) if accion == "bloquear" => format!(
                "todo: bloquear — plan bloqueado (turno {} de este motivo)",
                bloqueo.turnos
            ),
            _ => format!("todo: {accion} — {} ítems en el plan", lista.items().len()),
        };
        Ok(AgentToolResult::ok(contenido.clone(), resumen))
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

    #[test]
    fn bloqueo_exige_motivo_concreto_y_acotado() {
        let mut lista = ListaTodo::nueva();
        assert!(lista.bloquear("   ").is_err(), "sin motivo no hay bloqueo");
        assert!(
            lista.bloqueo().is_none(),
            "un rechazo no debe mutar la lista"
        );
        let largo = "x".repeat(MAX_MOTIVO_BLOQUEO + 1);
        assert!(lista.bloquear(&largo).is_err(), "motivo acotado");
        lista
            .bloquear("  falta la API key del proveedor  ")
            .expect("bloquea");
        let bloqueo = lista.bloqueo().expect("vigente");
        assert_eq!(bloqueo.motivo, "falta la API key del proveedor");
        assert_eq!(
            bloqueo.turnos, 0,
            "el turno en curso lo cuenta el runtime al cerrarlo"
        );
    }

    #[test]
    fn bloqueo_cuenta_turnos_seguidos_y_un_motivo_nuevo_reinicia() {
        let mut lista = ListaTodo::nueva();
        lista.bloquear("falta credencial").expect("bloquea");
        lista.contar_turno_bloqueado();
        lista.contar_turno_bloqueado();
        lista.contar_turno_bloqueado();
        assert_eq!(
            lista.bloqueo().expect("vigente").turnos,
            3,
            "tres turnos cerrados con el mismo motivo"
        );
        // Repetir el MISMO motivo no reinicia: sigue atascado en lo mismo.
        lista.bloquear("falta credencial").expect("redeclara");
        assert_eq!(lista.bloqueo().expect("vigente").turnos, 3);
        // Un motivo distinto es un bloqueo nuevo.
        lista.bloquear("permiso denegado").expect("bloquea otro");
        assert_eq!(lista.bloqueo().expect("vigente").turnos, 0);
        // Sin bloqueo declarado, contar no inventa turnos.
        lista.desbloquear();
        lista.contar_turno_bloqueado();
        assert!(lista.bloqueo().is_none());
    }

    #[test]
    fn el_primer_avance_del_plan_levanta_el_bloqueo() {
        let mut lista = ListaTodo::nueva();
        lista.crear("Paso uno");
        lista.bloquear("falta credencial").expect("bloquea");
        lista.contar_turno_bloqueado();
        /* Cualquier avance caduca el bloqueo: sin esto, un agente que se
         * desatasca sin llamar `desbloquear` seguiría sumando turnos y la meta
         * se pausaría en falso. */
        lista.en_curso(1).expect("en curso");
        assert!(lista.bloqueo().is_none());
        lista.bloquear("falta credencial").expect("bloquea");
        lista.completar(1).expect("completa");
        assert!(lista.bloqueo().is_none());
        lista.bloquear("falta credencial").expect("bloquea");
        lista.actualizar(1, "Paso uno bis").expect("actualiza");
        assert!(lista.bloqueo().is_none());
        lista.bloquear("falta credencial").expect("bloquea");
        lista.crear("Paso dos");
        assert!(lista.bloqueo().is_none());
        /* Un id inexistente NO levanta el bloqueo: no hubo avance, hubo error. */
        lista.bloquear("falta credencial").expect("bloquea");
        assert!(lista.en_curso(99).is_err());
        assert!(lista.bloqueo().is_some());
        lista.vaciar();
        assert!(
            lista.bloqueo().is_none(),
            "cerrar la meta vacía el plan y su bloqueo"
        );
    }

    #[test]
    fn el_texto_del_plan_muestra_el_bloqueo_vigente() {
        let mut lista = ListaTodo::nueva();
        lista.bloquear("falta credencial").expect("bloquea");
        lista.contar_turno_bloqueado();
        assert_eq!(
            lista.a_texto(),
            "Plan actual: vacío (sin ítems).\n\
BLOQUEADO (turno 1 con este motivo): falta credencial"
        );
        lista.desbloquear();
        assert_eq!(lista.a_texto(), "Plan actual: vacío (sin ítems).");
    }

    #[tokio::test]
    async fn tool_todo_bloquea_y_desbloquea_con_motivo() {
        let store = Arc::new(Mutex::new(ListaTodo::nueva()));
        let ctx = ctx_con_todo(store.clone());
        let err = ToolTodo
            .ejecutar(&ctx, json!({"accion": "bloquear"}))
            .await
            .expect_err("motivo requerido");
        assert!(err.to_string().contains("motivo requerido"));
        let r = ToolTodo
            .ejecutar(
                &ctx,
                json!({"accion": "bloquear", "motivo": "falta el token del usuario"}),
            )
            .await
            .expect("bloquear");
        assert!(r.contenido.contains("BLOQUEADO"), "contenido: {}", r.contenido);
        assert!(r.resumen.contains("bloqueado"), "resumen: {}", r.resumen);
        assert_eq!(
            store.lock().await.bloqueo().expect("vigente").motivo,
            "falta el token del usuario"
        );
        let r = ToolTodo
            .ejecutar(&ctx, json!({"accion": "desbloquear"}))
            .await
            .expect("desbloquear");
        assert!(!r.contenido.contains("BLOQUEADO"));
        assert!(store.lock().await.bloqueo().is_none());
    }
}
