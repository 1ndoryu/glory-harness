/* [29-08-2026] Framework de tools del agente (plan-agente-ia-plugin, Fase 0).
 * OCP: las tools se registran en `AgentToolRegistry`; el runtime solo conoce el
 * trait. El LLM solo ve el JSON Schema; el runtime solo ve `ejecutar`.
 *
 * Portado a Glory Harness (plan 318A-13, Fase 1c): el contexto ya no lleva
 * tipos concretos de task (`PgPool`, `WebSearchService`, `LlmProviderService`)
 * sino puertos del núcleo. Las tools de dominio del consumidor (crear_tarea,
 * crear_habito, ...) reciben sus servicios por `dominio` (slot opaco que el
 * consumidor downcastea); el núcleo nunca lo interpreta (DIP). */

use crate::aprobacion::{PeticionAprobacion, RespuestaAprobacion};
use crate::error::{Error, Result};
use crate::pregunta::PreguntaPendiente;
use crate::permiso::{es_tool_propuesta, permiso_por_modo, resolver_permiso, Permiso};
use crate::ports::{AgentPersistence, McpProveedor, ProviderPort, WebFetchProvider, WebSearchProvider};
use crate::regla::{categorias_core, Clasificador, ReglaPermiso};
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
    /// [Bloque 3, F1] Descarga HTTP de una URL (`web_fetch`). Mismo contrato
    /// que `web_search`: `None` → la tool falla con error claro.
    pub web_fetch: Option<&'a dyn WebFetchProvider>,
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
    /// [318A-16 F5] Store del modo plan: presente SOLO cuando el turno corre
    /// en modo `plan`. Las tools de escritura de archivos registran aquí su
    /// propuesta (diff) en vez de escribir; el resto de consumidores lo
    /// ignoran (`None`).
    pub plan: Option<crate::plan::PlanCompartida>,
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
    /// Id dinámico desde 1c/Bl2: las tools MCP (`mcp_<servidor>_<tool>`) no
    /// pueden devolver un `&'static str`; el registro indexa por `String`.
    fn id(&self) -> &str;
    fn descripcion(&self) -> &str;
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
    tools: HashMap<String, Box<dyn AgentTool>>,
    /// Sandbox compartido (Fase 2). Se fija una vez por runtime; el runtime lo
    /// inyecta en el contexto al ejecutar tools.
    sandbox_archivos: Option<Arc<SandboxArchivos>>,
    /// Store del plan `todo` (318A-15 F5), mismo patrón que el sandbox.
    todo: Option<TodoCompartida>,
    /// [318A-15 F3] Overrides de permiso por conversación: `Arc` compartido
    /// (el runtime se clona el registro y ambos deben ver los mismos
    /// overrides). `None` (eliminado) → vuelve al default del modo.
    overrides: Arc<RwLock<HashMap<String, Permiso>>>,
    /// [318A-16 F1] Reglas v2 por categoría+patrón (ver `regla.rs`): lista
    /// ordenada por inserción, última coincidencia gana. `Arc` compartido por
    /// el mismo motivo que `overrides` (clones del registro en subagentes).
    reglas: Arc<RwLock<Vec<ReglaPermiso>>>,
    /// [318A-16 F1] Clasificadores de las tools del núcleo: de los argumentos
    /// de una llamada a su (categoría derivada, patrón). Las tools del
    /// consumidor sin entrada caen a su id como clave con patrón `*`.
    clasificadores: HashMap<String, Clasificador>,
    /// [318A-16 F1] Categoría estática de cada tool del núcleo (para la clave
    /// de resolución cuando la llamada no lleva argumento clasificable y para
    /// el ocultado de schema por regla deny de categoría).
    categorias: HashMap<String, &'static str>,
    /// [318A-16 F2] Peticiones de aprobación pendientes por `id` (canal
    /// explícito de respuesta). Arc compartido con los clones del registro.
    pendientes: Arc<RwLock<HashMap<String, PeticionAprobacion>>>,
    /// [318A-16 F2] Tokens de "permitir una vez" como (categoría, patrón) de
    /// la clase aprobada: se consumen en la primera llamada cuya clave
    /// coincida (una vez, sin regla persistente).
    una_vez: Arc<RwLock<Vec<(String, String)>>>,
    /// [04-09-2026 B3-F1] Preguntas pendientes de `ask_user` por `id` (canal
    /// explícito, mismo patrón que `pendientes`): la UI las muestra y la
    /// respuesta llega como nuevo mensaje de usuario. Arc compartido con los
    /// clones del registro.
    preguntas: Arc<RwLock<HashMap<String, PreguntaPendiente>>>,
}

