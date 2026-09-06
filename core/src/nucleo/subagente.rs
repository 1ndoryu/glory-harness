/* [318A-15 F4] Subagentes (paridad opencode `task` / claurst
 * `agent_tool.rs`): el modelo padre puede delegar trabajo acotado a una
 * sesión hija efímera con su propio system prompt, whitelist de tools y
 * presupuesto de pasos.
 *
 * Aislamiento: la sesión hija NUNCA escribe en la conversación del padre
 * (el bucle hijo no toca persistencia de mensajes) y devuelve solo un
 * resumen acotado (`resumen_acotado`).
 *
 * Profundidad máxima 1: los perfiles no incluyen `task` y `schema_hijo`
 * la excluye explícitamente — el modelo hijo no puede delegar (sin
 * recursión por contrato); el runtime añade un contador fail-closed.
 *
 * Seguridad: cada perfil whitelistea sus tools; ningún perfil incluye
 * ejecución de comandos ni edición de sistema (invariante verificada por
 * test). El hijo hereda la política de permisos F3 del padre porque
 * comparte el mismo registro/overrides. */

use crate::error::{Error, Result};
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU8, Ordering};

/// Perfil de sesión hija: identidad, system prompt, whitelist de tools y
/// presupuesto de pasos. Los perfiles viven en el núcleo (agnósticos); el
/// consumidor puede añadir los suyos con el mismo tipo.
#[derive(Debug, Clone)]
pub struct PerfilSubagente {
    pub id: &'static str,
    pub nombre: &'static str,
    pub instruccion_sistema: &'static str,
    /// Whitelist de tools del hijo. Nunca incluye `task` (sin recursión).
    pub tools: &'static [&'static str],
    /// Presupuesto máximo de pasos del bucle hijo.
    pub presupuesto_pasos: usize,
    /// [318A-16 F3] Tope de riesgo de `comando` permitido al hijo. `None` =
    /// sin comandos (los perfiles que no lo incluyen en `tools` no pueden
    /// llamar la tool); `Some(n)` = solo comandos con riesgo <= n.
    pub comandos_max_riesgo: Option<crate::bash_clasificar::NivelRiesgo>,
}

/// Perfiles agnósticos del núcleo. Ninguno incluye tools de ejecución de
/// comandos ni edición de sistema (invariante de seguridad de F4).
pub(crate) static PERFILES: &[PerfilSubagente] = &[
    PerfilSubagente {
        id: "explorar",
        nombre: "Exploración",
        instruccion_sistema: "Eres un subagente de exploración del asistente Glory. Tu única misión es INVESTIGAR: leer archivos, buscar código y consultar la web. No modificas nada. Reporta hallazgos concretos (rutas, líneas, datos).\n\nDevuelve un resumen conciso (máximo ~200 palabras) con: lo que hiciste, lo que quedó pendiente y el siguiente paso.",
        tools: &["file_read", "file_search", "web_search", "todo"],
        presupuesto_pasos: 8,
        /* [318A-16 F3] Explorar puede ejecutar comandos SOLO seguros (port
         * claurst): verificación con ls/git status, nada de escritura. */
        comandos_max_riesgo: Some(crate::bash_clasificar::NivelRiesgo::Seguro),
    },
    PerfilSubagente {
        id: "planificar",
        nombre: "Planificación",
        instruccion_sistema: "Eres un subagente de planificación del asistente Glory. Descompón el objetivo en pasos verificables usando `todo` y leyendo el contexto necesario. No ejecutas cambios. Entrega el plan ordenado con el criterio de éxito de cada paso.\n\nDevuelve un resumen conciso (máximo ~200 palabras) con: lo que hiciste, lo que quedó pendiente y el siguiente paso.",
        tools: &["file_read", "file_search", "web_search", "todo"],
        presupuesto_pasos: 8,
        comandos_max_riesgo: None,
    },
    PerfilSubagente {
        id: "revisar",
        nombre: "Revisión",
        instruccion_sistema: "Eres un subagente de revisión del asistente Glory. Audita código o texto contra los criterios dados: lee, compara y reporta problemas con ubicación exacta (archivo:línea). No modificas nada.\n\nDevuelve un resumen conciso (máximo ~200 palabras) con: lo que hiciste, lo que quedó pendiente y el siguiente paso.",
        tools: &["file_read", "file_search", "web_search"],
        presupuesto_pasos: 6,
        comandos_max_riesgo: None,
    },
    PerfilSubagente {
        id: "redactar",
        nombre: "Redacción",
        instruccion_sistema: "Eres un subagente de redacción del asistente Glory. Escribe o edita archivos siguiendo la instrucción: usa `file_write` para crear y `file_patch` para cambios localizados; verifica con `file_read`. Solo tocas los archivos indicados en la instrucción.\n\nDevuelve un resumen conciso (máximo ~200 palabras) con: lo que hiciste, lo que quedó pendiente y el siguiente paso.",
        tools: &["file_write", "file_patch", "file_read", "file_search", "todo"],
        presupuesto_pasos: 8,
        comandos_max_riesgo: None,
    },
];

