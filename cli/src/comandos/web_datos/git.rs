//! [089A-10] Estado Git local del workspace activo por HTTP (modo web).
//!
//! Espejo web de `desktop/src-tauri/src/git.rs` (089A-9): solo consulta
//! `git.exe` (sin commit/push/pull/merge/rebase); el cwd se resuelve en el
//! backend desde `comun.workspace` y la salida está acotada.

use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Path as Ruta, State},
    http::{HeaderMap, Method},
    Json,
};
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use super::{area_activa, error, sesion_y_comun, ApiError, AppState};
use crate::servicio::SesionComun;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_OUTPUT_BYTES: usize = 1024 * 1024;

/// [089A-9] Entrada del estado Git (`estado` = 2 primeros chars porcelain).
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

/// `GET /api/v1/session/{id}/git/estado` — estado Git del workspace activo.
pub(crate) async fn git_estado(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
) -> Result<Json<EstadoGit>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    let status = ejecutar_git(&raiz, &["status", "--porcelain=v1", "-z"]).await?;
    if status.codigo != Some(0) {
        let mensaje = texto(&status.error);
        if mensaje.contains("not a git repository") || mensaje.contains("no es un repositorio git") {
            return Ok(Json(EstadoGit {
                aplicable: false,
                raiz: Some(raiz.to_string_lossy().replace('\\', "/")),
                entradas: Vec::new(),
                diff: String::new(),
                truncado: false,
                mensaje: Some("el workspace no es un repositorio Git".to_string()),
            }));
        }
        return Err(error(
            "git_status_fallo",
            if mensaje.is_empty() {
                "git status falló".to_string()
            } else {
                mensaje
            },
        ));
    }

    let diff = ejecutar_git(&raiz, &["diff", "--no-ext-diff", "--unified=3"]).await?;
    if diff.codigo != Some(0) {
        return Err(error("git_diff_fallo", texto(&diff.error)));
    }

    Ok(Json(EstadoGit {
        aplicable: true,
        raiz: Some(raiz.to_string_lossy().replace('\\', "/")),
        entradas: parsear_status(&status.salida),
        diff: texto(&diff.salida),
        truncado: status.truncado || diff.truncado,
        mensaje: None,
    }))
}

fn raiz_activa(comun: &SesionComun) -> Result<std::path::PathBuf, ApiError> {
    let workspace = area_activa(comun)?
        .ok_or_else(|| error("workspace_no_configurado", "elige un workspace antes de consultar Git"))?;
    let raiz = std::path::PathBuf::from(workspace.ruta);
    let canon = raiz.canonicalize().map_err(|e| error("workspace_invalido", e.to_string()))?;
    if !canon.is_dir() {
        return Err(error("workspace_invalido", "el workspace activo no es una carpeta"));
    }
    Ok(canon)
}

async fn ejecutar_git(raiz: &Path, argumentos: &[&str]) -> Result<ResultadoProceso, ApiError> {
    let mut proceso = Command::new("git")
        .args(argumentos)
        .current_dir(raiz)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| error("git_no_disponible", format!("no se pudo iniciar git: {e}")))?;

    let salida = proceso
        .stdout
        .take()
        .ok_or_else(|| error("git_salida_no_disponible", "git no expuso stdout"))?;
    let stderr = proceso
        .stderr
        .take()
        .ok_or_else(|| error("git_salida_no_disponible", "git no expuso stderr"))?;

    let resultado = tokio::time::timeout(GIT_TIMEOUT, async {
        let (salida, err_bytes, estado) =
            tokio::join!(leer_limitado(salida), leer_limitado(stderr), proceso.wait());
        let estado = estado.map_err(|e| error("git_espera_fallo", e.to_string()))?;
        Ok::<_, ApiError>(ResultadoProceso {
            codigo: estado.code(),
            salida: salida.0,
            error: err_bytes.0,
            truncado: salida.1 || err_bytes.1,
        })
    })
    .await
    .map_err(|_| {
        error(
            "git_timeout",
            format!("Git superó el límite de {} segundos", GIT_TIMEOUT.as_secs()),
        )
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
