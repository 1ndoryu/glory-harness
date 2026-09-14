//! [139A-8 F5n/S8] Servicio Git ÚNICO del harness.
//!
//! Causa (auditoría §S8): `desktop/.../proyecto/git.rs` y
//! `cli/.../web_datos/git.rs` duplicaban el mismo contrato (ejecutor con
//! timeout + lectura acotada, parse porcelain `-z`, diffs staged/unstaged,
//! diff de untracked vía `--no-index`, detección de "no es un repositorio").
//! Un fix (p. ej. el timeout o el presupuesto de bytes) se hacía en un sitio
//! y quedaba en el otro.
//!
//! Fix: `ServicioGit` concentra la orquestación; Tauri y web son pasarelas
//! finas (resuelven la raíz de la sesión, llaman a `estado()` y mapean el
//! error a sus códigos). El barrido multi-repo (`arbol_repos`, 139A-2) se
//! CONSERVA en el adaptador desktop: es descubrimiento de la UI, no git.
//!
//! Solo consulta: `status`, `diff`, `branch --show-current`, `rev-parse`.
//! Nunca commit/push/pull/merge/rebase. El hijo no hereda el entorno del
//! operador ([139A-8 F3n/K2]: `aplicar_entorno_minimo`).

use crate::entorno::aplicar_entorno_minimo;
use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};

/// Política del servicio (antes duplicada en desktop + cli con los mismos
/// valores: la duplicación era de código, no de criterio).
pub struct PoliticaGit;

impl PoliticaGit {
    /// Timeout por proceso git.
    pub const TIMEOUT: Duration = Duration::from_secs(5);
    /// Tope de bytes por stream capturado (stdout/stderr).
    pub const MAX_OUTPUT_BYTES: usize = 1024 * 1024;
    /// Tope de archivos untracked con diff incluido.
    pub const MAX_UNTRACKED_FILES: usize = 256;
}

/// Error del servicio, con los códigos estables que ya exponían los
/// adaptadores (el front los distingue; no se renombran).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorGit {
    pub codigo: String,
    pub mensaje: String,
}

impl ErrorGit {
    fn nuevo(codigo: &str, mensaje: impl Into<String>) -> Self {
        Self {
            codigo: codigo.to_string(),
            mensaje: mensaje.into(),
        }
    }
}

impl std::fmt::Display for ErrorGit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.codigo, self.mensaje)
    }
}

impl std::error::Error for ErrorGit {}

/// Entrada del estado Git (`estado` = 2 primeros chars del porcelain).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EntradaGit {
    pub estado: String,
    pub ruta: String,
}

/// Estado Git de un workspace/repo (incluye `rama` [139A-2]; el adaptador
/// web la ignora: su contrato HTTP no la declara).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct EstadoGit {
    pub aplicable: bool,
    pub raiz: Option<String>,
    pub entradas: Vec<EntradaGit>,
    pub diff: String,
    pub truncado: bool,
    pub mensaje: Option<String>,
    pub diff_unstaged: Option<String>,
    pub diff_staged: Option<String>,
    /// Rama actual (`branch --show-current`; `HEAD@<hash>` si detached;
    /// `None` si no se pudo resolver).
    pub rama: Option<String>,
}

/// Servicio Git sobre una raíz canónica (el adaptador la resuelve desde la
/// sesión; el servicio no conoce sesiones ni workspaces).
pub struct ServicioGit {
    raiz: PathBuf,
}

impl ServicioGit {
    /// Crea el servicio sobre `raiz` (ya canonicalizada por el adaptador).
    pub fn nuevo(raiz: impl Into<PathBuf>) -> Self {
        Self { raiz: raiz.into() }
    }

    /// Estado completo: status porcelain + diffs + untracked + rama.
    pub async fn estado(&self) -> Result<EstadoGit, ErrorGit> {
        estado_en(&self.raiz).await
    }

    /// Rama actual, sin el resto del estado (para pasarelas parciales).
    pub async fn rama(&self) -> Option<String> {
        rama_en(&self.raiz).await
    }
}

struct ResultadoProceso {
    codigo: Option<i32>,
    salida: Vec<u8>,
    error: Vec<u8>,
    truncado: bool,
}

