// [Bloque 3, F3] Orquestacion de hooks: matcher y DispatcherHooks.
// Depende de tipos (contratos) y runner (ejecucion).
use super::runner::{RunnerComandoHttp, RunnerHook, SalidaHook};
use super::tipos::{EventoHook, Hook};
use serde_json::Value;
use std::sync::Arc;

/// [Bloque 3, F4] Matcher comodín de un patrón contra un valor (`*` = lo que
/// sea). Semántica de los matchers de tool de claurst: `"file_*"`, `"*_read"`,
/// etc. Patrón sin `*` = igualdad exacta.
#[must_use]
pub fn patron_coincide(patron: &str, valor: &str) -> bool {
    if patron == valor {
        return true;
    }
    if !patron.contains('*') {
        return false;
    }
    let segmentos: Vec<&str> = patron.split('*').collect();
    let mut resto = valor;
    for (i, segmento) in segmentos.iter().enumerate() {
        if segmento.is_empty() {
            continue;
        }
        let pos = if i == 0 {
            /* Segmento inicial: anclado al principio. */
            if resto.starts_with(segmento) {
                Some(0)
            } else {
                None
            }
        } else if i == segmentos.len() - 1 {
            /* Segmento final: anclado al final. */
            resto
                .len()
                .checked_sub(segmento.len())
                .filter(|inicio| &resto[*inicio..] == *segmento)
        } else {
            resto.find(segmento)
        };
        let Some(pos) = pos else {
            return false;
        };
        resto = &resto[pos + segmento.len()..];
        if i == 0 {
            resto = &valor[segmento.len()..];
        }
    }
    true
}

/// [Bloque 3, F4] Dispatcher de hooks configurados: recorre los que coinciden
/// con el evento (+ patrón de tool del payload) y devuelve si la acción debe
/// bloquearse. Sin hooks → no-op (no cambia el comportamiento del runtime).
pub struct DispatcherHooks {
    hooks: Vec<Hook>,
    runner: Arc<dyn RunnerHook>,
}

impl std::fmt::Debug for DispatcherHooks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DispatcherHooks")
            .field("hooks", &self.hooks)
            .field("runner", &"<dyn RunnerHook>")
            .finish()
    }
}

impl DispatcherHooks {
    /// Dispatcher sin hooks (no-op). Equivale al estado por defecto del
    /// runtime: emitir con él no cuesta y no cambia ningún comportamiento.
    #[must_use]
    pub fn vacia() -> Self {
        Self {
            hooks: Vec::new(),
            runner: Arc::new(RunnerComandoHttp::default()),
        }
    }

    /// Dispatcher con un runner inyectado (tests: runner que graba).
    #[must_use]
    pub fn con_runner(runner: Arc<dyn RunnerHook>) -> Self {
        Self {
            hooks: Vec::new(),
            runner,
        }
    }

    /// Añade un hook configurado.
    pub fn registrar(&mut self, hook: Hook) -> &mut Self {
        self.hooks.push(hook);
        self
    }

    /// Hooks registrados (para inspección/UI).
    #[must_use]
    pub fn hooks(&self) -> &[Hook] {
        &self.hooks
    }

    /// Dispara los hooks y conserva su resultado estructurado. Los fallos del
    /// runner se registran y se continúa (nunca abortan). Si varios hooks
    /// devuelven un ajuste, gana el último en orden de registro; el veto es OR.
    pub async fn disparar_con_salida(&self, evento: EventoHook, payload: Value) -> SalidaHook {
        let mut salida_final = SalidaHook::CONTINUAR;
        for hook in self.hooks.iter().filter(|h| h.evento == evento) {
            if let Some(patron) = &hook.tool_patron {
                let tool = payload
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                if !patron_coincide(patron, tool) {
                    continue;
                }
            }
            match self.runner.correr(hook, &payload).await {
                Ok(salida) => {
                    if salida.bloqueo && evento.puede_bloquear() {
                        salida_final.bloqueo = true;
                    }
                    if evento == EventoHook::PreCompact {
                        salida_final.ajuste = salida.ajuste;
                    }
                }
                Err(error) => {
                    /* Un hook roto no rompe el turno: se registra y se
                     * continúa con la semántica de sin-hook. */
                    tracing::warn!(hook = %hook.nombre, %error, "hook falló; se continúa sin él");
                }
            }
        }
        salida_final
    }

