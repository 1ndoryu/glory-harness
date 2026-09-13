//! [139A-8 F3n/K2] Higiene de entorno para procesos hijo.
//!
//! Causa (auditoría §K2, ALTA): los hijos se lanzaban con el entorno
//! heredado, incluidas las claves LLM del operador (`OPENAI_API_KEY`,
//! `GLORY_*`, …). Un `env`/`printenv`/`set` del modelo o un MCP/hook
//! malicioso las leía sin más.
//!
//! Fix: `env_clear()` + allowlist explícita en el punto de spawn. La lista
//! es el mínimo para que el hijo funcione (PATH, dirs de sistema/usuario,
//! locale) + prefijos de toolchains (`CARGO_*`, `RUSTUP_*`, sin secretos por
//! diseño). Cambio de conducta documentado: el hijo YA NO ve el entorno del
//! operador (proxies, configs de usuario, claves); lo que necesite debe
//! viajar como argumento o variable nombrada explícita.
//!
//! Nota Windows: el entorno es case-insensitive (`Path` vs `PATH`); la
//! comparación lo es también (solo en Windows; en Unix exacta).

use tokio::process::Command;

/// Variables que el hijo conserva (resto = se descarta).
pub const VARS_ENTORNO_MINIMO: &[&str] = &[
    "PATH",
    "PATHEXT",
    "COMSPEC",
    "OS",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "PROGRAMDATA",
    "TEMP",
    "TMP",
    "TMPDIR",
    "HOME",
    "USERPROFILE",
    "APPDATA",
    "LOCALAPPDATA",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "LC_MESSAGES",
    "TZ",
    "TERM",
    "CI",
];

/// Prefijos que pasan íntegros (toolchains; sin secretos por diseño).
pub const PREFIJOS_ENTORNO_PERMITIDOS: &[&str] = &["CARGO_", "RUSTUP_"];

fn nombre_igual(a: &str, b: &str) -> bool {
    if cfg!(windows) {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

fn prefijo_vale(nombre: &str, prefijo: &str) -> bool {
    if cfg!(windows) {
        nombre.len() >= prefijo.len() && nombre[..prefijo.len()].eq_ignore_ascii_case(prefijo)
    } else {
        nombre.starts_with(prefijo)
    }
}

fn var_permitida(nombre: &str) -> bool {
    VARS_ENTORNO_MINIMO.iter().any(|v| nombre_igual(nombre, v))
        || PREFIJOS_ENTORNO_PERMITIDOS
            .iter()
            .any(|p| prefijo_vale(nombre, p))
}

/// Filtra un snapshot `(nombre, valor)`: testeable sin tocar el entorno del
/// proceso (los tests no mutan `std::env`, global del proceso).
#[must_use]
pub fn filtrar_entorno(vars: impl Iterator<Item = (String, String)>) -> Vec<(String, String)> {
    vars.filter(|(nombre, _)| var_permitida(nombre)).collect()
}

/// `env_clear()` + reinyecta solo el subconjunto permitido, leído del
/// entorno actual. Llamar SIEMPRE antes de `spawn()` en hijos del harness
/// (jaula, hooks, MCP).
pub fn aplicar_entorno_minimo(cmd: &mut Command) {
    let limpias = filtrar_entorno(std::env::vars());
    cmd.env_clear();
    for (nombre, valor) in limpias {
        cmd.env(nombre, valor);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pares: &[(&str, &str)]) -> Vec<(String, String)> {
        pares
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn filtra_claves_y_conserva_lo_minimo() {
        let entrada = vars(&[
            ("PATH", "/bin"),
            ("OPENAI_API_KEY", "sk-secreta"),
            ("GLORY_HARNESS_SESION_SECRETO", "s3cr3t"),
            ("GH_MASTER_TOKEN", "tok"),
            ("CARGO_HOME", "/opt/cargo"),
            ("RUSTUP_TOOLCHAIN", "stable"),
            ("PATH_EXTRA", "/sospechoso"),
            ("HTTP_PROXY", "http://proxy:8080"),
            ("MI_CLAVE", "x"),
        ]);
        let salida = filtrar_entorno(entrada.into_iter());
        let nombres: Vec<&str> = salida.iter().map(|(k, _)| k.as_str()).collect();
        assert!(nombres.contains(&"PATH"), "PATH debe pasar: {nombres:?}");
        assert!(
            nombres.contains(&"CARGO_HOME"),
            "prefijo CARGO_ debe pasar: {nombres:?}"
        );
        assert!(
            nombres.contains(&"RUSTUP_TOOLCHAIN"),
            "prefijo RUSTUP_ debe pasar: {nombres:?}"
        );
        for secreta in [
            "OPENAI_API_KEY",
            "GLORY_HARNESS_SESION_SECRETO",
            "GH_MASTER_TOKEN",
            "MI_CLAVE",
        ] {
            assert!(
                !nombres.contains(&secreta),
                "{secreta} debe filtrarse: {nombres:?}"
            );
        }
        // Coincidencia exacta en escalares: el prefijo NO abre la puerta.
        assert!(
            !nombres.contains(&"PATH_EXTRA"),
            "PATH_EXTRA no es PATH: {nombres:?}"
        );
        assert!(
            !nombres.contains(&"HTTP_PROXY"),
            "el proxy del operador no se hereda: {nombres:?}"
        );
    }

    #[test]
    fn conserva_los_valores_sin_modificar() {
        let salida = filtrar_entorno(vars(&[("PATH", "a;b;c")]).into_iter());
        assert_eq!(salida, vec![("PATH".to_string(), "a;b;c".to_string())]);
    }

    /// Dumper de entorno del hijo: `set` vía cmd (Windows) o `env` (Unix).
    #[cfg(windows)]
    fn dumper() -> (&'static str, &'static [&'static str]) {
        ("cmd", &["/C", "set"])
    }
    #[cfg(not(windows))]
    fn dumper() -> (&'static str, &'static [&'static str]) {
        ("env", &[])
    }

    /// [139A-8 F3n/K2] El hijo ve SOLO la allowlist: un señuelo inyectado en
    /// el `Command` ANTES de `aplicar_entorno_minimo` desaparece (`env_clear`
    /// lo barre) y `PATH` sobrevive (el hijo sigue funcional). Sin mutar el
    /// entorno del proceso de test (los tests comparten proceso).
    #[tokio::test]
    async fn hijo_ve_solo_la_allowlist() {
        let (bin, args) = dumper();
        let mut cmd = Command::new(bin);
        cmd.args(args);
        cmd.env("GLORY_HARNESS_SENUELLO_K2", "debe-desaparecer");
        aplicar_entorno_minimo(&mut cmd);
        cmd.stdout(std::process::Stdio::piped());
        let salida = cmd.output().await.expect("dumper de entorno");
        assert!(salida.status.success(), "el dumper debe terminar con 0");
        let texto = String::from_utf8_lossy(&salida.stdout);
        assert!(
            !texto.contains("GLORY_HARNESS_SENUELLO_K2"),
            "el señuelo no debe llegar al hijo"
        );
        assert!(
            texto.to_uppercase().contains("PATH="),
            "PATH debe sobrevivir (hijo funcional)"
        );
    }
}
