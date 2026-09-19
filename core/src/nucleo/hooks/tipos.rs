// [Bloque 3, F1] Contratos de hooks: tipos, constructores y validacion K4.
// Sin ejecucion (runner) ni orquestacion (despacho).
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Timeout por defecto de cada hook (los fallos nunca abortan el turno;
/// se registran y se continúa con la misma semántica que sin hook).
pub const TIMEOUT_HOOK_DEFAULT: Duration = Duration::from_secs(10);
/// Límite de seguridad para el timeout configurable de un hook externo.
pub const TIMEOUT_HOOK_MAX: Duration = Duration::from_secs(60);

/// Declaración serializable de un comando de ciclo de vida. Se mantiene como
/// datos simples para que los consumidores puedan persistirlo sin serializar
/// `Duration` ni introducir shell parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComandoGancho {
    pub comando: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Milisegundos; `0` usa [`TIMEOUT_HOOK_DEFAULT`].
    #[serde(default)]
    pub timeout_ms: u64,
}

impl ComandoGancho {
    #[must_use]
    pub fn a_hook(&self) -> Option<Hook> {
        let comando = self.comando.trim();
        if comando.is_empty() {
            return None;
        }
        let timeout = Duration::from_millis(self.timeout_ms)
            .min(TIMEOUT_HOOK_MAX);
        Some(Hook::comando(
            "pre-compact-configurado",
            EventoHook::PreCompact,
            comando,
            self.args.clone(),
        ).con_timeout(if timeout.is_zero() { TIMEOUT_HOOK_DEFAULT } else { timeout }))
    }
}

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
    /// Eventos cuyo resultado puede vetar la acción en curso. `PreCompact`
    /// también es bloqueable: el runtime debe conservar el historial intacto
    /// cuando el hook devuelve exit 2.
    #[must_use]
    pub fn puede_bloquear(self) -> bool {
        matches!(
            self,
            EventoHook::PreToolUse | EventoHook::PreCompact | EventoHook::PermissionRequest
        )
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
    /// [139A-8 F3n/K4] `true` = hook de sistema construido en código
    /// (notificaciones, …): salta la allowlist de binarios porque el
    /// (comando, args) lo fijó el código, no una config. Cualquier futuro
    /// loader de config (fichero, BD, API) DEBE usar [`Hook::comando`]
    /// (validado en el runner); construir un interno desde input externo
    /// reabriría el RCE que cierra K4.
    pub(crate) interno: bool,
}

