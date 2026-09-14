//! Estado Git local del workspace activo (089A-9).
//!
//! [139A-8 F5n/S8] Pasarela fina: el estado lo calcula `ServicioGit` del
//! core (única orquestación); aquí solo resolución de sesión/raíz, comandos
//! Tauri y descubrimiento multi-repo. Git es una fuente independiente del
//! filesystem y de los cambios de tools. Este MVP solo consulta `git.exe`:
//! no hace commit, push, pull, merge ni rebase. El cwd se resuelve en
//! backend y la salida está acotada.

use serde::Serialize;
use std::path::Path;
use tauri::State;

use glory_harness_core::{ErrorGit, EstadoGit, ServicioGit};

// [109A-6] El módulo vive en `proyecto/`: `super` ya no es la raíz del crate.
use crate::{area_activa, sesion_actual, Estado, Sesion};

/// Un repositorio descubierto bajo el área activa. [139A-2]
#[derive(Debug, Serialize, Clone)]
pub(crate) struct RepoGit {
    /// Nombre de la carpeta del repo.
    pub nombre: String,
    /// Ruta canónica con `/` como separador.
    pub ruta: String,
    /// Ruta del repo relativa al área (`""` = el área misma): el front la
    /// usa para atribuir cada cambio del vault a su sección sin duplicar.
    pub prefijo: String,
}

/// Carpetas en las que nunca se entra al descubrir repos. [139A-2]
const IGNORADOS_DESCUBRIR: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    "tmp",
    "Temp",
    "__pycache__",
    ".venv",
    ".cargo",
];
/// Tope de repos devueltos: el frente pide un estado por repo y cada uno
/// cuesta 2-3 procesos git. [139A-2]
const MAX_REPOS: usize = 20;
/// Profundidad por defecto del barrido (carpetas dentro del área). [139A-2]
const PROFUNDIDAD_DEFECTO: u8 = 2;

#[tauri::command]
pub(crate) async fn workspace_git_estado(estado: State<'_, Estado>) -> Result<EstadoGit, ErrorGit> {
    let sesion = sesion_actual(&estado).map_err(|mensaje| ErrorGit {
        codigo: "sesion".to_string(),
        mensaje,
    })?;
    let raiz = raiz_activa(&sesion)?;
    ServicioGit::nuevo(raiz).estado().await
}

/// Estado git de un repo concreto descubierto por `workspace_git_repos`.
/// La ruta debe vivir dentro del área activa (contención: no se consulta
/// git fuera del workspace). [139A-2]
#[tauri::command]
pub(crate) async fn workspace_git_estado_en(
    estado: State<'_, Estado>,
    ruta: String,
) -> Result<EstadoGit, ErrorGit> {
    let sesion = sesion_actual(&estado).map_err(|mensaje| ErrorGit {
        codigo: "sesion".to_string(),
        mensaje,
    })?;
    let base = raiz_activa(&sesion)?;
    let candidata = Path::new(&ruta)
        .canonicalize()
        .map_err(|e| ErrorGit {
            codigo: "ruta_invalida".to_string(),
            mensaje: e.to_string(),
        })?;
    if !candidata.starts_with(&base) || !candidata.is_dir() {
        return Err(ErrorGit {
            codigo: "ruta_fuera_del_area".to_string(),
            mensaje: "el repo debe estar dentro del área activa".to_string(),
        });
    }
    ServicioGit::nuevo(candidata).estado().await
}

/// Repos git bajo el área activa, barrido descendente. `profundidad`
/// (`None` = 2): cuántos niveles de carpetas se rastrean. Los repos
/// anidados se colapsan al más externo (no se entra en ellos). [139A-2]
#[tauri::command]
pub(crate) async fn workspace_git_repos(
    estado: State<'_, Estado>,
    profundidad: Option<u8>,
) -> Result<Vec<RepoGit>, ErrorGit> {
    let sesion = sesion_actual(&estado).map_err(|mensaje| ErrorGit {
        codigo: "sesion".to_string(),
        mensaje,
    })?;
    let raiz = raiz_activa(&sesion)?;
    let niveles = profundidad.unwrap_or(PROFUNDIDAD_DEFECTO).min(5);
    Ok(descubrir_repos(&raiz, niveles))
}

/// Resumen batch: descubrir + estado por repo en UNA llamada (1 IPC en vez
/// de 1+N). Secuencial v1, sin caché de backend (sin watcher que invalide:
/// el front decide con firmas + single-flight). El repo ilegible no aborta
/// el lote: viaja con `est: None` + `error` y el front lo oculta igual que
/// en el fan-out. [139A-7]
#[derive(Debug, Serialize, Clone)]
pub(crate) struct ResumenRepo {
    pub repo: RepoGit,
    /// `None` = el estado falló (ver `error`). Espeja el `ResumenRepo` del
    /// front campo a campo para no mapear en el transporte.
    pub est: Option<EstadoGit>,
    pub error: Option<String>,
}

#[tauri::command]
pub(crate) async fn workspace_git_resumen(
    estado: State<'_, Estado>,
    profundidad: Option<u8>,
) -> Result<Vec<ResumenRepo>, ErrorGit> {
    let sesion = sesion_actual(&estado).map_err(|mensaje| ErrorGit {
        codigo: "sesion".to_string(),
        mensaje,
    })?;
    let raiz = raiz_activa(&sesion)?;
    let niveles = profundidad.unwrap_or(PROFUNDIDAD_DEFECTO).min(5);
    let mut lote = Vec::new();
    for dir in rutas_repos(&raiz, niveles) {
        let repo = describir_repo(&raiz, &dir);
        match ServicioGit::nuevo(&dir).estado().await {
            Ok(est) => lote.push(ResumenRepo {
                repo,
                est: Some(est),
                error: None,
            }),
            Err(fallo) => lote.push(ResumenRepo {
                repo,
                est: None,
                error: Some(fallo.mensaje),
            }),
        }
    }
    Ok(lote)
}

