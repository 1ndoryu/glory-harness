//! [089A-10] Files del workspace activo por HTTP (modo web).
//!
//! Espejo web de `desktop/src-tauri/src/filesystem.rs` (089A-9): la UI en el
//! navegador ya no depende de la app de escritorio para el panel Files. Misma
//! lógica pura (raíz desde la sesión, validación de contención, listado y
//! búsqueda acotados, lectura limitada) expuesta como endpoints GET.
//!
//! No acepta una raíz desde la UI: resuelve siempre el workspace activo de la
//! sesión (`comun.workspace` vía `area_activa`) y solo permite rutas relativas
//! contenidas en esa raíz.

use std::cmp::Ordering;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use axum::{
    extract::{Path as Ruta, Query, State},
    http::{HeaderMap, Method},
    Json,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{area_activa, error, sesion_y_comun, ApiError, AppState};
use crate::servicio::SesionComun;

const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_ENTRIES: usize = 500;
const MAX_SEARCH_RESULTS: usize = 200;
const MAX_SEARCH_DEPTH: usize = 24;
const MAX_TREE_DEPTH: u8 = 3;
const EXCLUDED_DIRECTORIES: [&str; 3] = [".git", "node_modules", "target"];

/// [089A-9] Información del workspace activo (panel Files).
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

/// Archivo leído para el visor web: `ruta` relativa al workspace (igual que el
/// comando Tauri `workspace_leer_archivo`), no absoluta.
#[derive(Debug, Serialize, Clone)]
pub(crate) struct ArchivoLeidoWeb {
    pub ruta: String,
    pub lineas: usize,
    pub contenido: String,
}

// ── Params de query ──────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsListar {
    pub ruta: Option<String>,
    pub profundidad: Option<u8>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsLeer {
    pub ruta: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParamsBuscar {
    pub consulta: Option<String>,
    pub ruta: Option<String>,
}

// ── Handlers HTTP ────────────────────────────────────────────────────────

/// `GET /api/v1/session/{id}/files/info` — información del workspace activo.
pub(crate) async fn files_info(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
) -> Result<Json<WorkspaceInfo>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    Ok(Json(info_raiz(&raiz)))
}

/// `GET /api/v1/session/{id}/files/listar?ruta=…&profundidad=…`
pub(crate) async fn files_listar(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
    Query(params): Query<ParamsListar>,
) -> Result<Json<ListadoWorkspace>, ApiError> {
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    let dir = resolver_existente(&raiz, params.ruta.as_deref().unwrap_or(""), true)?;
    let profundidad = params.profundidad.unwrap_or(0).min(MAX_TREE_DEPTH);
    let (entradas, truncado) = listar_directorio(&raiz, &dir, profundidad)?;
    Ok(Json(ListadoWorkspace {
        ruta: relativa(&raiz, &dir),
        entradas,
        truncado,
        excluidas: EXCLUDED_DIRECTORIES.iter().map(|s| (*s).to_string()).collect(),
    }))
}

/// `GET /api/v1/session/{id}/files/leer?ruta=…`
pub(crate) async fn files_leer(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
    Query(params): Query<ParamsLeer>,
) -> Result<Json<ArchivoLeidoWeb>, ApiError> {
    let ruta = params
        .ruta
        .filter(|r| !r.trim().is_empty())
        .ok_or_else(|| error("consulta_vacia", "la ruta es obligatoria"))?;
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    let archivo = resolver_existente(&raiz, &ruta, false)?;
    Ok(Json(leer_archivo_limitado(&raiz, &archivo, MAX_FILE_BYTES)?))
}

/// `GET /api/v1/session/{id}/files/buscar?consulta=…&ruta=…`
pub(crate) async fn files_buscar(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Ruta(id): Ruta<String>,
    Query(params): Query<ParamsBuscar>,
) -> Result<Json<ResultadoBusqueda>, ApiError> {
    let consulta = params.consulta.unwrap_or_default().trim().to_string();
    if consulta.is_empty() {
        return Err(error("consulta_vacia", "la consulta es obligatoria"));
    }
    let (_, comun) = sesion_y_comun(&headers, &Method::GET, &state, &id).await?;
    let raiz = raiz_activa(&comun)?;
    let inicio = resolver_existente(&raiz, params.ruta.as_deref().unwrap_or(""), true)?;
    let limite = 50usize.clamp(1, MAX_SEARCH_RESULTS);
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
    Ok(Json(ResultadoBusqueda {
        consulta,
        entradas,
        truncado,
        excluidas: EXCLUDED_DIRECTORIES.iter().map(|s| (*s).to_string()).collect(),
    }))
}

// ── Helpers puros (port de desktop/src-tauri/src/filesystem.rs) ──────────

/// Resuelve la raíz canónica del workspace activo de la sesión web.
fn raiz_activa(comun: &SesionComun) -> Result<PathBuf, ApiError> {
    let workspace = area_activa(comun)?
        .ok_or_else(|| error("workspace_no_configurado", "elige un workspace antes de explorar archivos"))?;
    let raiz = PathBuf::from(workspace.ruta);
    let raiz = raiz
        .canonicalize()
        .map_err(|e| error_io("workspace_invalido", e, Some(&raiz)))?;
    if !raiz.is_dir() {
        return Err(error("workspace_invalido", "el workspace activo no es una carpeta"));
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

fn resolver_existente(raiz: &Path, ruta: &str, debe_ser_directorio: bool) -> Result<PathBuf, ApiError> {
    let ruta = ruta.trim();
    let path = Path::new(ruta);
    if path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(error(
            "ruta_fuera_workspace",
            "la ruta debe ser relativa y no puede contener '..'",
        ));
    }
    let raiz = raiz
        .canonicalize()
        .map_err(|e| error_io("workspace_invalido", e, Some(raiz)))?;
    let objetivo = if ruta.is_empty() { raiz.clone() } else { raiz.join(path) };
    let canon = objetivo.canonicalize().map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            error("ruta_no_encontrada", "la ruta no existe")
        } else {
            error_io("ruta_no_disponible", e, Some(path))
        }
    })?;
    if !canon.starts_with(&raiz) {
        return Err(error("ruta_fuera_workspace", "la ruta queda fuera del workspace activo"));
    }
    if debe_ser_directorio && !canon.is_dir() {
        return Err(error("no_es_directorio", "la ruta no es una carpeta"));
    }
    if !debe_ser_directorio && canon.is_dir() {
        return Err(error("no_es_archivo", "la ruta es una carpeta"));
    }
    Ok(canon)
}

