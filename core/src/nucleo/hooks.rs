//! [Bloque 3, F4] Hooks de ciclo de vida del agente (evidencia: claurst
//! `docs/hooks.md` + `spec/07_hooks.md`).
//!
//! El núcleo emite eventos en los puntos del ciclo de vida (turno, tool,
//! subagente, compactación, permiso, sesión) y los hooks configurados se
//! disparan SIN acoplarse a un runner concreto: `DispatcherHooks` delega en
//! [`RunnerHook`], de modo que los tests inyectan un runner que graba (nunca
//! lanzan procesos) y el CLI/producción usa [`RunnerComandoHttp`].
//!
//! Semántica (espejo claurst):
//! - Un hook es `command` (proceso local con timeout) u `http` (POST JSON).
//!   Los tipos `prompt`/`agent` quedan diferidos (decisión del plan).
//! - Matcher por evento + patrón opcional de tool con `*` (comodín).
//! - Un hook de `command` que termina con exit code 2 BLOQUEA la acción en
//!   curso, pero solo en los eventos que pueden bloquear ([`EventoHook::
//!   puede_bloquear`]); el resto son informativos (sus fallos se registran y
//!   el turno continúa — nunca rompen la ejecución).
//! - El payload viaja como JSON (stdin del proceso / cuerpo del POST).
//! - Sin hooks configurados el dispatcher es un no-op barato: el runtime no
//!   cambia su comportamiento (los hooks son observación/política opcional).

use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

/// Timeout por defecto de cada hook (los fallos nunca abortan el turno;
/// se registran y se continúa con la misma semántica que sin hook).
pub const TIMEOUT_HOOK_DEFAULT: Duration = Duration::from_secs(10);

/// [Bloque 3, F4] Eventos de ciclo de vida que el núcleo puede emitir.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventoHook {
    /// Antes de ejecutar una tool aprobada (puede bloquearla).
    PreToolUse,
    /// Tras ejecutar una tool (con su resultado), sea éxito o fallo.
    PostToolUse,
    /// El agente terminó de responder al prompt del turno.
    Stop,
    /// El usuario envió un mensaje que inicia un turno.
    UserPromptSubmit,
    /// Sesión de conversación iniciada (el consumidor la abre al arrancar).
    SessionStart,
    /// Sesión de conversación terminada.
    SessionEnd,
    /// Sesión hija (subagente) iniciada.
    SubagentStart,
    /// Sesión hija (subagente) terminada.
    SubagentStop,
    /// Se va a compactar el contexto (solo cuando la compactación ocurre).
    PreCompact,
    /// La compactación del contexto terminó.
    PostCompact,
    /// Una tool pide permiso al usuario (veredicto ask).
    PermissionRequest,
}

impl EventoHook {
    /// [Bloque 3, F4] Eventos cuyo resultado puede BLOQUEAR la acción en
    /// curso (claurst: exit 2 del hook). El resto son informativos.
    #[must_use]
    pub fn puede_bloquear(self) -> bool {
        matches!(self, EventoHook::PreToolUse | EventoHook::PermissionRequest)
    }

    /// Nombre estable del evento (para logs y payloads de diagnóstico).
    #[must_use]
    pub fn nombre(self) -> &'static str {
        match self {
            EventoHook::PreToolUse => "PreToolUse",
            EventoHook::PostToolUse => "PostToolUse",
            EventoHook::Stop => "Stop",
            EventoHook::UserPromptSubmit => "UserPromptSubmit",
            EventoHook::SessionStart => "SessionStart",
            EventoHook::SessionEnd => "SessionEnd",
            EventoHook::SubagentStart => "SubagentStart",
            EventoHook::SubagentStop => "SubagentStop",
            EventoHook::PreCompact => "PreCompact",
            EventoHook::PostCompact => "PostCompact",
            EventoHook::PermissionRequest => "PermissionRequest",
        }
    }
}

/// [Bloque 3, F4] Tipos de hook implementados: `command` (proceso local con
/// timeout; exit 2 = bloquea) y `http` (POST del payload JSON). `prompt` y
/// `agent` quedan diferidos por decisión del plan (item 2).
#[derive(Debug, Clone)]
pub enum TipoHook {
    /// Proceso local: recibe el payload JSON por stdin.
    Comando { comando: String, args: Vec<String> },
    /// POST del payload JSON a la URL.
    Http { url: String },
}

/// [Bloque 3, F4] Hook configurado: a qué evento responde, con qué patrón de
/// tool (comodín `*`) y cómo se ejecuta.
#[derive(Debug, Clone)]
pub struct Hook {
    /// Nombre legible (aparece en logs y errores).
    pub nombre: String,
    pub evento: EventoHook,
    /// Filtro opcional por tool (aplica a Pre/PostToolUse y
    /// PermissionRequest). Admite `*`; `None` = todas las tools.
    pub tool_patron: Option<String>,
    pub tipo: TipoHook,
    pub timeout: Duration,
}

impl Hook {
    /// Hook de tipo `command`: proceso local que recibe el payload por stdin.
    #[must_use]
    pub fn comando(
        nombre: impl Into<String>,
        evento: EventoHook,
        comando: impl Into<String>,
        args: Vec<String>,
    ) -> Self {
        Self {
            nombre: nombre.into(),
            evento,
            tool_patron: None,
            tipo: TipoHook::Comando {
                comando: comando.into(),
                args,
            },
            timeout: TIMEOUT_HOOK_DEFAULT,
        }
    }

    /// Hook de tipo `http`: POST del payload JSON a la URL.
    #[must_use]
    pub fn http(nombre: impl Into<String>, evento: EventoHook, url: impl Into<String>) -> Self {
        Self {
            nombre: nombre.into(),
            evento,
            tool_patron: None,
            tipo: TipoHook::Http { url: url.into() },
            timeout: TIMEOUT_HOOK_DEFAULT,
        }
    }