/// Descripción front de un repo descubierto. [139A-2]
fn describir_repo(raiz: &Path, dir: &Path) -> RepoGit {
    RepoGit {
        nombre: nombre_carpeta(dir),
        ruta: normalizar_ruta(dir),
        prefijo: prefijo_en(raiz, dir),
    }
}

/// Rutas de repos (mismo orden que `descubrir_repos` daba: por ruta
/// normalizada). Núcleo compartido por `workspace_git_repos` y
/// `workspace_git_resumen`. [139A-7]
fn rutas_repos(raiz: &Path, niveles: u8) -> Vec<std::path::PathBuf> {
    let mut rutas = Vec::new();
    let mut pendientes = vec![(raiz.to_path_buf(), 0u8)];
    while let Some((dir, nivel)) = pendientes.pop() {
        if rutas.len() >= MAX_REPOS {
            break;
        }
        if es_repo(&dir) {
            rutas.push(dir);
            continue;
        }
        if nivel >= niveles {
            continue;
        }
        let Ok(hijos) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut subdirs: Vec<std::path::PathBuf> = hijos
            .filter_map(|h| h.ok().map(|e| e.path()))
            .filter(|p| p.is_dir() && !ignorado(p))
            .collect();
        subdirs.sort();
        for sub in subdirs {
            pendientes.push((sub, nivel + 1));
        }
    }
    rutas.sort_by_key(|p| normalizar_ruta(p));
    rutas
}

/// Barrido en anchura: si la carpeta es repo se registra y no se desciende
/// (colapso de anidados); si no, se desciende hasta `niveles`. [139A-2]
fn descubrir_repos(raiz: &Path, niveles: u8) -> Vec<RepoGit> {
    rutas_repos(raiz, niveles)
        .iter()
        .map(|dir| describir_repo(raiz, dir))
        .collect()
}

fn es_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

fn ignorado(dir: &Path) -> bool {
    dir.file_name()
        .and_then(|n| n.to_str())
        .map(|n| IGNORADOS_DESCUBRIR.contains(&n) || n.starts_with('.'))
        .unwrap_or(true)
}

fn nombre_carpeta(dir: &Path) -> String {
    dir.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_string()
}

fn normalizar_ruta(dir: &Path) -> String {
    dir.to_string_lossy().replace('\\', "/")
}

/// Prefijo del repo relativo al área (`""` si es el área misma). [139A-2]
fn prefijo_en(raiz: &Path, dir: &Path) -> String {
    dir.strip_prefix(raiz)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_default()
}

fn raiz_activa(sesion: &Sesion) -> Result<std::path::PathBuf, ErrorGit> {
    let workspace = area_activa(sesion)
        .map_err(|mensaje| ErrorGit {
            codigo: "workspace_invalido".to_string(),
            mensaje,
        })?
        .ok_or_else(|| ErrorGit {
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

#[cfg(test)]
mod tests {
    use super::descubrir_repos;
    use std::path::PathBuf;

    /// Árbol temporal: repoA (.git) con anidado (colapsa), repoB, carpeta
    /// sin repo con repo profundo (nivel 3, fuera con profundidad 2) y
    /// `node_modules` con `.git` (ignorado). [139A-2] El sufijo evita que
    /// dos tests en hilos paralelos compartan el mismo temporal.
    fn arbol_repos(sufijo: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("gh-repos-{}-{sufijo}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for dir in [
            "repoA/.git",
            "repoA/tools/sub/.git",
            "repoB/.git",
            "suelto/hondo/repoC/.git",
            "node_modules/falso/.git",
            "target/falso/.git",
        ] {
            std::fs::create_dir_all(base.join(dir)).unwrap();
        }
        base
    }

    #[test]
    fn descubre_repos_colapsa_anidados_e_ignora() {
        let base = arbol_repos("colapsa");
        let repos = descubrir_repos(&base, 2);
        let rutas: Vec<&str> = repos.iter().map(|r| r.ruta.as_str()).collect();
        let a = base.join("repoA").to_string_lossy().replace('\\', "/");
        let b = base.join("repoB").to_string_lossy().replace('\\', "/");
        assert_eq!(rutas, vec![a.as_str(), b.as_str()]);
        assert_eq!(repos[0].nombre, "repoA");
        assert_eq!(repos[0].prefijo, "repoA");
        assert_eq!(repos[1].prefijo, "repoB");
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn profundidad_mayor_alcanza_repo_hondo() {
        let base = arbol_repos("hondo");
        let repos = descubrir_repos(&base, 3);
        assert!(repos.iter().any(|r| r.nombre == "repoC"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn la_raiz_repo_no_desciende() {
        let base = std::env::temp_dir().join(format!("gh-raiz-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join(".git")).unwrap();
        std::fs::create_dir_all(base.join("hijo/.git")).unwrap();
        let repos = descubrir_repos(&base, 2);
        assert_eq!(repos.len(), 1);
        assert!(repos[0].ruta.ends_with(base.file_name().unwrap().to_str().unwrap()));
        assert_eq!(repos[0].prefijo, "");
        let _ = std::fs::remove_dir_all(&base);
    }
}