impl Default for AgentToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl AgentToolRegistry {
    #[must_use]
    pub fn new() -> Self {
        let mut clasificadores = HashMap::new();
        let mut categorias = HashMap::new();
        /* [318A-16 F1] Tabla estática de clasificación de las tools del
         * núcleo (semántica central, no por-tool para no acoplar cada tool a
         * la política de permisos). */
        for (tool_id, categoria, clasificador) in categorias_core() {
            categorias.insert(tool_id.to_string(), categoria);
            if let Some(cl) = clasificador {
                clasificadores.insert(tool_id.to_string(), cl);
            }
        }
        Self {
            tools: HashMap::new(),
            sandbox_archivos: None,
            todo: None,
            overrides: Arc::new(RwLock::new(HashMap::new())),
            reglas: Arc::new(RwLock::new(Vec::new())),
            clasificadores,
            categorias,
            pendientes: Arc::new(RwLock::new(HashMap::new())),
            una_vez: Arc::new(RwLock::new(Vec::new())),
            preguntas: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn registrar(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.insert(tool.id().to_string(), tool);
    }

    /// [Bloque 3, F2] Registra las tools de un servidor MCP: una tool
    /// `mcp_<servidor>_<herramienta>` por herramienta listada, con la
    /// categoría `mcp` (permisos F3: efecto=true → ask en predeterminado,
    /// deny en meta/plan, allow en autónomo) y su schema declarado por el
    /// servidor. Fail-closed: un error de transporte/lista propaga y el
    /// consumidor decide omitir el servidor; sin proveedor no hay tools MCP.
    pub async fn registrar_mcp(
        &mut self,
        servidor: &str,
        proveedor: Arc<dyn McpProveedor>,
    ) -> Result<()> {
        let herramientas = proveedor.listar_herramientas().await?;
        let prefijo = crate::mcp::sanitizar_id(servidor);
        for herramienta in herramientas {
            let id = format!("mcp_{}_{}", prefijo, crate::mcp::sanitizar_id(&herramienta.nombre));
            self.categorias.insert(id.clone(), crate::regla::CAT_MCP);
            self.tools.insert(
                id.clone(),
                Box::new(crate::mcp::ToolMcpAdapter::nuevo(
                    id,
                    herramienta,
                    Arc::clone(&proveedor),
                )),
            );
        }
        Ok(())
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
    pub fn ids(&self) -> Vec<&str> {
        let mut ids: Vec<&str> = self.tools.keys().map(String::as_str).collect();
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
        let mut schemas: Vec<Value> = self
            .tools
            .iter()
            .filter(|(id, _)| solo_ids.map(|ids| ids.contains(&id.as_str())).unwrap_or(true))
            .filter(|(id, _)| {
                /* deny silencioso: override `deny`, modo meta con efecto o
                 * regla v2 deny con patrón `*` (opencode `visibleTools`) — la
                 * tool no se ofrece (no solo policy). */
                !self.esta_denegada(id, modo)
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

    /* [318A-16 F1] Reglas v2: `establecer_regla` agrega al final (las
     * aprobaciones de la sesión se escriben después de las reglas de
     * configuración → última coincidencia gana). */

    /// Agrega una regla de permiso por categoría+patrón (F1). Se apila al
    /// final de la lista: la última coincidencia decide (findLast opencode).
    pub fn establecer_regla(&self, regla: ReglaPermiso) {
        let mut guard = self.reglas.write().unwrap_or_else(|p| p.into_inner());
        guard.push(regla);
    }

    /// Reglas vigentes (para la UI de F2 y tests deterministas).
    #[must_use]
    pub fn reglas(&self) -> Vec<ReglaPermiso> {
        self.reglas.read().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// [318A-16 F2] Siembra una aprobación de UNA vez (categoría + patrón
    /// exacto derivado) sin petición pendiente. Lo usan los consumidores cuyo
    /// runtime se reconstruye por turno (PT): el "Permitir una vez" se
    /// persiste al final del turno anterior y se reinyecta aquí antes de que
    /// el siguiente stream evalúe la misma llamada.
    pub fn aprobacion_una_vez(&self, categoria: impl Into<String>, patron: impl Into<String>) {
        self.una_vez
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push((categoria.into(), patron.into()));
    }

    /// Claves de resolución de una llamada en orden de especificidad:
    /// 1. (categoría derivada del argumento, patrón concreto) si hay
    ///    clasificador para la tool y el argumento aplica;
    /// 2. (categoría estática de la tool, `*`);
    /// 3. (id de la tool, `*`) — herramientas del consumidor sin clasificar.
    fn claves_para(&self, tool_id: &str, args: &Value) -> Vec<(String, String)> {
        let mut claves: Vec<(String, String)> = Vec::new();
        if let Some(clasificador) = self.clasificadores.get(tool_id) {
            if let Some(derivada) = clasificador(args) {
                claves.push(derivada);
            }
        }
        if let Some(categoria) = self.categorias.get(tool_id) {
            claves.push((categoria.to_string(), "*".to_string()));
        }
        claves.push((tool_id.to_string(), "*".to_string()));
        claves
    }

    /* [318A-15 F3] Permisos por tool con herencia default-del-modo y override
     * por conversación. El override vive en un `Arc` compartido: el runtime
     * clona el registro en `nuevo()` y ambos comparten el mismo mapa, así la
     * conversación puede establecer overrides sin reconstruir el registro. */

    /// Override de permiso de una tool para esta conversación (F3).
    /// `Some(Permiso)` reemplaza al default del modo; `None` lo restaura.
    pub fn establecer_permiso(&self, tool_id: &str, permiso: Option<Permiso>) {
        let mut guard = self.overrides.write().unwrap_or_else(|p| p.into_inner());
        match permiso {
            Some(p) => {
                guard.insert(tool_id.to_string(), p);
            }
            None => {
                guard.remove(tool_id);
            }
        }
    }

    /// Permiso efectivo de una tool para esta conversación: override si
    /// existe; si no, default del modo actual según tenga efecto o no.
    /// (Sin argumentos: cubre F3 y el ocultado de schema por reglas.)
    #[must_use]
    pub fn permiso_para(&self, tool_id: &str, modo: &str) -> Permiso {
        self.permiso_para_llamada(tool_id, &Value::Null, modo)
    }

    /// [318A-16 F1] Permiso efectivo de una LLAMADA concreta: clasifica los
    /// argumentos, evalúa las reglas v2 sobre la clave más específica que
    /// tenga coincidencias y resuelve contra override de conversación y
    /// default del modo (orden en `resolver_permiso`).
    #[must_use]
    pub fn permiso_para_llamada(&self, tool_id: &str, args: &Value, modo: &str) -> Permiso {
        /* [318A-16 F5] Modo plan: las tools de propuesta (escritura de
         * archivos) quedan `allow` para que registren su diff en la store
         * del plan en vez de aplicarlo; el resto de efectos siguen `deny`
         * (semántica de meta, ver `plan.rs`). */
        let default = if modo == "plan" && es_tool_propuesta(tool_id) {
            Permiso::Allow
        } else {
            permiso_por_modo(modo, self.tiene_efecto(tool_id))
        };
        let override_conv = self
            .overrides
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .get(tool_id)
            .copied();
        if override_conv == Some(Permiso::Deny) {
            /* Fail-closed: la denegación de conversación no se abre con
             * reglas (orden 1 de `resolver_permiso`). */
            return Permiso::Deny;
        }
        let claves = self.claves_para(tool_id, args);
        /* [318A-16 F2] Aislamiento de clases: una llamada CLASIFICADA (clave
         * derivada del argumento) se resuelve SOLO contra su clase derivada.
         * Las claves estáticas (categoría de la tool con patrón `*`, id de la
         * tool) son el fallback para llamadas SIN clasificar; evaluarlas aquí
         * filtraría reglas de clase amplias (p. ej. `escritura:**` creada por
         * "permitir siempre") hacia otra clase (escritura_fuera_repo) a través
         * del patrón estático `*` (que `**` coincide). */
        let hay_clave_derivada = self
            .clasificadores
            .get(tool_id)
            .is_some_and(|cl| cl(args).is_some());
        let limite = if hay_clave_derivada { 1 } else { claves.len() };
        /* [318A-16 F2] "Permitir una vez": el token de la clase aprobada se
         * consume en la primera llamada cuya clave coincida y NO vuelve a
         * preguntar en ese mismo turno. */
        {
            let mut tokens = self.una_vez.write().unwrap_or_else(|p| p.into_inner());
            if let Some(pos) = tokens.iter().position(|(cat, pat)| {
                claves[..limite].iter().any(|(c, p)| c == cat && p == pat)
            }) {
                tokens.remove(pos);
                return Permiso::Allow;
            }
        }
        let reglas = self.reglas.read().unwrap_or_else(|p| p.into_inner());
        for (categoria, patron) in claves.into_iter().take(limite) {
            let coincidentes = crate::regla::reglas_coincidentes(&categoria, &patron, &reglas);
            if !coincidentes.is_empty() {
                return resolver_permiso(default, override_conv, &coincidentes);
            }
        }
        resolver_permiso(default, override_conv, &[])
    }

    /// ¿La tool está denegada (`deny`) en esta conversación? El runtime usa
    /// este gate tanto para retirarla del schema como para denegar si llega a
    /// proponerse.
    #[must_use]
    pub fn esta_denegada(&self, tool_id: &str, modo: &str) -> bool {
        self.permiso_para(tool_id, modo) == Permiso::Deny
    }

    /* [318A-16 F2] Canal de aprobación explícito: peticiones con `id` y
     * respuesta de tres vías (Rechazar / Permitir / Permitir siempre). El
     * estado vive aquí (Arc compartido) para que la conversación responda
     * entre turnos sin reconstruir el registro. */

    /// Clasificación presentable de una llamada (la clave más específica F1,
    /// "categoría:patrón" o "tool:*") para el evento y la UI.
    #[must_use]
    pub fn clasificar_llamada(&self, tool_id: &str, args: &Value) -> String {
        match self.claves_para(tool_id, args).first() {
            Some((cat, pat)) => format!("{cat}:{pat}"),
            None => format!("{tool_id}:*"),
        }
    }

    /// Registra una petición de aprobación pendiente (una por tool: la nueva
    /// deja obsoleta la anterior sin responder si el turno siguió adelante).
    pub fn registrar_peticion(&self, peticion: PeticionAprobacion) {
        let mut guard = self.pendientes.write().unwrap_or_else(|p| p.into_inner());
        guard.retain(|_, p| p.tool != peticion.tool);
        guard.insert(peticion.id.clone(), peticion);
    }

    /// Peticiones pendientes sin responder (la UI las muestra mientras
    /// existan; se retiran al responder).
    #[must_use]
    pub fn peticiones_pendientes(&self) -> Vec<PeticionAprobacion> {
        let mut v: Vec<_> = self
            .pendientes
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .cloned()
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Responde una petición pendiente aplicando la decisión y retirándola de
    /// la cola. `Aprobar` deja un token de una vez (clase derivada);
    /// `Siempre`/`Rechazar` crean la regla F1 de esa clase (la última regla
    /// coincide primero: la decisión del usuario manda sobre reglas previas).
    pub fn responder_peticion(
        &self,
        id: &str,
        respuesta: RespuestaAprobacion,
    ) -> std::result::Result<(), String> {
        let peticion = {
            let mut guard = self.pendientes.write().unwrap_or_else(|p| p.into_inner());
            guard
                .remove(id)
                .ok_or_else(|| format!("petición de aprobación desconocida o ya respondida: {id}"))?
        };
        let clave = self
            .claves_para(&peticion.tool, &peticion.argumentos)
            .into_iter()
            .next()
            .unwrap_or_else(|| (peticion.tool.clone(), "*".to_string()));
        match respuesta {
            RespuestaAprobacion::Aprobar => {
                /* Una vez: token de la clase EXACTA (categoría + valor). */
                self.una_vez
                    .write()
                    .unwrap_or_else(|p| p.into_inner())
                    .push(clave);
            }
            RespuestaAprobacion::Siempre | RespuestaAprobacion::Rechazar => {
                /* Siempre/Rechazar = CLASE (categoría derivada, `**`): la
                 * categoría ya separa escritura/escritura_fuera_repo/
                 * lectura_fuera_repo/red/...; `**` cubre cualquier valor de
                 * esa clase sin abrir la tool entera. (Plan: "no exactamente
                 * el mismo comando sino tipos de comando".) */
                if let Some(regla) = respuesta.regla_para(&clave.0, "**") {
                    self.establecer_regla(regla);
                }
            }
        }
        Ok(())
    }

    /* [04-09-2026 B3-F1] Canal de preguntas de `ask_user`: registrar,
     * listar y consumir. La respuesta NO se almacena (llega como nuevo
     * mensaje de usuario); consumir = retirar la pendiente. */

    /// Registra una pregunta pendiente al usuario.
    pub fn registrar_pregunta(&self, pregunta: PreguntaPendiente) {
        self.preguntas
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .insert(pregunta.id.clone(), pregunta);
    }

    /// Preguntas pendientes sin responder (la UI las muestra; se retiran al
    /// recibir la respuesta del usuario en un nuevo turno).
    #[must_use]
    pub fn preguntas_pendientes(&self) -> Vec<PreguntaPendiente> {
        let mut v: Vec<_> = self
            .preguntas
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .values()
            .cloned()
            .collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    /// Consume una pregunta pendiente. `Err` si el id es desconocido o ya fue
    /// respondido.
    pub fn responder_pregunta(&self, id: &str) -> std::result::Result<(), String> {
        let mut guard = self.preguntas.write().unwrap_or_else(|p| p.into_inner());
        if guard.remove(id).is_some() {
            Ok(())
        } else {
            Err(format!("pregunta desconocida o ya respondida: {id}"))
        }
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
        let descripciones: [(&str, &'static str); 7] = [
            ("file_read", crate::tools_archivo::ToolFileRead.descripcion()),
            ("file_write", crate::tools_archivo::ToolFileWrite.descripcion()),
            ("file_patch", crate::tools_archivo::ToolFilePatch.descripcion()),
            ("file_search", crate::tools_archivo::ToolFileSearch.descripcion()),
            ("web_search", crate::tools_web::ToolWebSearch.descripcion()),
            ("todo", crate::todo::ToolTodo.descripcion()),
            ("repo_map", crate::repo_map::ToolRepoMap.descripcion()),
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

    /* [318A-16 F1] Motor de reglas v2 integrado en el registro: fixture
     * determinista (sin LLM) que ejercita clasificación por argumentos,
     * evaluación findLast y ocultado de schema por regla deny. */

    struct StubFileWrite;

    #[async_trait]
    impl AgentTool for StubFileWrite {
        fn id(&self) -> &'static str {
            "file_write"
        }
        fn descripcion(&self) -> &'static str {
            "Escribe un archivo (stub F1)"
        }
        fn schema(&self) -> Value {
            json!({ "type": "object", "properties": { "ruta": {"type": "string"} } })
        }
        fn efecto(&self) -> bool {
            true
        }
        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            _argumentos: Value,
        ) -> Result<AgentToolResult> {
            Ok(AgentToolResult::ok("escrito", "file_write"))
        }
    }

    struct StubWeb;

    #[async_trait]
    impl AgentTool for StubWeb {
        fn id(&self) -> &'static str {
            "web_search"
        }
        fn descripcion(&self) -> &'static str {
            "Busca en la web (stub F1)"
        }
        fn schema(&self) -> Value {
            json!({ "type": "object", "properties": { "query": {"type": "string"} } })
        }
        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            _argumentos: Value,
        ) -> Result<AgentToolResult> {
            Ok(AgentToolResult::ok("resultados", "web_search"))
        }
    }

    fn registry_con_fixture() -> AgentToolRegistry {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(StubFileWrite));
        registry.registrar(Box::new(StubWeb));
        registry
    }

    #[test]
    fn f1_escribir_dentro_permitido_por_regla_fuera_pide_aprobacion() {
        /* Criterio del plan: regla allow por categoría con patrón de
         * escritura dentro del árbol del proyecto (src/) permite escribir
         * AUNQUE el default del modo sea ask, y una escritura FUERA sin
         * regla sigue pidiendo aprobación. Sin LLM: la decisión es pura
         * sobre la llamada. */
        let registry = registry_con_fixture();
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "escritura",
            "src/**",
            Permiso::Allow,
        ));
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "src/main.rs" }),
                "predeterminado"
            ),
            Permiso::Allow,
            "regla allow de categoría gana al ask del modo (criterio F1)"
        );
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "../fuera.txt" }),
                "predeterminado"
            ),
            Permiso::Ask,
            "escritura fuera del workspace sin regla → sigue pidiendo aprobación"
        );
    }

    #[test]
    fn f1_deny_de_escritura_fuera_no_afecta_lecturas_ni_escrituras_dentro() {
        /* Patrón `**`: la clase entera (los valores de rutas fuera llevan
         * separador, y `*` no cruza `/` — glob(7), igual que las referencias). */
        let registry = registry_con_fixture();
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "escritura_fuera_repo",
            "**",
            Permiso::Deny,
        ));
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "../x.txt" }),
                "autonomo"
            ),
            Permiso::Deny,
            "deny fuera gana incluso en modo autonomo (fail-closed)"
        );
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "src/x.txt" }),
                "autonomo"
            ),
            Permiso::Allow,
            "la regla deny de FUERA no bloquea la escritura dentro"
        );
        assert_eq!(
            registry.permiso_para_llamada(
                "file_read",
                &json!({ "ruta": "notas.md" }),
                "autonomo"
            ),
            Permiso::Allow
        );
    }

    #[test]
    fn f1_deny_de_categoria_con_patron_asterisco_oculta_la_tool_del_schema() {
        /* opencode `visibleTools`: una regla deny con patrón `*` retira la
         * tool del schema (no solo policy). */
        let registry = registry_con_fixture();
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "red",
            "*",
            Permiso::Deny,
        ));
        let nombres: Vec<String> = registry
            .schemas_openai(None, "predeterminado")
            .iter()
            .filter_map(|s| s["function"]["name"].as_str().map(String::from))
            .collect();
        assert!(
            !nombres.contains(&"web_search".to_string()),
            "deny red:* oculta web_search del schema: {nombres:?}"
        );
        assert!(
            nombres.contains(&"file_write".to_string()),
            "la deny de red no oculta las tools de archivo"
        );
    }

    #[test]
    fn f1_regla_allow_no_tapa_deny_mas_reciente_para_subconjunto() {
        /* Plan: "regla 'git *' no tapa 'git push' cuando existe una regla
         * deny más específica" — con el wildcard propio sobre la categoría
         * `comando` cuando la tool exista (F3); aquí el mismo principio con
         * rutas: deny más reciente y más estrecha gana a allow genérico. */
        let registry = registry_con_fixture();
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "escritura",
            "**",
            Permiso::Allow,
        ));
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "escritura",
            "**/secretos/**",
            Permiso::Deny,
        ));
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "src/main.rs" }),
                "predeterminado"
            ),
            Permiso::Allow
        );
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "src/secretos/claves.rs" }),
                "predeterminado"
            ),
            Permiso::Deny,
            "la deny más reciente y específica gana (findLast)"
        );
    }

    #[test]
    fn f1_override_deny_de_conversacion_gana_a_regla_allow() {
        let registry = registry_con_fixture();
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            "escritura",
            "**",
            Permiso::Allow,
        ));
        /* El usuario deniega la tool en la conversación (F3): fail-closed,
         * ninguna regla la vuelve a abrir. */
        registry.establecer_permiso("file_write", Some(Permiso::Deny));
        assert_eq!(
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": "src/x.rs" }),
                "predeterminado"
            ),
            Permiso::Deny
        );
    }

    /* [318A-16 F2] Canal de aprobación explícito (Rechazar / Permitir una
     * vez / Permitir siempre): fixture determinista sobre el registro, sin
     * LLM. Las peticiones llevan id; la respuesta aplica token de una vez o
     * regla F1 de la CLASE derivada. */

    fn peticion_file_write(id: &str, ruta: &str) -> PeticionAprobacion {
        let registry = registry_con_fixture();
        PeticionAprobacion::nueva(
            id,
            "file_write",
            json!({ "ruta": ruta }),
            registry.clasificar_llamada("file_write", &json!({ "ruta": ruta })),
        )
    }

    #[test]
    fn f2_aprobar_una_vez_ejecuta_y_consume_el_token() {
        let registry = registry_con_fixture();
        let llamada = |ruta: &str| {
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": ruta }),
                "predeterminado",
            )
        };
        assert_eq!(llamada("src/a.rs"), Permiso::Ask, "base: ask");
        registry.registrar_peticion(peticion_file_write("p1", "src/a.rs"));
        registry
            .responder_peticion("p1", RespuestaAprobacion::Aprobar)
            .expect("responde p1");
        assert_eq!(
            llamada("src/a.rs"),
            Permiso::Allow,
            "permitida una vez: el re-envío del turno ejecuta sin preguntar"
        );
        assert_eq!(
            llamada("src/a.rs"),
            Permiso::Ask,
            "token consumido: la siguiente petición vuelve a preguntar"
        );
    }

    #[test]
    fn f2_siempre_crea_regla_de_clase_que_no_vuelve_a_preguntar() {
        let registry = registry_con_fixture();
        let llamada = |ruta: &str| {
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": ruta }),
                "predeterminado",
            )
        };
        registry.registrar_peticion(peticion_file_write("p2", "src/a.rs"));
        registry
            .responder_peticion("p2", RespuestaAprobacion::Siempre)
            .expect("responde p2");
        /* Misma CLASE (escritura dentro del workspace), distinto valor: ya no
         * pregunta — el "siempre" recuerda el tipo, no el comando exacto. */
        assert_eq!(llamada("src/b.rs"), Permiso::Allow);
        assert_eq!(llamada("src/sub/c.rs"), Permiso::Allow);
        /* La clase fuera del workspace sigue pidiendo aprobación. */
        assert_eq!(llamada("../fuera.txt"), Permiso::Ask);
    }

    #[test]
    fn f2_rechazar_crea_regla_deny_de_clase_y_no_reintenta() {
        let registry = registry_con_fixture();
        let llamada = |ruta: &str| {
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": ruta }),
                "predeterminado",
            )
        };
        registry.registrar_peticion(peticion_file_write("p3", "../fuera.txt"));
        registry
            .responder_peticion("p3", RespuestaAprobacion::Rechazar)
            .expect("responde p3");
        assert_eq!(
            llamada("../otro.txt"),
            Permiso::Deny,
            "la clase escritura_fuera_repo queda denegada (no solo el archivo)"
        );
        assert_eq!(
            llamada("src/a.rs"),
            Permiso::Ask,
            "la deny de fuera no afecta las escrituras dentro"
        );
    }

    #[test]
    fn f2_aprobacion_una_vez_sembrada_se_consume_en_la_primera_llamada_igual() {
        let registry = registry_con_fixture();
        let llamada = |ruta: &str| {
            registry.permiso_para_llamada(
                "file_write",
                &json!({ "ruta": ruta }),
                "predeterminado",
            )
        };
        /* Consumidor con runtime por turno (PT): siembra sin petición. */
        registry.aprobacion_una_vez("escritura", "src/a.rs");
        assert_eq!(llamada("src/a.rs"), Permiso::Allow, "siembra: ejecuta sin preguntar");
        assert_eq!(llamada("src/a.rs"), Permiso::Ask, "token consumido: vuelve a preguntar");
    }

    #[test]
    fn f2_respuesta_a_id_desconocido_es_error() {
        let registry = registry_con_fixture();
        let err = registry
            .responder_peticion("no-existe", RespuestaAprobacion::Siempre)
            .expect_err("id desconocido debe fallar");
        assert!(err.contains("desconocida o ya respondida"), "{err}");
        assert!(registry.peticiones_pendientes().is_empty());
    }

    #[test]
    fn f2_registrar_peticion_de_la_misma_tool_supersede_la_anterior() {
        let registry = registry_con_fixture();
        registry.registrar_peticion(peticion_file_write("p-old", "src/a.rs"));
        registry.registrar_peticion(peticion_file_write("p-nueva", "src/b.rs"));
        let pendientes = registry.peticiones_pendientes();
        assert_eq!(pendientes.len(), 1, "la petición vieja deja de estar pendiente");
        assert_eq!(pendientes[0].id, "p-nueva");
    }
}