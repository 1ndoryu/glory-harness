//! Runtime del agente (plan 318A-13, Fase 1c): port agnóstico de
//! `src/agent/runtime.rs` de task **sin SQL y sin `AppState`**. Todo acceso a
//! estado durable entra por [`AgentPersistence`]; el proveedor LLM es
//! [`LlmProviderService`] (movido al núcleo en Fase 1b).
//!
//! Frontera heredada de task (H2/H3): loop LLM → tools → LLM con límite de
//! turns (configurable), timeout por tool, fallo parcial como resultado de
//! tool (no aborta el turno) y cancelación real cuando el cliente corta el
//! SSE (`tx.is_closed()` → no se siguen ejecutando tools ni se consumen
//! tokens). El contexto de productividad (notas/tareas/hábitos) y la memoria/
//! skills NO se cargan aquí: el consumidor los inyecta en `historial` antes
//! de llamar (son consultas de su dominio; R3: el núcleo nunca persiste por
//! su cuenta).

use std::sync::Arc;
use uuid::Uuid;

use std::collections::HashMap;

use crate::context::AgentContextManager;
use crate::guardas::GuardasTurno;
use crate::hooks::DispatcherHooks;
use crate::telemetria::TelemetriaTurno;
use crate::tool::AgentToolRegistry;

/* [059A-N S2] Split estructural de runtime.rs: el bucle del turno principal
 * vive en `turno.rs`, la capa de llamada LLM/ejecución de tools en
 * `tools.rs` y las sesiones hijas en `subagente.rs`. Movimiento puro: cada
 * archivo abre su propio `impl AgentRuntime` (los campos privados viven en
 * este módulo; los hijos los ven por privacidad de módulo). */
mod subagente;
mod tools;
/* [129A-2] `tools.rs` aloja `cierre_wrap_up` (mudado desde `turno/mod.rs` para
 * devolverlo bajo el tope de 500 líneas efectivas): necesita nombrar
 * `turno::EstadoTurno`, así que el módulo pasa a `pub(crate)`. */
pub(crate) mod turno;
/// [109A-4 F4] Petición de turno con política forzada opcional: el transporte
/// la construye para `/meta <texto>` sin tocar el modo de la sesión.
pub use turno::PeticionTurno;
/* [139A-8 F4/S2] Segundo split: lo que quedaba en este `mod.rs` (config,
 * puertos, construcción, modos, telemetría y ciclo) sale a cuatro módulos
 * propios con movimiento puro (mismos cuerpos, misma visibilidad pública;
 * solo sube a `pub(crate)` lo que ya usaban los hermanos `subagente`,
 * `tools` y `turno/`). */
mod ciclo;
mod construccion;
mod modos;
mod telemetria;
/* [139A-8 F4/S2] `TurnoConfig`, `PuertosHarness` y `CompactarManual` siguen
 * resolviendo como `runtime::TurnoConfig`, `runtime::PuertosHarness` y
 * `runtime::CompactarManual` (los usa el cli y `crate::runtime::*`): la ruta
 * pública no cambia. */
pub use construccion::{PuertosHarness, TurnoConfig};
pub use telemetria::CompactarManual;
/* [139A-8 F4/S2] Los hermanos (`subagente`, `tools`, `turno/`) consumen estas
 * ayudas vía `super::*`: se re-exportan para no tocar esos archivos en este
 * split (movimiento puro). */
pub(crate) use ciclo::{mensajes_usuario_resumen, wrap_up_instruccion};
/* [20-09-2026] `LlamadaLlm` (parámetros de `llm_llamada`): los hermanos
 * `subagente` y `turno/` la nombran vía `super::*` sin conocer `tools`. */
pub(crate) use tools::LlamadaLlm;

/// [109A-5 F2] Reparto del plan visible entre conversaciones.
///
/// La store de la tool `todo` vive en el REGISTRY del runtime
/// (`Arc<Mutex<ListaTodo>>`), y un runtime atiende a varias conversaciones a lo
/// largo de su vida (el escritorio reusa la sesión al cambiar de hilo). Por eso
/// el runtime guarda aquí la lista de cada conversación y solo deja cargada en
/// el registry la de la conversación ACTIVA: sin este reparto, cambiar de hilo
/// mostraría el plan del anterior y el modelo de la conversación nueva podría
/// seguir editando tareas ajenas (el plan es de la conversación, no del
/// proceso).
#[derive(Default)]
struct PlanesConversacion {
    /// Conversación cuya lista está ahora mismo cargada en el registry.
    activa: Option<Uuid>,
    /// Listas de las demás conversaciones atendidas por este runtime.
    listas: HashMap<Uuid, crate::todo::ListaTodo>,
}

