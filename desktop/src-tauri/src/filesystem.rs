//! Filesystem local del workspace activo para Files (089A-9).
//!
//! Este módulo no acepta una raíz desde la UI: resuelve siempre el workspace
//! activo de la sesión y solo permite rutas relativas contenidas en esa raíz.
//! Listado y búsqueda son acotados para no convertir la apertura del panel en
//! un snapshot completo del proyecto.

use chrono::{DateTime, Utc};
use serde::Serialize;
use std::cmp::Ordering;
use std::fs;
use std::path::{Component, Path, PathBuf};
use tauri::State;

use super::{area_activa, sesion_actual, Estado, Sesion};

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 500;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_SEARCH_DEPTH: usize = 24;
const MAX_TREE_DEPTH: u8 = 3;
const EXCLUDED_DIRECTORIES: [&str; 3] = [".git", "node_modules", "target"];

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ErrorFilesystem {
    pub codigo: String,
    pub mensaje: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ruta: Option<String>,
}

impl ErrorFilesystem {
    fn new(codigo: &str, mensaje: impl Into<String>, ruta: Option<&Path>) -> Self {
        Self {
            codigo: codigo.to_string(),
            mensaje: mensaje.into(),
            ruta: ruta.map(|p| p.to_string_lossy().replace('\\', "/")),
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct WorkspaceInfo {
    pub ruta: String,
    pub nombre: String,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct EntradaWorkspace {
    pub ruta: String,
    pub nombre: String,
    pub tipo: String,
    pub tamano: Option<u64>,
    pub modificado_en: Option<String>,
    pub ignorado: bool,
    pub hijos: Option<Vec<EntradaWorkspace>>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ListadoWorkspace {
    pub ruta: String,
    pub entradas: Vec<EntradaWorkspace>,
    pub truncado: bool,
    pub excluidas: Vec<String>,
}

#[derive(Debug, Serialize, Clone)]
pub(crate) struct ResultadoBusqueda {
    pub consulta: String,
    pub entradas: Vec<EntradaWorkspace>,
    pub truncado: bool,
    pub excluidas: Vec<String>,
}

#[tauri::command]
pub(crate) fn workspace_info(estado: State<'_, Estado>) -> Result<WorkspaceInfo, ErrorFilesystem> {
    let sesion = sesion_actual(&estado).map_err(|e| ErrorFilesystem::new("sesion", e, None))?;
    let raiz = raiz_activa(&sesion)?;
    Ok(info_raiz(&raiz))
}

#[tauri::command]
pub(crate) fn workspace_listar_entrada(
    estado: State<'_, Estado>,
    ruta_relativa: String,
    profundidad: Option<u8>,
) -> Result<ListadoWorkspace, ErrorFilesystem> {
    let sesion = sesion_actual(&estado).map_err(|e| ErrorFilesystem::new("sesion", e, None))?;
    let raiz = raiz_activa(&sesion)?;
    let dir = resolver_existente(&raiz, &ruta_relativa, true)?;
    let profundidad = profundidad.unwrap_or(0).min(MAX_TREE_DEPTH);
    let (entradas, truncado) = listar_directorio(&raiz, &dir, profundidad)?;
    Ok(ListadoWorkspace {
        ruta: relativa(&raiz, &dir),
        entradas,
        truncado,
        excluidas: EXCLUDED_DIRECTORIES.iter().map(|s| (*s).to_string()).collect(),
    })
}

#[tauri::command]
pub(crate) fn workspace_leer_archivo(
    estado: State<'_, Estado>,
    ruta_relativa: String,
    limite_bytes: Option<u64>,
) -> Result<super::archivo::ArchivoLeido, ErrorFilesystem> {
    let sesion = sesion_actual(&estado).map_err(|e| ErrorFilesystem::new("sesion", e, None))?;
    let raiz = raiz_activa(&sesion)?;
    let archivo = resolver_existente(&raiz, &ruta_relativa, false)?;
    leer_archivo_limitado(&raiz, &archivo, limite_bytes.unwrap_or(MAX_FILE_BYTES))
}

#[tauri::command]
pub(crate) fn workspace_buscar(
    estado: State<'_, Estado>,
    consulta: String,
    ruta_relativa: Option<String>,
    limite_resultados: Option<usize>,
) -> Result<ResultadoBusqueda, ErrorFilesystem> {
    let consulta = consulta.trim().to_string();
    if consulta.is_empty() {
        return Err(ErrorFilesystem::new("consulta_vacia", "la consulta es obligatoria", None));
    }
    let sesion = sesion_actual(&estado).map_err(|e| ErrorFilesystem::new("sesion", e, None))?;
    let raiz = raiz_activa(&sesion)?;
    let inicio = resolver_existente(&raiz, ruta_relativa.as_deref().unwrap_or(""), true)?;
    let limite = limite_resultados.unwrap_or(50).clamp(1, MAX_SEARCH_RESULTS);
    let consulta_lower = consulta.to_lowercase();
    let mut pendientes = vec![(inicio, 0usize)];
    let mut entradas = Vec::new();
    let mut truncado = false;

    while let Some((directorio, profundidad)) = pendientes.pop() {
        let mut hijos = fs::read_dir(&directorio)
            .map_err(|e| error_io("listar_no_disponible", e, Some(&directorio)))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| error_io("listar_no_disponible", e, Some(&directorio)))?;
        hijos.sort_by(|a, b| nombre_de(&a.path()).cmp(&nombre_de(&b.path())));
        for hijo in hijos {
            let path = hijo.path();
            let nombre = nombre_de(&path);
            let entrada = entrada_workspace(&raiz, &path)?;
            if nombre.to_lowercase().contains(&consulta_lower) {
                entradas.push(entrada.clone());
                if entradas.len() >= limite {
                    truncado = true;
                    break;
                }
            }
            if entrada.tipo == "directorio"
                && !entrada.ignorado
                && profundidad < MAX_SEARCH_DEPTH
            {
                pendientes.push((path, profundidad + 1));
            }
        }
        if truncado {
            break;
        }
    }

    entradas.sort_by(|a, b| a.ruta.to_lowercase().cmp(&b.ruta.to_lowercase()));
    Ok(ResultadoBusqueda {
        consulta,
        entradas,
        truncado,
        excluidas: EXCLUDED_DIRECTORIES.iter().map(|s| (*s).to_string()).collect(),
    })
}

fn raiz_activa(sesion: &Sesion) -> Result<PathBuf, ErrorFilesystem> {
    let workspace = area_activa(sesion)
        .map_err(|e| ErrorFilesystem::new("workspace_invalido", e, None))?
        .ok_or_else(|| {
            ErrorFilesystem::new(
                "workspace_no_configurado",
                "elige un workspace antes de explorar archivos",
                None,
            )
        })?;
    let raiz = PathBuf::from(workspace.ruta);
    let raiz = raiz
        .canonicalize()
        .map_err(|e| error_io("workspace_invalido", e, Some(&raiz)))?;
    if !raiz.is_dir() {
        return Err(ErrorFilesystem::new(
            "workspace_invalido",
            "el workspace activo no es una carpeta",
            Some(&raiz),
        ));
    }
    Ok(raiz)
}

fn info_raiz(raiz: &Path) -> WorkspaceInfo {
    let nombre = raiz
        .file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| raiz.to_string_lossy().into_owned());
    WorkspaceInfo {
        ruta: raiz.to_string_lossy().replace('\\', "/"),
        nombre,
    }
}

fn resolver_existente(raiz: &Path, ruta: &str, debe_ser_directorio: bool) -> Result<PathBuf, ErrorFilesystem> {
    let ruta = ruta.trim();
    let path = Path::new(ruta);
    if path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(ErrorFilesystem::new(
            "ruta_fuera_workspace",
            "la ruta debe ser relativa y no puede contener '..'",
            Some(path),
        ));
    }
    let raiz = raiz.canonicalize().map_err(|e| error_io("workspace_invalido", e, Some(raiz)))?;
    let objetivo = if ruta.is_empty() { raiz.clone() } else { raiz.join(path) };
    let canon = objetivo
        .canonicalize()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ErrorFilesystem::new("ruta_no_encontrada", "la ruta no existe", Some(path))
            } else {
                error_io("ruta_no_disponible", e, Some(path))
            }
        })?;
    if !canon.starts_with(raiz) {
        return Err(ErrorFilesystem::new(
            "ruta_fuera_workspace",
            "la ruta queda fuera del workspace activo",
            Some(path),
        ));
    }
    if debe_ser_directorio && !canon.is_dir() {
        return Err(ErrorFilesystem::new("no_es_directorio", "la ruta no es una carpeta", Some(path)));
    }
    if !debe_ser_directorio && canon.is_dir() {
        return Err(ErrorFilesystem::new("no_es_archivo", "la ruta es una carpeta", Some(path)));
    }
    Ok(canon)
}