/// Resuelve un perfil por id (`None` si no existe).
#[must_use]
pub(crate) fn perfil_subagente(id: &str) -> Option<PerfilSubagente> {
    PERFILES.iter().find(|p| p.id == id).cloned()
}

/// Lista de ids de perfiles disponibles (para mensajes al modelo).
#[must_use]
pub(crate) fn perfiles_disponibles() -> Vec<String> {
    PERFILES.iter().map(|p| p.id.to_string()).collect()
}

/// Límite (caracteres) del resumen que el hijo devuelve al padre.
pub(crate) const RESUMEN_SUBAGENTE_MAX_CHARS: usize = 4_000;

/// Convierte el resultado estructurado del hijo en el resultado de tool que
/// ve el modelo padre (checklist F4: la única salida al padre es el
/// `resultado` estructurado). Si el hijo se cortó por presupuesto
/// (`parcial=true`), el padre recibe el marcador `[SUBAGENTE PARCIAL …]`
/// para que pueda pedir retomar el trabajo en vez de fingir que acabó.
#[must_use]
pub(crate) fn enmarcar_resultado_para_padre(resultado: ResultadoSubagente) -> AgentToolResult {
    AgentToolResult {
        ok: resultado.ok,
        contenido: if resultado.parcial {
            format!(
                "[SUBAGENTE PARCIAL — presupuesto agotado en {} pasos] {}",
                resultado.pasos_usados, resultado.resumen
            )
        } else {
            resultado.resumen.clone()
        },
        resumen: resultado.resumen,
        diff: None,
        evento_extra: None,
    }
}

/// Acota el resumen del hijo para que no sature la conversación del padre.
#[must_use]
pub(crate) fn resumen_acotado(texto: &str) -> String {
    if texto.chars().count() <= RESUMEN_SUBAGENTE_MAX_CHARS {
        texto.to_string()
    } else {
        let recortado: String = texto.chars().take(RESUMEN_SUBAGENTE_MAX_CHARS).collect();
        format!("{recortado}\n…[resumen truncado por el límite del subagente]")
    }
}

/// Resultado estructurado de la sesión hija — la ÚNICA salida al padre
/// (checklist F4: "la única salida al padre es el resultado estructurado
/// `{ resumen, secciones?, parcial }`"). `parcial=true` cuando el presupuesto
/// de pasos se agotó sin respuesta final del hijo y se cerró con el wrap-up
/// "hecho / pendiente / siguiente paso". `secciones` es opcional en el
/// contrato y el núcleo no las emite (resumen plano); el consumidor puede
/// añadirlas si su UI las necesita.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResultadoSubagente {
    /// `false` si la sesión no se pudo iniciar (tope de concurrentes,
    /// profundidad) o el hijo no produjo resumen.
    pub ok: bool,
    pub resumen: String,
    /// `true` si el presupuesto de pasos se agotó y se cerró con el wrap-up
    /// parcial "hecho / pendiente / siguiente paso" (SSE `SubagenteFin.parcial`).
    pub parcial: bool,
    pub pasos_usados: usize,
}

