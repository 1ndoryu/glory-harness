//! Estado Git local del workspace activo (089A-9).
//!
//! Git es una fuente independiente del filesystem y de los cambios de tools.
//! Este MVP solo consulta `git.exe`: no hace commit, push, pull, merge ni
//! rebase. El cwd se resuelve en backend y la salida está acotada.

use serde::Serialize;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tauri::State;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::{area_activa, sesion_actual, Estado, Sesion};

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ErrorGit {
    pub codigo: String,
    pub mensaje: String,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct EntradaGit {
    pub estado: String,
    pub ruta: String,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct EstadoGit {
    pub aplicable: bool,
    pub raiz: Option<String>,
    pub entradas: Vec<EntradaGit>,
    pub diff: String,
    pub truncado: bool,
    pub mensaje: Option<String>,
}

struct ResultadoProceso {
    codigo: Option<i32>,
    salida: Vec<u8>,
    error: Vec<u8>,
    truncado: bool,
}

#[tauri::command]
pub(crate) async fn workspace_git_estado(
    estado: State<'_, Estado>,
) -> Result<EstadoGit, ErrorGit> {
    let sesion = sesion_actual(&estado).map_err(|mensaje| ErrorGit {
        codigo: "sesion".to_string(),
        mensaje,
    })?;
    let raiz = raiz_activa(&sesion)?;
    let status = ejecutar_git(&raiz, &["status", "--porcelain=v1", "-z"]).await?;
    if status.codigo != Some(0) {
        let mensaje = texto(&status.error);
        if mensaje.contains("not a git repository") || mensaje.contains("no es un repositorio git") {
            return Ok(EstadoGit {
                aplicable: false,
                raiz: Some(raiz.to_string_lossy().replace('\\', "/")),
                entradas: Vec::new(),
                diff: String::new(),
                truncado: false,
                mensaje: Some("el workspace no es un repositorio Git".to_string()),
            });
        }
        return Err(ErrorGit {
            codigo: "git_status_fallo".to_string(),
            mensaje: if mensaje.is_empty() { "git status falló".to_string() } else { mensaje },
        });
    }

    let diff = ejecutar_git(&raiz, &["diff", "--no-ext-diff", "--unified=3"]).await?;
    if diff.codigo != Some(0) {
        return Err(ErrorGit {
            codigo: "git_diff_fallo".to_string(),
            mensaje: texto(&diff.error),
        });
    }

    Ok(EstadoGit {
        aplicable: true,
        raiz: Some(raiz.to_string_lossy().replace('\\', "/")),
        entradas: parsear_status(&status.salida),
        diff: texto(&diff.salida),
        truncado: status.truncado || diff.truncado,
        mensaje: None,
    })
}

fn raiz_activa(sesion: &Sesion) -> Result<std::path::PathBuf, ErrorGit> {
    let workspace = area_activa(sesion).map_err(|mensaje| ErrorGit {
        codigo: "workspace_invalido".to_string(),
        mensaje,
    })?.ok_or_else(|| ErrorGit {
        codigo: "workspace_no_configurado".to_string(),
        mensaje: "elige un workspace antes de consultar Git".to_string(),
    })?;
    let raiz = std::path::PathBuf::from(workspace.ruta);
    let canon = raiz.canonicalize().map_err(|e| ErrorGit {
        codigo: "workspace_invalido".to_string(),
        mensaje: e.to_string(),
    })?;
    if !canon.is_dir() {
        return Err(ErrorGit {
            codigo: "workspace_invalido".to_string(),
            mensaje: "el workspace activo no es una carpeta".to_string(),
        });
    }
    Ok(canon)
}

async fn ejecutar_git(raiz: &Path, argumentos: &[&str]) -> Result<ResultadoProceso, ErrorGit> {
    let mut proceso = Command::new("git")
        .args(argumentos)
        .current_dir(raiz)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| ErrorGit {
            codigo: "git_no_disponible".to_string(),
            mensaje: format!("no se pudo iniciar git: {e}"),
        })?;

    let salida = proceso.stdout.take().ok_or_else(|| ErrorGit {
        codigo: "git_salida_no_disponible".to_string(),
        mensaje: "git no expuso stdout".to_string(),
    })?;
    let error = proceso.stderr.take().ok_or_else(|| ErrorGit {
        codigo: "git_salida_no_disponible".to_string(),
        mensaje: "git no expuso stderr".to_string(),
    })?;

    let resultado = tokio::time::timeout(GIT_TIMEOUT, async {
        let (salida, error, estado) = tokio::join!(leer_limitado(salida), leer_limitado(error), proceso.wait());
        let estado = estado.map_err(|e| ErrorGit {
            codigo: "git_espera_fallo".to_string(),
            mensaje: e.to_string(),
        })?;
        Ok::<_, ErrorGit>(ResultadoProceso {
            codigo: estado.code(),
            salida: salida.0,
            error: error.0,
            truncado: salida.1 || error.1,
        })
    })
    .await
    .map_err(|_| ErrorGit {
        codigo: "git_timeout".to_string(),
        mensaje: format!("Git superó el límite de {} segundos", GIT_TIMEOUT.as_secs()),
    })??;
    Ok(resultado)
}

async fn leer_limitado<R: AsyncRead + Unpin>(mut lector: R) -> (Vec<u8>, bool) {
    let mut bytes = Vec::with_capacity(MAX_OUTPUT_BYTES.min(8192));
    let mut bloque = [0_u8; 8192];
    let mut truncado = false;
    loop {
        match lector.read(&mut bloque).await {
            Ok(0) => break,
            Ok(n) => {
                let disponible = MAX_OUTPUT_BYTES.saturating_sub(bytes.len());
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

fn parsear_status(bytes: &[u8]) -> Vec<EntradaGit> {
    let mut entradas = Vec::new();
    for registro in bytes.split(|byte| *byte == 0).filter(|registro| !registro.is_empty()) {
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

#[cfg(test)]
mod tests {
    use super::parsear_status;

    #[test]
    fn parsea_status_porcelain_nul() {
        let entradas = parsear_status(b" M src/main.rs\0?? nuevo.txt\0");
        assert_eq!(entradas.len(), 2);
        assert_eq!(entradas[0].estado, " M");
        assert_eq!(entradas[0].ruta, "src/main.rs");
        assert_eq!(entradas[1].estado, "??");
    }
}