fn listar_directorio(
    raiz: &Path,
    directorio: &Path,
    profundidad: u8,
) -> Result<(Vec<EntradaWorkspace>, bool), ErrorFilesystem> {
    let mut hijos = fs::read_dir(directorio)
        .map_err(|e| error_io("listar_no_disponible", e, Some(directorio)))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| error_io("listar_no_disponible", e, Some(directorio)))?;
    hijos.sort_by(|a, b| comparar_entradas(&a.path(), &b.path()));
    let truncado = hijos.len() > MAX_ENTRIES;
    let mut entradas = Vec::with_capacity(hijos.len().min(MAX_ENTRIES));
    for hijo in hijos.into_iter().take(MAX_ENTRIES) {
        let path = hijo.path();
        let mut entrada = entrada_workspace(raiz, &path)?;
        if profundidad > 0 && entrada.tipo == "directorio" && !entrada.ignorado {
            let (nietos, _) = listar_directorio(raiz, &path, profundidad - 1)?;
            entrada.hijos = Some(nietos);
        }
        entradas.push(entrada);
    }
    Ok((entradas, truncado))
}

fn entrada_workspace(raiz: &Path, path: &Path) -> Result<EntradaWorkspace, ErrorFilesystem> {
    let enlace = fs::symlink_metadata(path)
        .map_err(|e| error_io("entrada_no_disponible", e, Some(path)))?;
    let es_enlace = enlace.file_type().is_symlink();
    let metadata = fs::metadata(path).ok();
    let directorio = metadata.as_ref().is_some_and(|m| m.is_dir());
    let ignorado = directorio && es_excluida(path);
    let tipo = if es_enlace {
        "enlace"
    } else if directorio {
        "directorio"
    } else if metadata.as_ref().is_some_and(|m| m.is_file()) {
        "archivo"
    } else {
        "desconocido"
    };
    let modificado_en = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .map(|t| DateTime::<Utc>::from(t).to_rfc3339());
    Ok(EntradaWorkspace {
        ruta: relativa(raiz, path),
        nombre: nombre_de(path),
        tipo: tipo.to_string(),
        tamano: metadata.as_ref().map(|m| m.len()),
        modificado_en,
        ignorado,
        hijos: None,
    })
}