/// Schema de la sesión hija: whitelist del perfil + deny heredado de la
/// política F3 del padre (mismo registro) + exclusión explícita de `task`
/// (patrón claurst `agent_tool.rs`: sin recursión).
#[must_use]
pub(crate) fn schema_hijo(
    registry: &AgentToolRegistry,
    perfil: &PerfilSubagente,
    modo: &str,
) -> Vec<Value> {
    registry
        .schemas_openai(Some(perfil.tools), modo)
        .into_iter()
        .filter(|s| nombre_de_schema(s) != "task")
        .collect()
}

/// Nombre de la tool dentro de un schema OpenAI (`function.name`).
#[must_use]
pub(crate) fn nombre_de_schema(schema: &Value) -> &str {
    schema["function"]["name"]
        .as_str()
        .or_else(|| schema["name"].as_str())
        .unwrap_or("")
}

/// Profundidad máxima de sesiones hijas (F4): 1 nivel de delegación.
pub(crate) const PROFUNDIDAD_MAX_SUBAGENTES: u8 = 1;

/// Tope de subagentes CONCURRENTES en el proceso (F4, default 2). El
/// contador es global: el daemon corre un runtime por conversación y cada
/// turno ejecuta sus tools en serie, así que un tope por runtime sería
/// vacuo. Saturado → la delegación se rechaza y el padre decide (no cola).
pub(crate) const CONCURRENTES_MAX_SUBAGENTES: u8 = 2;

pub(crate) static SUBAGENTES_EN_CURSO: AtomicU8 = AtomicU8::new(0);

/// Guarda pura del tope de concurrentes (testeable sin runtime/LLM).
#[must_use]
pub(crate) fn concurrencia_permitida(actual: u8) -> bool {
    actual < CONCURRENTES_MAX_SUBAGENTES
}

/// Decrementa el contador global de subagentes al salir de la sesión hija.
pub(crate) struct GuardiaConcurrencia;

impl Drop for GuardiaConcurrencia {
    fn drop(&mut self) {
        SUBAGENTES_EN_CURSO.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Límite absoluto de `max_pasos` por llamada (acotado, no configurable por
/// el modelo: el presupuesto del perfil es el techo razonable).
pub(crate) const MAX_PASOS_TASK_LIMITE: usize = 16;

/// Presupuesto efectivo de la sesión hija: `None` → el default del perfil;
/// si el modelo pide uno, debe estar en 1..=`MAX_PASOS_TASK_LIMITE`.
pub(crate) fn presupuesto_efectivo(base: usize, max_pasos: Option<usize>) -> Result<usize> {
    let Some(max) = max_pasos else {
        return Ok(base);
    };
    if max == 0 || max > MAX_PASOS_TASK_LIMITE {
        return Err(Error::Validacion(format!(
            "task: 'max_pasos' debe estar entre 1 y {MAX_PASOS_TASK_LIMITE}"
        )));
    }
    Ok(max)
}

/// Guarda pura del límite de profundidad (testeable sin runtime/LLM).
#[must_use]
pub(crate) fn profundidad_permitida(actual: u8) -> bool {
    actual < PROFUNDIDAD_MAX_SUBAGENTES
}

/// Decrementa la profundidad de subagentes al salir (fail-closed del
/// contador de `AgentRuntime`).
pub(crate) struct GuardiaProfundidad<'a>(pub(crate) &'a AtomicU8);

impl Drop for GuardiaProfundidad<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Tool `task`: aparece en el schema del padre (sujeta a la política F3,
/// con efecto → ask en modo predeterminado) pero el runtime la intercepta
/// en el bucle y ejecuta la sesión hija. La ejecución directa es un error:
/// la sesión hija solo existe dentro de un turno del runtime.
pub struct ToolTask;

#[async_trait]
impl AgentTool for ToolTask {
    fn id(&self) -> &'static str {
        "task"
    }

    fn descripcion(&self) -> &'static str {
        "Delega una tarea acotada a una sesión hija (subagente) con agente especializado (explorar|planificar|revisar|redactar) y presupuesto de pasos. Devuelve un resumen conciso de lo hecho, lo pendiente y el siguiente paso. Úsala para trabajo independiente que no requiere tu contexto."
    }

    fn schema(&self) -> Value {
        /* Bare parameters object: el registro lo envuelve con nombre y
         * descripción en `schemas_openai` (contrato común del núcleo). */
        json!({
            "type": "object",
            "properties": {
                "agente": {
                    "type": "string",
                    "enum": ["explorar", "planificar", "revisar", "redactar"],
                    "description": "Rol del subagente: explorar (investigar sin modificar), planificar (descomponer en pasos), revisar (auditar), redactar (escribir/editar archivos)."
                },
                "objetivo": {
                    "type": "string",
                    "description": "Tarea concreta y acotada para el subagente."
                },
                "contexto": {
                    "type": "string",
                    "description": "(opcional) Contexto mínimo que el subagente necesita y que no está en el workspace."
                },
                "max_pasos": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 16,
                    "description": "(opcional) Presupuesto máximo de pasos del subagente; por defecto usa el del agente."
                }
            },
            "required": ["agente", "objetivo"]
        })
    }