pub struct AgentRuntime {
    pub registry: AgentToolRegistry,
    pub contexto: Arc<tokio::sync::Mutex<AgentContextManager>>,
    pub turno_config: TurnoConfig,
    /// [059A-21 M3] Puertos del consumidor agrupados: la misma struct que
    /// recibe `nuevo` (una sola fuente; antes 5 campos sueltos la duplicaban).
    puertos: PuertosHarness,
    /// [318A-15 F4] Profundidad de sesiones hijas activas (máx 1). El schema
    /// del hijo excluye `task` (sin recursión por contrato); el contador es
    /// fail-closed para llamadas directas.
    profundidad_subagente: std::sync::atomic::AtomicU8,
    /// [318A-15 F0] Acumulador de telemetría del turno (interior-mutable;
    /// reseteado al emitir `Telemetria` justo antes de `Done`).
    telemetria: std::sync::Mutex<TelemetriaTurno>,
    /// [318A-15 F2] Reglas del consumidor (AGENTS.md / skills) inyectadas en
    /// la ranura `[REGLAS]`. Interior-mutable: el CLI la fija tras construir
    /// el runtime; vacía por defecto (ranura nunca huérfana).
    reglas: std::sync::Mutex<String>,
    /// [318A-15 F6] ¿Una tool está en curso? La compactación se omite durante
    /// tool_calls largos (ventana de seguridad configurable, item 4).
    tool_en_curso: std::sync::atomic::AtomicBool,
    /// [318A-16 F5] Store del modo plan del turno en curso. `Some` solo tras
    /// empezar un turno con `modo == "plan"`; `None` en el resto. Vive en el
    /// runtime (efímera, nunca en BD): el consumidor la lee tras el turno
    /// para mostrar el diff acumulado (CLI) o descartarla.
    plan_actual: std::sync::Mutex<Option<crate::plan::PlanCompartida>>,
    /// [109A-5 F2] Plan visible repartido por conversación (ver
    /// [`PlanesConversacion`]). Efímero y acotado al uso de la sesión.
    planes: std::sync::Mutex<PlanesConversacion>,
    /// [109A-4 F4] Modo FORZADO para el turno en curso (`/meta <texto>`): un
    /// turno solo-lectura sin tocar el modo de la sesión. `None` = usar
    /// `turno_config.modo`. Interior-mutable porque `ejecutar_turno` toma
    /// `&self`; el guard del turno lo limpia al terminar (también si el
    /// future se cancela o el turno entra en pánico), de modo que nunca
    /// sobrevive a su turno.
    modo_turno: std::sync::Mutex<Option<String>>,
    /// [Bloque 3, F1] Guardas de turno (respuesta vacía → reintento único;
    /// repetición → aviso). Configurables por el consumidor; activas por
    /// defecto. Deterministas y sin I/O (guardas.rs).
    guardas: std::sync::Mutex<GuardasTurno>,
    /// [Bloque 3, F4] Hooks de ciclo de vida configurados (nucleo/hooks.rs):
    /// interior-mutable; vacíos por defecto = emisión no-op. El consumidor
    /// los fija con [`AgentRuntime::set_hooks`] tras construir el runtime.
    hooks: std::sync::Mutex<Arc<DispatcherHooks>>,
}

/* [059A-21] El veredicto de permiso del turno (`VerdictoPermiso` +
 * `decidir_permiso`) vive en `politica::permiso` junto a las demás decisiones
 * de política (una sola casa: permiso por modo, override, reglas, veredicto).
 * Aquí solo se re-exporta para que los flujos del turno (turno/permisos.rs,
 * subagente.rs) lo consuman vía `super::*` sin conocer el detalle. */