    /// Compatibilidad para eventos cuyo consumidor solo necesita el veto.
    pub async fn disparar(&self, evento: EventoHook, payload: Value) -> bool {
        self.disparar_con_salida(evento, payload).await.bloqueo
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{EventoHook, Hook, RunnerHook, SalidaHook};
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::Mutex;

    /// Runner de pruebas: graba (evento, payload) por hook y devuelve la
    /// salida configurada (o error) — nunca lanza procesos ni hace HTTP.
    struct RunnerGrabador {
        registros: Mutex<Vec<(String, Value)>>,
        respuesta: Mutex<Result<SalidaHook, String>>,
    }

    impl RunnerGrabador {
        fn nuevo() -> Arc<Self> {
            Arc::new(Self {
                registros: Mutex::new(Vec::new()),
                respuesta: Mutex::new(Ok(SalidaHook::CONTINUAR)),
            })
        }
        fn con_salida(respuesta: Result<SalidaHook, String>) -> Arc<Self> {
            Arc::new(Self {
                registros: Mutex::new(Vec::new()),
                respuesta: Mutex::new(respuesta),
            })
        }
        fn registros(&self) -> Vec<(String, Value)> {
            self.registros.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RunnerHook for RunnerGrabador {
        async fn correr(
            &self,
            hook: &Hook,
            payload: &Value,
        ) -> std::result::Result<SalidaHook, String> {
            self.registros
                .lock()
                .unwrap()
                .push((hook.nombre.clone(), payload.clone()));
            self.respuesta.lock().unwrap().clone()
        }
    }

    fn payload_tool(tool: &str) -> Value {
        serde_json::json!({ "tool": tool })
    }

    /* --- Matcher puro --- */

    #[test]
    fn patron_igualdad_exacta_sin_comodin() {
        assert!(patron_coincide("web_search", "web_search"));
        assert!(!patron_coincide("web_search", "web_fetch"));
    }

    #[test]
    fn patron_comodin_medio() {
        assert!(patron_coincide("file_*", "file_read"));
        assert!(patron_coincide("file_*", "file_write"));
        assert!(!patron_coincide("file_*", "web_fetch"));
        assert!(patron_coincide("co*_write", "comando_write"));
    }

    #[test]
    fn patron_comodin_prefijo_y_sufijo() {
        assert!(patron_coincide("*_read", "file_read"));
        assert!(patron_coincide("*_read", "task_read"));
        assert!(patron_coincide("ask*", "ask_user"));
        assert!(!patron_coincide("*_read", "file_write"));
    }

    #[test]
    fn patron_estrella_global() {
        assert!(patron_coincide("*", "cualquier_tool"));
        assert!(patron_coincide("**", "cualquier_tool"));
    }

    /* --- Eventos y bloqueo --- */

    #[test]
    fn eventos_bloqueables_y_nombres() {
        assert!(EventoHook::PreToolUse.puede_bloquear());
        assert!(EventoHook::PreCompact.puede_bloquear());
        assert!(EventoHook::PermissionRequest.puede_bloquear());
        assert!(!EventoHook::PostToolUse.puede_bloquear());
        assert!(!EventoHook::Stop.puede_bloquear());
        assert_eq!(EventoHook::PreToolUse.nombre(), "PreToolUse");
        assert_eq!(EventoHook::SessionEnd.nombre(), "SessionEnd");
    }

    #[tokio::test]
    async fn sin_hooks_es_no_op() {
        let d = DispatcherHooks::vacia();
        assert!(
            !d.disparar(EventoHook::PreToolUse, payload_tool("file_read"))
                .await
        );
    }

    #[tokio::test]
    async fn solo_dispara_hooks_del_evento_correcto() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando(
            "post",
            EventoHook::PostToolUse,
            "echo",
            vec![],
        ));
        d.registrar(Hook::comando("stop", EventoHook::Stop, "echo", vec![]));

        d.disparar(EventoHook::Stop, serde_json::json!({})).await;

        let registros = runner.registros();
        assert_eq!(registros.len(), 1, "solo el hook del evento Stop");
        assert_eq!(registros[0].0, "stop");
    }

    #[tokio::test]
    async fn payload_llega_integro_al_runner() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando("pre", EventoHook::PreToolUse, "echo", vec![]));

        let payload = serde_json::json!({ "tool": "comando", "tool_input": { "a": 1 } });
        d.disparar(EventoHook::PreToolUse, payload.clone()).await;