    fn efecto(&self) -> bool {
        true
    }

    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        _argumentos: Value,
    ) -> Result<AgentToolResult> {
        Err(Error::Validacion(
            "la tool 'task' se ejecuta a través del runtime (sesión hija); no puede invocarse directamente"
                .into(),
        ))
    }
}

/// Registra la tool `task` en el registro (el runtime la intercepta).
pub fn registrar_tool_task(registry: &mut AgentToolRegistry) {
    registry.registrar(Box::new(ToolTask));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permiso::Permiso;

    /// Invariante de seguridad F4: ninguna herramienta de ejecución de
    /// comandos ni edición de sistema entra en los perfiles del dominio.
    const PROHIBIDAS: &[&str] = &[
        "comando",
        "bash",
        "terminal",
        "exec",
        "system",
        "run_cmd",
        "powershell",
        "sudo",
        "shell",
        "proceso",
        "script",
    ];

    /// Stub de tool para tests de schema (el núcleo no expone un constructor
    /// de tools arbitrarias fuera de sus módulos).
    struct StubTool {
        id: &'static str,
        efecto: bool,
    }

    #[async_trait]
    impl AgentTool for StubTool {
        fn id(&self) -> &'static str {
            self.id
        }

        fn descripcion(&self) -> &'static str {
            "stub de test"
        }

        fn schema(&self) -> Value {
            json!({
                "type": "function",
                "function": {
                    "name": self.id,
                    "parameters": { "type": "object", "properties": {} }
                }
            })
        }

        fn efecto(&self) -> bool {
            self.efecto
        }

        async fn ejecutar(
            &self,
            _ctx: &AgentToolContext<'_>,
            _argumentos: Value,
        ) -> Result<AgentToolResult> {
            Ok(AgentToolResult {
                ok: true,
                contenido: "ok".into(),
                resumen: "ok".into(),
                diff: None,
                evento_extra: None,
            })
        }
    }

    #[test]
    fn f4_perfiles_no_incluyen_ejecucion_ni_edicion_de_sistema() {
        for perfil in PERFILES {
            assert!(!perfil.tools.is_empty(), "perfil {} sin tools", perfil.id);
            for id in perfil.tools {
                assert!(
                    !PROHIBIDAS.contains(id),
                    "perfil {} incluye tool prohibida '{}'",
                    perfil.id,
                    id
                );
                assert_ne!(
                    *id, "task",
                    "perfil {} no puede delegar (sin recursión)",
                    perfil.id
                );
            }
        }
    }

    #[test]
    fn f4_schema_hijo_excluye_task_y_aplica_whitelist() {
        let mut registry = AgentToolRegistry::new();
        for id in [
            "file_read",
            "file_write",
            "file_patch",
            "file_search",
            "web_search",
            "todo",
        ] {
            registry.registrar(Box::new(StubTool {
                id,
                efecto: !matches!(id, "file_read" | "file_search" | "web_search"),
            }));
        }
        registrar_tool_task(&mut registry);
        let perfil = perfil_subagente("redactar").expect("perfil redactar existe");
        let schemas = schema_hijo(&registry, &perfil, "predeterminado");
        let nombres: Vec<&str> = schemas.iter().map(nombre_de_schema).collect();
        assert!(
            !nombres.contains(&"task"),
            "task no puede aparecer en el hijo"
        );
        for id in perfil.tools {
            assert!(
                nombres.contains(id),
                "whitelist '{}' ausente del schema hijo",
                id
            );
        }
        assert!(
            !nombres.contains(&"web_search"),
            "web_search no está en la whitelist de redactar"
        );
    }

    #[test]
    fn f6_programar_tarea_excluida_de_subagentes() {
        /* [318A-16 F6] `programar_tarea` solo la usa el agente principal, como
         * la tool `task` (sin recursión de scheduling desde un hijo). Aunque el
         * registry la tenga registrada, ningún perfil de subagente la expone. */
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(StubTool {
            id: "programar_tarea",
            efecto: true,
        }));
        registrar_tool_task(&mut registry);
        for perfil in perfiles_disponibles() {
            let perfil = perfil_subagente(&perfil).expect("perfil existe");
            let schemas = schema_hijo(&registry, &perfil, "predeterminado");
            let nombres: Vec<&str> = schemas.iter().map(nombre_de_schema).collect();
            assert!(
                !nombres.contains(&"programar_tarea"),
                "el perfil '{}' no puede exponer programar_tarea (solo el principal)",
                perfil.id
            );
        }
    }

    #[test]
    fn f4_schema_hijo_respeta_deny_del_padre() {
        let mut registry = AgentToolRegistry::new();
        registry.registrar(Box::new(StubTool {
            id: "file_write",
            efecto: true,
        }));
        registry.registrar(Box::new(StubTool {
            id: "file_read",
            efecto: false,
        }));
        registrar_tool_task(&mut registry);
        registry.establecer_permiso("file_write", Some(Permiso::Deny));
        let perfil = perfil_subagente("redactar").expect("perfil redactar existe");
        let schemas = schema_hijo(&registry, &perfil, "predeterminado");
        let nombres: Vec<&str> = schemas.iter().map(nombre_de_schema).collect();
        assert!(
            !nombres.contains(&"file_write"),
            "deny del padre debe excluir la tool del schema hijo (herencia F3)"
        );
        assert!(nombres.contains(&"file_read"));
    }

    #[test]
    fn f4_profundidad_maxima_uno() {
        assert!(profundidad_permitida(0));
        assert!(!profundidad_permitida(1));
        assert!(!profundidad_permitida(2));
    }

    #[test]
    fn f4_concurrencia_tope_dos() {
        assert!(concurrencia_permitida(0));
        assert!(concurrencia_permitida(1));
        assert!(
            !concurrencia_permitida(2),
            "con 2 sesiones hijas activas la tercera se rechaza"
        );
        assert_eq!(CONCURRENTES_MAX_SUBAGENTES, 2);
    }

    #[test]
    fn f4_presupuesto_efectivo_valida_max_pasos() {
        assert_eq!(presupuesto_efectivo(8, None).expect("default"), 8);
        assert_eq!(presupuesto_efectivo(8, Some(3)).expect("recortado"), 3);
        assert!(
            presupuesto_efectivo(8, Some(0)).is_err(),
            "0 fuera de rango"
        );
        assert!(
            presupuesto_efectivo(8, Some(17)).is_err(),
            "17 supera el límite absoluto"
        );
        let error = presupuesto_efectivo(8, Some(0)).expect_err("error");
        assert!(error.to_string().contains("max_pasos"));
    }

    #[test]
    fn f4_schema_task_usa_vocabulario_del_plan() {
        let mut registry = AgentToolRegistry::new();
        registrar_tool_task(&mut registry);
        let schema = registry
            .schemas_openai(None, "predeterminado")
            .into_iter()
            .find(|s| nombre_de_schema(s) == "task")
            .expect("schema de task presente");
        let props = schema["function"]["parameters"]["properties"]
            .as_object()
            .expect("properties");
        for clave in ["agente", "objetivo", "contexto", "max_pasos"] {
            assert!(props.contains_key(clave), "faltó el parámetro '{clave}'");
        }
        let requeridos = schema["function"]["parameters"]["required"]
            .as_array()
            .expect("required");
        let requeridos: Vec<&str> = requeridos.iter().filter_map(Value::as_str).collect();
        assert_eq!(requeridos, vec!["agente", "objetivo"]);
        assert!(!props.contains_key("perfil"), "vocabulario viejo eliminado");
        assert!(!props.contains_key("instruccion"));
    }

    #[test]
    fn f4_task_en_schema_padre_y_resumen_acotado() {
        let mut registry = AgentToolRegistry::new();
        registrar_tool_task(&mut registry);
        let schemas = registry.schemas_openai(None, "predeterminado");
        assert!(
            schemas.iter().any(|s| nombre_de_schema(s) == "task"),
            "el padre debe ver la tool task en su schema"
        );

        let largo = "x".repeat(5_000);
        let acotado = resumen_acotado(&largo);
        assert!(acotado.chars().count() <= RESUMEN_SUBAGENTE_MAX_CHARS + 50);
        assert!(acotado.contains("truncado"));
        assert_eq!(resumen_acotado("corto"), "corto");
    }

    /* E2E determinista (fixture, sin proveedor): el camino completo que ve el
     * padre — sesión hija → `ResultadoSubagente` → `enmarcar_resultado_para_padre`
     * — produce el `AgentToolResult` con el resumen acotado, y el corte por
     * presupuesto se marca `[SUBAGENTE PARCIAL …]` con el nº de pasos usados
     * (el padre puede pedir retomar; nunca un falso éxito). */
    #[test]
    fn f4_e2e_fixture_resultado_estructurado_hacia_el_padre() {
        let completo = enmarcar_resultado_para_padre(ResultadoSubagente {
            ok: true,
            resumen: "Leí `core/src/subagente.rs`: implementa la tool task…".into(),
            parcial: false,
            pasos_usados: 3,
        });
        assert!(completo.ok);
        assert!(completo.contenido.contains("Leí `core/src/subagente.rs`"));
        assert!(
            !completo.contenido.contains("PARCIAL"),
            "sin corte por presupuesto no hay marcador parcial"
        );
        assert_eq!(completo.diff, None);

        let parcial = enmarcar_resultado_para_padre(ResultadoSubagente {
            ok: true,
            resumen: "Empecé la lectura; falta el módulo `runtime.rs`.".into(),
            parcial: true,
            pasos_usados: 8,
        });
        assert!(parcial
            .contenido
            .contains("[SUBAGENTE PARCIAL — presupuesto agotado en 8 pasos]"));
        assert!(parcial.contenido.contains("falta el módulo"));

        let rechazado = enmarcar_resultado_para_padre(ResultadoSubagente {
            ok: false,
            resumen: "tope de subagentes concurrentes alcanzado (2)".into(),
            parcial: false,
            pasos_usados: 0,
        });
        assert!(!rechazado.ok);
        assert_eq!(rechazado.contenido, rechazado.resumen);
    }
}