fn leer_archivo_limitado(
    raiz: &Path,
    archivo: &Path,
    limite_bytes: u64,
) -> Result<super::archivo::ArchivoLeido, ErrorFilesystem> {
    let limite = limite_bytes.clamp(1, MAX_FILE_BYTES);
    let tamano = fs::metadata(archivo)
        .map_err(|e| error_io("lectura_no_disponible", e, Some(archivo)))?
        .len();
    if tamano > limite {
        return Err(ErrorFilesystem::new(
            "archivo_demasiado_grande",
            format!("el archivo ocupa {tamano} bytes y el límite es {limite}"),
            Some(archivo),
        ));
    }
    let contenido = fs::read_to_string(archivo).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            ErrorFilesystem::new("archivo_binario", "el archivo no es texto UTF-8", Some(archivo))
        } else {
            error_io("lectura_no_disponible", e, Some(archivo))
        }
    })?;
    Ok(super::archivo::ArchivoLeido {
        ruta: relativa(raiz, archivo),
        lineas: contenido.lines().count(),
        contenido,
    })
}

fn relativa(raiz: &Path, path: &Path) -> String {
    path.strip_prefix(raiz)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn nombre_de(path: &Path) -> String {
    path.file_name()
        .map(|v| v.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn es_excluida(path: &Path) -> bool {
    EXCLUDED_DIRECTORIES.iter().any(|nombre| nombre_de(path) == *nombre)
}

fn comparar_entradas(a: &Path, b: &Path) -> Ordering {
    let a_dir = fs::metadata(a).map(|m| m.is_dir()).unwrap_or(false);
    let b_dir = fs::metadata(b).map(|m| m.is_dir()).unwrap_or(false);
    b_dir.cmp(&a_dir).then_with(|| nombre_de(a).to_lowercase().cmp(&nombre_de(b).to_lowercase()))
}

fn error_io(codigo: &str, error: std::io::Error, ruta: Option<&Path>) -> ErrorFilesystem {
    let codigo = if error.kind() == std::io::ErrorKind::PermissionDenied {
        "permiso_denegado"
    } else {
        codigo
    };
    ErrorFilesystem::new(codigo, error.to_string(), ruta)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn fixture() -> (PathBuf, PathBuf) {
        let raiz = std::env::temp_dir().join(format!("glory-harness-files-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(raiz.join("src")).unwrap();
        let mut archivo = fs::File::create(raiz.join("src/main.rs")).unwrap();
        writeln!(archivo, "fn main() {{}}\n").unwrap();
        fs::write(raiz.join("README.md"), "hola\nmundo\n").unwrap();
        (raiz.clone(), raiz.join("src/main.rs"))
    }

    #[test]
    fn rechaza_escape_y_absoluta() {
        let (raiz, _) = fixture();
        assert_eq!(resolver_existente(&raiz, "../secreto", false).unwrap_err().codigo, "ruta_fuera_workspace");
        assert_eq!(resolver_existente(&raiz, &std::env::temp_dir().to_string_lossy(), true).unwrap_err().codigo, "ruta_fuera_workspace");
        fs::remove_dir_all(raiz).unwrap();
    }

    #[test]
    fn lista_directorios_primero_y_lee_texto() {
        let (raiz, archivo) = fixture();
        let (entradas, truncado) = listar_directorio(&raiz, &raiz, 0).unwrap();
        assert!(!truncado);
        assert_eq!(entradas[0].tipo, "directorio");
        let leido = leer_archivo_limitado(&raiz, &archivo, MAX_FILE_BYTES).unwrap();
        assert!(leido.contenido.contains("fn main"));
        fs::remove_dir_all(raiz).unwrap();
    }

    #[test]
    fn informa_archivo_grande_y_binario() {
        let (raiz, _) = fixture();
        fs::write(raiz.join("grande.txt"), vec![b'x'; (MAX_FILE_BYTES + 1) as usize]).unwrap();
        fs::write(raiz.join("dato.bin"), [0, 159, 146, 150]).unwrap();
        let grande = resolver_existente(&raiz, "grande.txt", false).unwrap();
        let binario = resolver_existente(&raiz, "dato.bin", false).unwrap();
        assert_eq!(leer_archivo_limitado(&raiz, &grande, MAX_FILE_BYTES).unwrap_err().codigo, "archivo_demasiado_grande");
        assert_eq!(leer_archivo_limitado(&raiz, &binario, MAX_FILE_BYTES).unwrap_err().codigo, "archivo_binario");
        fs::remove_dir_all(raiz).unwrap();
    }
}