        let registros = runner.registros();
        assert_eq!(registros.len(), 1);
        assert_eq!(registros[0].1, payload);
    }

    #[tokio::test]
    async fn bloqueo_pre_tool_use_se_propaga() {
        let runner = RunnerGrabador::con_salida(Ok(SalidaHook::BLOQUEAR));
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando(
            "veto",
            EventoHook::PreToolUse,
            "false",
            vec![],
        ));

        assert!(
            d.disparar(EventoHook::PreToolUse, payload_tool("comando"))
                .await,
            "PreToolUse puede bloquear"
        );
    }

    #[tokio::test]
    async fn precompact_acepta_ajuste_y_veto() {
        let runner = RunnerGrabador::con_salida(Ok(SalidaHook {
            bloqueo: true,
            ajuste: Some(serde_json::json!({"resumen_llm": "ajustado"})),
        }));
        let mut d = DispatcherHooks::con_runner(runner);
        d.registrar(Hook::comando("pre", EventoHook::PreCompact, "hook", vec![]));
        let salida = d
            .disparar_con_salida(EventoHook::PreCompact, serde_json::json!({}))
            .await;
        assert!(salida.bloqueo);
        assert_eq!(salida.ajuste, Some(serde_json::json!({"resumen_llm": "ajustado"})));
    }

    #[tokio::test]
    async fn bloqueo_se_ignora_en_eventos_informativos() {
        let runner = RunnerGrabador::con_salida(Ok(SalidaHook::BLOQUEAR));
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando(
            "post",
            EventoHook::PostToolUse,
            "false",
            vec![],
        ));
        d.registrar(Hook::comando("stop", EventoHook::Stop, "false", vec![]));

        assert!(
            !d.disparar(EventoHook::PostToolUse, payload_tool("comando"))
                .await,
            "PostToolUse es informativo: el bloqueo no aplica"
        );
        assert!(!d.disparar(EventoHook::Stop, serde_json::json!({})).await);
    }

    #[tokio::test]
    async fn fallo_del_runner_no_aborta_y_no_bloquea() {
        let runner = RunnerGrabador::con_salida(Err("proceso ausente".into()));
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando(
            "roto",
            EventoHook::PreToolUse,
            "no_existe",
            vec![],
        ));

        assert!(
            !d.disparar(EventoHook::PreToolUse, payload_tool("comando"))
                .await,
            "un hook que falla se ignora (fail-open del observador, no del permiso)"
        );
    }

    #[tokio::test]
    async fn matcher_de_tool_filtra_por_payload() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(
            Hook::comando("solo_archivo", EventoHook::PreToolUse, "echo", vec![])
                .para_tool("file_*"),
        );

        d.disparar(EventoHook::PreToolUse, payload_tool("web_fetch"))
            .await;
        assert!(
            runner.registros().is_empty(),
            "web_fetch no coincide con file_*"
        );

        d.disparar(EventoHook::PreToolUse, payload_tool("file_write"))
            .await;
        assert_eq!(
            runner.registros().len(),
            1,
            "file_write coincide con file_*"
        );
    }

    #[tokio::test]
    async fn hook_sin_patron_corre_para_toda_tool() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando(
            "todas",
            EventoHook::PreToolUse,
            "echo",
            vec![],
        ));

        d.disparar(EventoHook::PreToolUse, payload_tool("cualquiera"))
            .await;
        assert_eq!(runner.registros().len(), 1);
    }

    #[tokio::test]
    async fn orden_de_registro_es_el_orden_de_disparo() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(Hook::comando("uno", EventoHook::Stop, "echo", vec![]));
        d.registrar(Hook::comando("dos", EventoHook::Stop, "echo", vec![]));
        d.registrar(Hook::comando("tres", EventoHook::Stop, "echo", vec![]));

        d.disparar(EventoHook::Stop, serde_json::json!({})).await;

        let nombres: Vec<String> = runner.registros().into_iter().map(|(n, _)| n).collect();
        assert_eq!(nombres, vec!["uno", "dos", "tres"]);
    }

    #[tokio::test]
    async fn el_patron_usa_el_campo_tool_del_payload() {
        let runner = RunnerGrabador::nuevo();
        let mut d = DispatcherHooks::con_runner(runner.clone());
        d.registrar(
            Hook::comando("mira_tool", EventoHook::PermissionRequest, "echo", vec![])
                .para_tool("*_write"),
        );

        d.disparar(
            EventoHook::PermissionRequest,
            serde_json::json!({ "tool": "comando_write" }),
        )
        .await;
        assert_eq!(
            runner.registros().len(),
            1,
            "comando_write coincide con *_write"
        );
    }
}