pub use crate::politica::permiso::{decidir_permiso, VerdictoPermiso};
/* [059A-21] La maquinaria del prompt por capas (`SYSTEM_PROMPT`,
 * `DesgloseContexto`, `ensamblar_prompt_sistema`, `fecha_hoy`,
 * `workspace_visible`, `info_git`) vive en `nucleo::prompt` (extraida de
 * aqui en 059A-21). El runtime solo la re-exporta para que los flujos del
 * turno (`super::*`) y los consumidores (`crate::runtime::fecha_hoy` en
 * context.rs, `glory_harness_core::runtime::ensamblar_prompt_sistema` en el
 * cli) sigan resolviendo sin conocer el detalle. */
pub(crate) use crate::nucleo::prompt::fecha_hoy;
#[cfg(test)]
pub(crate) use crate::nucleo::prompt::info_git;
pub use crate::nucleo::prompt::{ensamblar_prompt_sistema, DesgloseContexto};

#[cfg(test)]
mod tests {
    use super::ciclo::mensajes_usuario_resumen;
    use super::DesgloseContexto;
    use crate::context::ContextoConfig;
    use crate::llm::AiMessage;
    use uuid::Uuid;

    #[test]
    fn resumen_acota_prompt() {
        let largo = "x".repeat(2000);
        assert_eq!(mensajes_usuario_resumen(&largo).len(), 500);
    }