impl Hook {
    /// Hook de tipo `command` desde config/externo: el runner valida el
    /// binario contra la allowlist ([`validar_comando_hook`]) antes de
    /// lanzarlo; fuera de lista → error y el hook se salta (fail-closed,
    /// el turno continúa).
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
            interno: false,
        }
    }

    /// Hook de tipo `command` de sistema (código de confianza): sin
    /// allowlist, pero con higiene de entorno y topes igual que el externo.
    /// SOLO para (comando, args) fijados en código; jamás desde input.
    #[must_use]
    pub fn comando_interno(
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
            interno: true,
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
            interno: false,
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

/// [139A-8 F3n/K4] Binarios que un hook EXTERNO (config) puede lanzar.
/// Lectura/inspección + `git`: nada que sea un intérprete (powershell,
/// cmd, sh, python, node…) ni que escale/borre. La config de gancho llega
/// por flags, SQLite y API web (`ComandoGancho`): sin allowlist sería RCE
/// remoto. Extra del operador vía `GLORY_HOOKS_ALLOW` (basenames separados
/// por `,`; documentado, bajo su responsabilidad).
pub const HOOKS_BINARIOS_PERMITIDOS: &[&str] = &[
    "echo", "git", "jq", "yq", "rg", "fd", "cat", "date", "uname", "hostname",
];

/// Tope de argumentos por hook externo (anti-bomba de argv).
pub const MAX_ARGS_HOOK: usize = 32;
/// Tope de bytes totales de argv por hook externo.
pub const MAX_BYTES_ARGS_HOOK: usize = 64 * 1024;

/// Basename normalizado: quita directorios y extensiones Windows;
/// minúsculas en Windows (el FS no distingue). `pub(crate)` para reuso en
/// la allowlist MCP ([`crate::mcp`], K7): misma normalización, distinta lista.
pub(crate) fn normalizar_bin_hook(comando: &str) -> String {
    let base = comando.rsplit(['/', '\\']).next().unwrap_or(comando);
    if cfg!(windows) {
        let lower = base.to_lowercase();
        lower
            .strip_suffix(".exe")
            .or_else(|| lower.strip_suffix(".cmd"))
            .or_else(|| lower.strip_suffix(".bat"))
            .or_else(|| lower.strip_suffix(".ps1"))
            .unwrap_or(&lower)
            .to_string()
    } else {
        base.to_string()
    }
}

/// Extras del operador desde `GLORY_HOOKS_ALLOW` (coma-separados).
fn extras_hooks_desde_env() -> Vec<String> {
    std::env::var("GLORY_HOOKS_ALLOW")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Valida (comando, args) de un hook externo. `extras` = allowlist adicional
/// (el wrapper público la lee de `GLORY_HOOKS_ALLOW`; los tests la inyectan).
pub fn validar_comando_hook_con_extras(
    comando: &str,
    args: &[String],
    extras: &[String],
) -> Result<(), String> {
    let bin = normalizar_bin_hook(comando);
    let permitido = HOOKS_BINARIOS_PERMITIDOS.iter().any(|b| *b == bin)
        || extras.iter().any(|e| normalizar_bin_hook(e) == bin);
    if !permitido {
        return Err(format!(
            "hook denegado ('{comando}'): binario fuera de la allowlist K4 \
             (ampliable con GLORY_HOOKS_ALLOW); los hooks de sistema usan Hook::comando_interno"
        ));
    }
    if args.len() > MAX_ARGS_HOOK {
        return Err(format!(
            "hook denegado ('{comando}'): {} args superan el tope {MAX_ARGS_HOOK}",
            args.len()
        ));
    }
    let bytes: usize = args.iter().map(|a| a.len()).sum();
    if bytes > MAX_BYTES_ARGS_HOOK {
        return Err(format!(
            "hook denegado ('{comando}'): {bytes} bytes de argv superan el tope {MAX_BYTES_ARGS_HOOK}"
        ));
    }
    Ok(())
}

/// Valida con los extras del entorno actual.
pub fn validar_comando_hook(comando: &str, args: &[String]) -> Result<(), String> {
    validar_comando_hook_con_extras(comando, args, &extras_hooks_desde_env())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn comando_gancho_valida_y_limita_timeout() {
        let comando = ComandoGancho {
            comando: "echo".into(),
            args: vec!["ok".into()],
            timeout_ms: 90_000,
        };
        let hook = comando.a_hook().expect("comando válido");
        assert_eq!(hook.evento, EventoHook::PreCompact);
        assert_eq!(hook.timeout, TIMEOUT_HOOK_MAX);
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
    /// intérpretes y escalada, vengan pelados o con ruta.
    #[test]
    fn k4_allowlist_acepta_lectura_y_deniega_interpretes() {
        let sin_args: Vec<String> = vec![];
        for bin in [
            "echo", "git", "jq", "yq", "rg", "fd", "cat", "date", "uname", "hostname",
        ] {
            assert!(
                validar_comando_hook_con_extras(bin, &sin_args, &[]).is_ok(),
                "{bin} debería pasar"
            );
        }
        for bin in [
            "powershell",
            "powershell.exe",
            "cmd",
            "sh",
            "bash",
            "python",
            "python3",
            "node",
            "rm",
            "sudo",
            "curl",
            "wget",
        ] {
            assert!(
                validar_comando_hook_con_extras(bin, &sin_args, &[]).is_err(),
                "{bin} debería denegarse"
            );
        }
    }

    /// [139A-8 F3n/K4] Bypass por ruta absoluta, `..` o extensión: el
    /// basename manda, así que un intérprete no cuela aunque venga con
    /// ruta; y un permitido con ruta absoluta sigue pasando (setups
    /// del operador con `git` fuera del PATH).
    #[test]
    fn k4_bypass_por_ruta_y_extension_denegados() {
        let sin_args: Vec<String> = vec![];
        for bin in [
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            r"C:\Windows\System32\cmd.exe",
            "/bin/sh",
            "/usr/bin/curl",
            "../../bin/python",
            "..\\..\\tools\\sh",
        ] {
            assert!(
                validar_comando_hook_con_extras(bin, &sin_args, &[]).is_err(),
                "{bin} debería denegarse"
            );
        }
        assert!(validar_comando_hook_con_extras("git", &sin_args, &[]).is_ok());
        if cfg!(windows) {
            assert!(validar_comando_hook_con_extras(
                r"C:\Program Files\Git\bin\git.exe",
                &sin_args,
                &[]
            )
            .is_ok());
            assert!(validar_comando_hook_con_extras("ECHO.EXE", &sin_args, &[]).is_ok());
        }
    }

    /// [139A-8 F3n/K4] `GLORY_HOOKS_ALLOW` (aquí inyectado) abre la puerta a
    /// un binario del operador; los topes de argv frenan la bomba de
    /// argumentos.
    #[test]
    fn k4_extras_y_topes_de_argv() {
        let sin_args: Vec<String> = vec![];
        let extras = vec!["mi-notificador".to_string()];
        assert!(validar_comando_hook_con_extras("mi-notificador", &sin_args, &[]).is_err());
        assert!(validar_comando_hook_con_extras("mi-notificador", &sin_args, &extras).is_ok());
        let muchos: Vec<String> = (0..40).map(|i| format!("arg{i}")).collect();
        assert!(validar_comando_hook_con_extras("echo", &muchos, &[]).is_err());
        let gordo = vec!["x".repeat(70 * 1024)];
        assert!(validar_comando_hook_con_extras("echo", &gordo, &[]).is_err());
    }
}