async fn estado_en(raiz: &Path) -> Result<EstadoGit, ErrorGit> {
    let status = ejecutar_git(
        raiz,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )
    .await?;
    if status.codigo != Some(0) {
        let mensaje = texto(&status.error);
        if es_error_no_repo(&mensaje) {
            return Ok(EstadoGit {
                aplicable: false,
                raiz: Some(normalizar_ruta(raiz)),
                entradas: Vec::new(),
                diff: String::new(),
                truncado: false,
                mensaje: Some("el workspace no es un repositorio Git".to_string()),
                diff_unstaged: None,
                diff_staged: None,
                rama: None,
            });
        }
        return Err(ErrorGit::nuevo(
            "git_status_fallo",
            if mensaje.is_empty() {
                "git status falló".to_string()
            } else {
                mensaje
            },
        ));
    }

    let diff_unstaged = ejecutar_git(raiz, &["diff", "--no-ext-diff", "--unified=3"]).await?;
    let diff_staged =
        ejecutar_git(raiz, &["diff", "--no-ext-diff", "--unified=3", "--cached"]).await?;
    let presupuesto_untracked =
        PoliticaGit::MAX_OUTPUT_BYTES.saturating_sub(diff_unstaged.salida.len());
    let untracked = archivos_no_rastreados(raiz, &status.salida, presupuesto_untracked).await?;
    if diff_unstaged.codigo != Some(0) {
        return Err(ErrorGit::nuevo(
            "git_diff_fallo",
            texto(&diff_unstaged.error),
        ));
    }
    if diff_staged.codigo != Some(0) {
        return Err(ErrorGit::nuevo(
            "git_diff_cached_fallo",
            texto(&diff_staged.error),
        ));
    }

    Ok(EstadoGit {
        aplicable: true,
        raiz: Some(normalizar_ruta(raiz)),
        entradas: parsear_status(&status.salida),
        diff: format!("{}{}", texto(&diff_unstaged.salida), untracked.diff),
        truncado: status.truncado
            || diff_unstaged.truncado
            || diff_staged.truncado
            || untracked.truncado,
        mensaje: None,
        diff_staged: Some(texto(&diff_staged.salida)),
        diff_unstaged: Some(format!(
            "{}{}",
            texto(&diff_unstaged.salida),
            untracked.diff
        )),
        rama: rama_en(raiz).await,
    })
}

/// ¿El stderr de `git status` indica "no es un repositorio" (EN + ES)?
fn es_error_no_repo(mensaje: &str) -> bool {
    mensaje.contains("not a git repository") || mensaje.contains("no es un repositorio git")
}

struct UntrackedDiff {
    diff: String,
    truncado: bool,
}

async fn archivos_no_rastreados(
    raiz: &Path,
    status: &[u8],
    max_bytes: usize,
) -> Result<UntrackedDiff, ErrorGit> {
    let mut diff = String::new();
    let mut truncado = false;
    let mut archivos = 0;
    for registro in status
        .split(|byte| *byte == 0)
        .filter(|registro| registro.starts_with(b"?? "))
    {
        if archivos >= PoliticaGit::MAX_UNTRACKED_FILES {
            truncado = true;
            break;
        }
        let ruta = String::from_utf8_lossy(&registro[3..]).replace('\\', "/");
        if ruta.ends_with('/') || !ruta_valida(&ruta) {
            continue;
        }
        archivos += 1;
        let argumentos = [
            "diff",
            "--no-ext-diff",
            "--no-index",
            "--unified=3",
            "/dev/null",
            ruta.as_str(),
        ];
        let resultado = ejecutar_git(raiz, &argumentos).await?;
        if resultado.codigo != Some(1) {
            return Err(ErrorGit::nuevo(
                "git_diff_untracked_fallo",
                texto(&resultado.error),
            ));
        }
        let disponible = max_bytes.saturating_sub(diff.len());
        if resultado.salida.len() > disponible {
            diff.push_str(&String::from_utf8_lossy(&resultado.salida[..disponible]));
            truncado = true;
            break;
        }
        diff.push_str(&texto(&resultado.salida));
        if resultado.truncado {
            truncado = true;
            break;
        }
    }
    Ok(UntrackedDiff { diff, truncado })
}

/// Solo componentes normales: el path viene del porcelain de git, pero el
/// `--no-index` lo convierte en argumento de proceso (defensa en
/// profundidad: sin `..`, absolutos ni NUL).
fn ruta_valida(ruta: &str) -> bool {
    !ruta.is_empty()
        && Path::new(ruta)
            .components()
            .all(|componente| matches!(componente, Component::Normal(_)))
}

/// Rama actual sin recortar: `branch --show-current`; si está vacío
/// (detached), `HEAD@<7>`; si tampoco hay HEAD (sin commits), `None`.
async fn rama_en(raiz: &Path) -> Option<String> {
    let actual = ejecutar_git(raiz, &["branch", "--show-current"])
        .await
        .ok()?;
    let nombre = texto(&actual.salida).trim().to_string();
    if !nombre.is_empty() {
        return Some(nombre);
    }
    let head = ejecutar_git(raiz, &["rev-parse", "--short", "HEAD"])
        .await
        .ok()?;
    let hash = texto(&head.salida).trim().to_string();
    if head.codigo == Some(0) && !hash.is_empty() {
        return Some(format!("HEAD@{hash}"));
    }
    None
}