    /* [318A-15 F3] Gate de permisos: ask emite la pregunta, deny deniega,
     * y ninguno de los dos se reintenta en el mismo turno (el repetido no
     * vuelve a emitir el evento: el modelo ya fue informado). */
    #[test]
    fn f3_ask_pregunta_y_el_repetido_no_reeventa() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Ask, false),
            VerdictoPermiso::Preguntar
        );
        assert_eq!(
            decidir_permiso(Permiso::Ask, true),
            VerdictoPermiso::RepetidoPregunta
        );
    }

    #[test]
    fn f3_deny_deniega_y_el_repetido_no_reeventa() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Deny, false),
            VerdictoPermiso::Denegar
        );
        assert_eq!(
            decidir_permiso(Permiso::Deny, true),
            VerdictoPermiso::RepetidoDenegado
        );
    }

    #[test]
    fn f3_allow_ejecuta_siempre() {
        use super::{decidir_permiso, VerdictoPermiso};
        use crate::permiso::Permiso;
        assert_eq!(
            decidir_permiso(Permiso::Allow, false),
            VerdictoPermiso::Ejecutar
        );
        assert_eq!(
            decidir_permiso(Permiso::Allow, true),
            VerdictoPermiso::Ejecutar
        );
    }

    /* [318A-15 F5] El wrap-up al agotar `max_turns` cierra con estructura
     * (hecho / pendiente / siguiente) y prohíbe seguir ejecutando tools. */
    #[test]
    fn wrap_up_pide_cierre_estructurado_sin_tools() {
        use super::ciclo::{wrap_up_instruccion, WRAP_UP_TEXTO};
        let consigna = wrap_up_instruccion();
        assert_eq!(consigna, WRAP_UP_TEXTO);
        for eje in ["HECHO", "PENDIENTE", "SIGUIENTE PASO"] {
            assert!(
                consigna.contains(eje),
                "la consigna de cierre cubre el eje {eje}"
            );
        }
        assert!(consigna.contains("NO ejecutes más herramientas"));
    }

    fn config_prueba() -> ContextoConfig {
        ContextoConfig {
            max_ventana: 128_000,
            reserva_salida: 20_000,
            umbral: 0.5,
            cola_verbatim: 0.025,
            umbral_piso: 0.75,
            umbral_degenerado: 0.85,
            ..ContextoConfig::default()
        }
    }

    fn mensaje(rol: &str, texto: impl Into<String>) -> AiMessage {
        AiMessage::texto(rol, texto)
    }

    /* [318A-7] Tests del desglose de contexto emitido en cada llamada LLM:
     * separa system / definiciones de tools / mensajes / resultados de tools
     * y calcula la ocupación contra la ventana efectiva. */

    #[test]
    fn desglose_separa_secciones() {
        /* "aaaa" = 4 chars = 1 token; "bbbbbbbb" = 8 chars = 2 tokens. */
        let mensajes = vec![
            mensaje("system", "aaaa"),
            mensaje("user", "bbbbbbbb"),
            mensaje("assistant", "bbbbbbbb"),
            mensaje("tool", "aaaa"),
        ];
        /* JSON serializado: {"name":"aaaa"} = 14 chars = 4 tokens. */
        let schemas = vec![serde_json::json!({"name": "aaaa"})];
        let desglose = DesgloseContexto::calcular(&mensajes, &schemas, &config_prueba());

        assert_eq!(desglose.system_instrucciones, 1);
        assert_eq!(desglose.mensajes, 4); // user 2 + assistant 2
        assert_eq!(desglose.resultados_tools, 1);
        assert_eq!(desglose.definiciones_tools, 4);
        assert_eq!(desglose.total_entrada, 10);
        assert_eq!(desglose.max_ventana, 128_000);
        assert_eq!(desglose.reserva_salida, 20_000);
    }

    #[test]
    fn desglose_calcula_ocupacion_sobre_ventana_efectiva() {
        /* Ventana efectiva = 128_000 − 20_000 = 108_000. 10_800 tokens = 10%. */
        let mensajes = vec![mensaje("system", "a".repeat(43_200))]; // 10_800 tokens
        let desglose = DesgloseContexto::calcular(&mensajes, &[], &config_prueba());

        assert_eq!(desglose.total_entrada, 10_800);
        assert!(
            (desglose.ocupacion_pct - 10.0).abs() < 0.001,
            "esperado 10%, got {}",
            desglose.ocupacion_pct
        );
    }

    #[test]
    fn desglose_sin_mensajes_es_cero() {
        let desglose = DesgloseContexto::calcular(&[], &[], &config_prueba());
        assert_eq!(desglose.total_entrada, 0);
        assert_eq!(desglose.ocupacion_pct, 0.0);
    }

    /* [318A-15 F1] Tests del prompt por capas. `ensamblar_prompt_sistema`
     * recibe la fecha como parámetro para que las aserciones sean
     * deterministas (el E2E no depende del proveedor ni del reloj). */

    use super::construccion::TurnoConfig;
    use super::{ensamblar_prompt_sistema, info_git};
    use crate::context::{CIERRE_ENTORNO, CIERRE_REGLAS, MARCA_ENTORNO, MARCA_REGLAS};

    fn config_con_workspace(workspace: Option<&str>) -> TurnoConfig {
        TurnoConfig {
            workspace: workspace.map(str::to_owned),
            ..TurnoConfig::default()
        }
    }

    /// Directorio temporal único por test (bajo el temp del sistema), para
    /// ejercitar la detección git sin tocar el árbol del proyecto.
    fn dir_temporal(nombre: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-f1-{}-{}-{nombre}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    #[test]
    fn prompt_fixture_turno_contiene_fecha_workspace_y_marcadores() {
        /* Fixture del E2E: un turno que inyecta reglas en la ranura y un
         * workspace real; el prompt ensamblado debe llevar fecha, workspace,
         * modelo y AMBOS marcadores, con el bloque de entorno cerrado. */
        let config = config_con_workspace(Some("C:/workspace/fixture-proyecto"));
        let reglas = "Regla de prueba: los cambios se describen en español.";
        let prompt = ensamblar_prompt_sistema(&config, reglas, "2026-09-03");

        assert!(prompt.contains(MARCA_ENTORNO), "marca [ENTORNO] presente");
        assert!(
            prompt.contains(CIERRE_ENTORNO),
            "cierre [/ENTORNO] presente"
        );
        assert!(prompt.contains("Fecha: 2026-09-03"), "fecha inyectada");
        assert!(
            prompt.contains("Workspace: C:/workspace/fixture-proyecto"),
            "workspace inyectado"
        );
        assert!(
            prompt.contains(MARCA_REGLAS),
            "marca [REGLAS] presente con contenido"
        );
        assert!(prompt.contains(CIERRE_REGLAS), "cierre [/REGLAS] presente");
        assert!(prompt.contains(reglas), "contenido de reglas presente");
        assert!(
            prompt.contains("Modelo activo"),
            "modelo activo en el entorno"
        );
        assert!(
            prompt.contains("Git: no"),
            "sin repo en el fixture → Git: no"
        );
    }

    #[test]
    fn capa_reglas_vacia_no_deja_marcador_huerfano() {
        let config = config_con_workspace(None);
        let prompt = ensamblar_prompt_sistema(&config, "   ", "2026-09-03");

        assert!(prompt.contains(MARCA_ENTORNO));
        assert!(
            !prompt.contains(MARCA_REGLAS),
            "sin [REGLAS] huérfano cuando la capa está vacía"
        );
        assert!(!prompt.contains(CIERRE_REGLAS));
        assert!(
            prompt.contains("Workspace: (no disponible)"),
            "sin workspace no se inventa una ruta (no cae al cwd del proceso)"
        );
    }

    #[test]
    fn capas_en_orden_estatico_luego_dinamico() {
        let config = config_con_workspace(Some("C:/workspace/x"));
        let prompt = ensamblar_prompt_sistema(&config, "una regla", "2026-09-03");

        let base = prompt.find("DIRECTRICES:").expect("capa base presente");
        let reglas = prompt.find(MARCA_REGLAS).expect("ranura reglas presente");
        let entorno = prompt.find(MARCA_ENTORNO).expect("entorno presente");
        let modelo = prompt.find("Modelo activo").expect("modelo presente");
        assert!(
            base < reglas && reglas < entorno && entorno < modelo,
            "orden base → reglas → entorno"
        );
    }

    #[test]
    fn prompt_sistema_del_runtime_lleva_fecha_iso() {
        /* El camino real del turno usa `fecha_hoy()`; verificamos el formato
         * sin depender del reloj (determinismo): "Fecha: AAAA-MM-DD". */
        let config = config_con_workspace(None);
        let fecha = chrono::Utc::now().format("%Y-%m-%d").to_string();
        let prompt = ensamblar_prompt_sistema(&config, "", &fecha);
        assert!(prompt.contains(&format!("Fecha: {fecha}")));
    }

    #[test]
    fn info_git_detecta_rama_y_ausencia_de_repo() {
        let repo = dir_temporal("git-rama");
        std::fs::create_dir_all(repo.join(".git")).expect("crear .git");
        std::fs::write(repo.join(".git").join("HEAD"), "ref: refs/heads/main\n")
            .expect("escribir HEAD");
        let rama = info_git(repo.to_str().expect("ruta utf8"));
        assert_eq!(rama.as_deref(), Some("main"));
        std::fs::remove_dir_all(&repo).ok();

        let sin_repo = dir_temporal("sin-repo");
        let rama = info_git(sin_repo.to_str().expect("ruta utf8"));
        assert_eq!(rama, None);
        std::fs::remove_dir_all(&sin_repo).ok();
    }

    /* [109A-4 F4] `/meta <texto>`: UN turno solo-lectura. El modo forzado vive
     * en el runtime (no en la config de la sesión) y el guard del turno lo
     * limpia al salir, así que no se filtra al turno siguiente. */

    /// Runtime mínimo para tests del override: sin tools de dominio (las
    /// agnósticas de memoria/web las añade `nuevo`), LLM sin claves (no se
    /// llama) y persistencia en memoria.
    fn runtime_de_prueba(modo: &str) -> super::AgentRuntime {
        use crate::llm::{LlavesProveedor, LlmProviderService};
        use crate::nucleo::memoria::soporte::TiendaPrueba;
        use crate::tool::AgentToolRegistry;
        use std::sync::Arc;

        super::AgentRuntime::nuevo(
            AgentToolRegistry::new(),
            super::construccion::PuertosHarness {
                persistencia: Arc::new(TiendaPrueba::default()),
                llm: Arc::new(LlmProviderService::new(LlavesProveedor::from_env())),
                web_search: None,
                web_fetch: None,
                dominio: None,
                ejecutor_comando: None,
                programador_tareas: None,
                navegador: None,
            },
            super::construccion::TurnoConfig {
                modo: modo.into(),
                ..super::construccion::TurnoConfig::default()
            },
        )
    }

    #[test]
    fn f4_modo_forzado_no_toca_el_modo_de_la_sesion() {
        let runtime = runtime_de_prueba("predeterminado");
        assert_eq!(runtime.modo_efectivo(), "predeterminado");

        let guarda = runtime.guarda_modo_turno(Some("meta"));
        assert_eq!(runtime.modo_efectivo(), "meta");
        /* El modo de la sesión sigue intacto: el override es del turno. */
        assert_eq!(runtime.turno_config.modo, "predeterminado");
        drop(guarda);

        /* Soltado el guard (fin, error o cancelación del turno) el runtime
         * vuelve al modo de la sesión: nada se filtra al turno siguiente. */
        assert_eq!(runtime.modo_efectivo(), "predeterminado");

        /* Un turno normal posterior tampoco hereda un override previo. */
        let guarda = runtime.guarda_modo_turno(Some("meta"));
        drop(guarda);
        let guarda = runtime.guarda_modo_turno(None);
        assert_eq!(runtime.modo_efectivo(), "predeterminado");
        drop(guarda);
    }

    #[test]
    fn f4_modo_forzado_deniega_efectos_y_deja_leer() {
        use crate::permiso::Permiso;

        let runtime = runtime_de_prueba("autonomo");
        /* Sin override manda la sesión (autónomo): no se pregunta nada. */
        assert_eq!(
            runtime
                .registry
                .permiso_para("memoria_guardar", &runtime.modo_efectivo()),
            Permiso::Allow
        );

        let _guarda = runtime.guarda_modo_turno(Some("meta"));
        let modo = runtime.modo_efectivo();

        /* Con efecto → deny, y la tool NI SE OFRECE en el schema del turno
         * (deny silencioso: el modelo no la ve en ese turno). */
        assert_eq!(
            runtime.registry.permiso_para("memoria_guardar", &modo),
            Permiso::Deny
        );
        assert!(runtime.registry.esta_denegada("memoria_guardar", &modo));
        let nombres: Vec<String> = runtime
            .registry
            .schemas_openai(None, &modo)
            .iter()
            .map(|s| s["function"]["name"].as_str().unwrap_or("").to_string())
            .collect();
        assert!(
            !nombres.iter().any(|n| n == "memoria_guardar"),
            "una tool con efecto no debe verse en un turno forzado a meta: {nombres:?}"
        );

        /* Sin efecto → allow, y sigue disponible para que el turno pueda
         * leer y responder. */
        assert_eq!(
            runtime.registry.permiso_para("memoria_recordar", &modo),
            Permiso::Allow
        );
        assert!(nombres.iter().any(|n| n == "memoria_recordar"));
    }

    /* [109A-5 F2] El plan visible es de la CONVERSACIÓN, no del proceso: un
     * runtime atiende a varias y la store vive en su registry. Estos tests
     * fijan el reparto (sin él, cambiar de hilo mostraba el plan del anterior
     * y el modelo podía editar tareas ajenas). */

    /// Crea un runtime de prueba con tareas ya en el plan (como si un turno
    /// anterior las hubiera dejado).
    async fn runtime_con_plan(modo: &str, textos: &[&str]) -> super::AgentRuntime {
        let runtime = runtime_de_prueba(modo);
        let store = runtime.registry.todo().expect("store de todo registrada");
        let mut lista = store.lock().await;
        for texto in textos {
            lista.crear(texto);
        }
        drop(lista);
        runtime
    }

    /// Textos del plan cargado ahora mismo en el registry.
    async fn plan_cargado(runtime: &super::AgentRuntime) -> Vec<String> {
        let store = runtime.registry.todo().expect("store de todo registrada");
        /* El guard se liga a una local para que se suelte ANTES de que muera el
         * `Arc` que lo presta (un temporal al final del bloque lo haría tarde). */
        let guard = store.lock().await;
        let textos: Vec<String> = guard.visibles().into_iter().map(|t| t.texto).collect();
        drop(guard);
        textos
    }

    #[tokio::test]
    async fn f2_el_plan_no_se_filtra_entre_conversaciones() {
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        /* Se prepara el plan de A y luego se visita B: B no debe heredarlo. */
        let runtime = runtime_con_plan("predeterminado", &["paso de A"]).await;
        runtime.cargar_plan_de(conv_a);
        assert_eq!(plan_cargado(&runtime).await, vec!["paso de A".to_string()]);

        runtime.cargar_plan_de(conv_b);
        assert!(
            plan_cargado(&runtime).await.is_empty(),
            "una conversación nueva arranca sin plan ajeno"
        );
    }

    #[tokio::test]
    async fn f2_volver_a_una_conversacion_restaura_su_plan() {
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        let runtime = runtime_con_plan("predeterminado", &["paso de A"]).await;
        runtime.cargar_plan_de(conv_a);
        runtime.cargar_plan_de(conv_b);
        runtime.cargar_plan_de(conv_a);
        assert_eq!(
            plan_cargado(&runtime).await,
            vec!["paso de A".to_string()],
            "el resume es por conversación: volver a A conserva su plan"
        );
        /* Y el plan de A no se duplicó al ir y volver. */
        runtime.cargar_plan_de(conv_a);
        assert_eq!(plan_cargado(&runtime).await.len(), 1);
    }

    #[tokio::test]
    async fn f2_olvidar_tareas_solo_borra_la_conversacion_indicada() {
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        let runtime = runtime_con_plan("predeterminado", &["paso de B"]).await;
        runtime.cargar_plan_de(conv_b);
        /* Cerrar la meta de A (que no está activa) no debe tocar el plan de B. */
        assert!(runtime.olvidar_tareas(conv_a));
        assert_eq!(plan_cargado(&runtime).await, vec!["paso de B".to_string()]);
        /* Cerrar la de B sí vacía el plan cargado. */
        assert!(runtime.olvidar_tareas(conv_b));
        assert!(plan_cargado(&runtime).await.is_empty());
    }

    #[tokio::test]
    async fn f2_emitir_tareas_publica_la_lista_completa_y_respeta_el_silencio() {
        let conv = Uuid::new_v4();
        let runtime = runtime_con_plan("predeterminado", &["uno", "dos"]).await;
        runtime.cargar_plan_de(conv);
        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::evento::AgenteEvento>(8);
        runtime.emitir_tareas(&tx, true).await;
        drop(tx);
        match rx.try_recv() {
            Ok(crate::evento::AgenteEvento::TareasActualizadas { items }) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].texto, "uno");
            }
            otro => panic!("se esperaba TareasActualizadas, llegó {otro:?}"),
        }

        /* Lista vacía + `solo_si_hay`: no se emite nada (el arranque de un
         * turno sin plan no debe pintar un bloque huérfano). */
        let vacio = Uuid::new_v4();
        let (tx, mut rx) = tokio::sync::mpsc::channel::<crate::evento::AgenteEvento>(8);
        runtime.cargar_plan_de(vacio);
        runtime.emitir_tareas(&tx, true).await;
        drop(tx);
        assert!(
            rx.try_recv().is_err(),
            "sin tareas y sin acción previa no hay evento"
        );
    }

    /* [109A-5 F4] El contador de bloqueo es el oráculo que decide la pausa de
     * la meta, y vive en el runtime: la tool declara el bloqueo, el runtime
     * suma un turno al cerrar y el servicio lee el resultado. Estos tests fijan
     * las dos invariantes que sostienen ese camino sin depender del modelo: el
     * contador es por CONVERSACIÓN (como el plan) y volver a la conversación
     * conserva los turnos acumulados. */

    #[tokio::test]
    async fn f4_el_bloqueo_cuenta_turnos_por_conversacion_y_los_conserva_al_volver() {
        let conv_a = Uuid::new_v4();
        let conv_b = Uuid::new_v4();
        let runtime = runtime_de_prueba("predeterminado");

        runtime.cargar_plan_de(conv_a);
        {
            let store = runtime.registry.todo().expect("store de todo registrada");
            let mut lista = store.lock().await;
            lista.bloquear("falta la API key").expect("motivo válido");
        }
        /* Tres turnos cerrados con el mismo motivo: el umbral que el servicio
         * traduce en pausa. */
        for _ in 0..3 {
            runtime.contar_bloqueo_del_turno(conv_a).await;
        }
        let bloqueo = runtime.bloqueo_de(conv_a).expect("A sigue bloqueada");
        assert_eq!(bloqueo.motivo, "falta la API key");
        assert_eq!(bloqueo.turnos, 3);

        /* La conversación B no hereda el bloqueo ni el contador de A. */
        runtime.cargar_plan_de(conv_b);
        assert!(
            runtime.bloqueo_de(conv_b).is_none(),
            "el bloqueo de A no puede viajar a B"
        );

        /* Irse y volver conserva lo acumulado: el contador es del plan de A. */
        runtime.cargar_plan_de(conv_a);
        assert_eq!(
            runtime
                .bloqueo_de(conv_a)
                .expect("A sigue bloqueada")
                .turnos,
            3
        );

        /* Y un avance real del plan lo levanta: sin eso la meta quedaría pausada
         * para siempre por un motivo ya resuelto. */
        {
            let store = runtime.registry.todo().expect("store de todo registrada");
            let mut lista = store.lock().await;
            lista.crear("paso siguiente");
        }
        assert!(
            runtime.bloqueo_de(conv_a).is_none(),
            "avanzar el plan levanta el bloqueo vigente"
        );
    }
}