fn listar_directorio(
    raiz: &Path,
    directorio: &Path,
    profundidad: u8,
) -> Result<(Vec<EntradaWorkspace>, bool), ApiError> {
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

fn entrada_workspace(raiz: &Path, path: &Path) -> Result<EntradaWorkspace, ApiError> {
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

fn leer_archivo_limitado(raiz: &Path, archivo: &Path, limite_bytes: u64) -> Result<ArchivoLeidoWeb, ApiError> {
    let limite = limite_bytes.clamp(1, MAX_FILE_BYTES);
    let tamano = fs::metadata(archivo)
        .map_err(|e| error_io("lectura_no_disponible", e, Some(archivo)))?
        .len();
    if tamano > limite {
        return Err(error(
            "archivo_demasiado_grande",
            format!("el archivo ocupa {tamano} bytes y el límite es {limite}"),
        ));
    }
    let contenido = fs::read_to_string(archivo).map_err(|e| {
        if e.kind() == std::io::ErrorKind::InvalidData {
            error("archivo_binario", "el archivo no es texto UTF-8")
        } else {
            error_io("lectura_no_disponible", e, Some(archivo))
        }
    })?;
    Ok(ArchivoLeidoWeb {
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

fn error_io(codigo: &str, io: std::io::Error, ruta: Option<&Path>) -> ApiError {
    let codigo = if io.kind() == std::io::ErrorKind::PermissionDenied {
        "permiso_denegado"
    } else {
        codigo
    };
    let mensaje = match ruta {
        Some(p) => format!("{} ({})", io, p.to_string_lossy().replace('\\', "/")),
        None => io.to_string(),
    };
    error(codigo, mensaje)
}