async fn ejecutar_git(raiz: &Path, argumentos: &[&str]) -> Result<ResultadoProceso, ErrorGit> {
    let mut spawn = tokio::process::Command::new("git");
    spawn
        .args(argumentos)
        .current_dir(raiz)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    // [139A-8 F3n/K2] El hijo no hereda el entorno del operador (claves LLM).
    aplicar_entorno_minimo(&mut spawn);
    let mut proceso = spawn.spawn().map_err(|e| {
        ErrorGit::nuevo("git_no_disponible", format!("no se pudo iniciar git: {e}"))
    })?;

    let salida = proceso.stdout.take().ok_or_else(|| {
        ErrorGit::nuevo("git_salida_no_disponible", "git no expuso stdout")
    })?;
    let error = proceso.stderr.take().ok_or_else(|| {
        ErrorGit::nuevo("git_salida_no_disponible", "git no expuso stderr")
    })?;

    let resultado = tokio::time::timeout(PoliticaGit::TIMEOUT, async {
        let (salida, error, estado) =
            tokio::join!(leer_limitado(salida), leer_limitado(error), proceso.wait());
        let estado = estado
            .map_err(|e| ErrorGit::nuevo("git_espera_fallo", e.to_string()))?;
        Ok::<_, ErrorGit>(ResultadoProceso {
            codigo: estado.code(),
            salida: salida.0,
            error: error.0,
            truncado: salida.1 || error.1,
        })
    })
    .await
    .map_err(|_| {
        ErrorGit::nuevo(
            "git_timeout",
            format!(
                "Git superó el límite de {} segundos",
                PoliticaGit::TIMEOUT.as_secs()
            ),
        )
    })??;
    Ok(resultado)
}

async fn leer_limitado<R: AsyncRead + Unpin>(mut lector: R) -> (Vec<u8>, bool) {
    let mut bytes = Vec::with_capacity(PoliticaGit::MAX_OUTPUT_BYTES.min(8192));
    let mut bloque = [0_u8; 8192];
    let mut truncado = false;
    loop {
        match lector.read(&mut bloque).await {
            Ok(0) => break,
            Ok(n) => {
                let disponible = PoliticaGit::MAX_OUTPUT_BYTES.saturating_sub(bytes.len());
                if n > disponible {
                    bytes.extend_from_slice(&bloque[..disponible]);
                    truncado = true;
                } else if disponible > 0 {
                    bytes.extend_from_slice(&bloque[..n]);
                } else {
                    truncado = true;
                }
            }
            Err(_) => break,
        }
    }
    (bytes, truncado)
}

/// Parsea el porcelain `-z` a entradas (`estado` + `ruta` con `/`).
fn parsear_status(bytes: &[u8]) -> Vec<EntradaGit> {
    let mut entradas = Vec::new();
    for registro in bytes
        .split(|byte| *byte == 0)
        .filter(|registro| !registro.is_empty())
    {
        if registro.len() < 4 {
            continue;
        }
        let estado = String::from_utf8_lossy(&registro[..2]).into_owned();
        let ruta = String::from_utf8_lossy(&registro[3..]).replace('\\', "/");
        entradas.push(EntradaGit { estado, ruta });
    }
    entradas
}

fn texto(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn normalizar_ruta(dir: &Path) -> String {
    dir.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::{es_error_no_repo, parsear_status, ruta_valida};

    #[test]
    fn parsea_status_porcelain_nul() {
        let entradas = parsear_status(b" M src/main.rs\0?? nuevo.txt\0");
        assert_eq!(entradas.len(), 2);
        assert_eq!(entradas[0].estado, " M");
        assert_eq!(entradas[0].ruta, "src/main.rs");
        assert_eq!(entradas[1].estado, "??");
    }

    #[test]
    fn detecta_no_repo_en_ingles_y_espanol() {
        assert!(es_error_no_repo(
            "fatal: not a git repository (or any of the parent directories)"
        ));
        assert!(es_error_no_repo("fatal: no es un repositorio git"));
        assert!(!es_error_no_repo("fatal: bad revision 'HEAD'"));
    }

    #[test]
    fn rechaza_rutas_no_normales_para_diff() {
        assert!(ruta_valida("src/main.rs"));
        assert!(!ruta_valida(""));
        assert!(!ruta_valida("../fuera.txt"));
        assert!(!ruta_valida("/absoluta.txt"));
    }
}