    /// Restringe el hook a una tool (o patrón con `*`).
    #[must_use]
    pub fn para_tool(mut self, patron: impl Into<String>) -> Self {
        self.tool_patron = Some(patron.into());
        self
    }

    /// Timeout propio del hook (default [`TIMEOUT_HOOK_DEFAULT`]).
    #[must_use]
    pub fn con_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }
}

/// Resultado de ejecutar un hook.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SalidaHook {
    /// `true` = el hook pide bloquear la acción (solo aplica en eventos que
    /// pueden bloquear; claurst: exit code 2 del comando).
    pub bloqueo: bool,
}

impl SalidaHook {
    pub const CONTINUAR: Self = Self { bloqueo: false };
    pub const BLOQUEAR: Self = Self { bloqueo: true };
}

/// [Bloque 3, F4] Seam de ejecución: el dispatcher no conoce procesos ni
/// HTTP. Los tests inyectan un runner que graba; producción usa
/// [`RunnerComandoHttp`].
#[async_trait]
pub trait RunnerHook: Send + Sync {
    /// Ejecuta el hook con el payload ya serializable. `Err` = fallo del
    /// runner (se registra y el turno continúa; jamás aborta).
    async fn correr(&self, hook: &Hook, payload: &Value)
        -> std::result::Result<SalidaHook, String>;
}

/// Runner de producción: `command` → proceso local con el JSON en stdin y
/// timeout (exit 0/1 = continúa, exit 2 = bloquea); `http` → POST JSON
/// (informativo: 2xx = continúa, el resto se registra sin bloquear).
pub struct RunnerComandoHttp {
    /// Timeout aplicado cuando el hook no trae el suyo.
    pub timeout: Duration,
}

impl Default for RunnerComandoHttp {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT_HOOK_DEFAULT,
        }
    }
}

#[async_trait]
impl RunnerHook for RunnerComandoHttp {
    async fn correr(
        &self,
        hook: &Hook,
        payload: &Value,
    ) -> std::result::Result<SalidaHook, String> {
        let timeout = if hook.timeout.is_zero() {
            self.timeout
        } else {
            hook.timeout
        };
        match &hook.tipo {
            TipoHook::Comando { comando, args } => {
                let cuerpo = serde_json::to_string(payload).map_err(|e| e.to_string())?;
                let mut child = tokio::process::Command::new(comando)
                    .args(args)
                    .stdin(std::process::Stdio::piped())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .spawn()
                    .map_err(|e| format!("no se pudo lanzar '{comando}': {e}"))?;
                if let Some(mut stdin) = child.stdin.take() {
                    use tokio::io::AsyncWriteExt;
                    if let Err(e) = stdin.write_all(cuerpo.as_bytes()).await {
                        return Err(format!("no se pudo escribir el payload a '{comando}': {e}"));
                    }
                }
                match tokio::time::timeout(timeout, child.wait()).await {
                    Ok(Ok(status)) => {
                        /* [Bloque 3, F4] Exit 2 = bloquear (claurst). */
                        if status.code() == Some(2) {
                            Ok(SalidaHook::BLOQUEAR)
                        } else {
                            Ok(SalidaHook::CONTINUAR)
                        }
                    }
                    Ok(Err(e)) => Err(format!("'{comando}' falló: {e}")),
                    Err(_) => {
                        let _ = child.kill().await;
                        Err(format!(
                            "'{comando}' excedió el timeout de {}s",
                            timeout.as_secs()
                        ))
                    }
                }
            }
            TipoHook::Http { url } => {
                let cliente = reqwest::Client::new();
                match tokio::time::timeout(timeout, cliente.post(url).json(payload).send()).await {
                    Ok(Ok(respuesta)) => {
                        let status = respuesta.status();
                        if !status.is_success() {
                            tracing::warn!(url = %url, status = %status, "hook http respondió no-2xx (se continúa)");
                        }
                        Ok(SalidaHook::CONTINUAR)
                    }
                    Ok(Err(e)) => Err(format!("POST {url} falló: {e}")),
                    Err(_) => Err(format!(
                        "POST {url} excedió el timeout de {}s",
                        timeout.as_secs()
                    )),
                }
            }
        }
    }
}

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

    /// Dispara los hooks que coinciden con `evento` (y, si el payload trae
    /// `"tool"`, con el patrón del hook). Devuelve `true` si algún hook pidió
    /// bloquear y el evento lo permite ([`EventoHook::puede_bloquear`]). Los
    /// fallos del runner se registran y se continúa (nunca abortan).
    pub async fn disparar(&self, evento: EventoHook, payload: Value) -> bool {
        let mut bloqueado = false;
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
                        bloqueado = true;
                    }
                }
                Err(error) => {
                    /* [Bloque 3, F4] Un hook roto no rompe el turno: se
                     * registra y se continúa con la semántica de sin-hook. */
                    tracing::warn!(hook = %hook.nombre, %error, "hook falló; se continúa sin él");
                }
            }
        }
        bloqueado
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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

    #[test]
    fn constructor_por_tipo_es_diferenciable() {
        let cmd = Hook::comando("c", EventoHook::Stop, "echo", vec!["-n".into()]);
        assert!(matches!(cmd.tipo, TipoHook::Comando { .. }));
        assert!(cmd.tool_patron.is_none());
        let http = Hook::http("h", EventoHook::Stop, "http://localhost:9/hook")
            .con_timeout(Duration::from_secs(2));
        assert!(matches!(http.tipo, TipoHook::Http { .. }));
        assert_eq!(http.timeout, Duration::from_secs(2));
    }
}
