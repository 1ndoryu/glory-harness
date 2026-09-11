//! [089A-2] Lectura de archivos del workspace para el visor.
//!
//! Solo sirve archivos bajo `base` (la ruta del workspace activo, que pasa el
//! frontend): canonicaliza y rechaza escapes (`..`, enlaces fuera), carpetas,
//! binarios (no UTF-8) y archivos de más de 256 KB. Errores explícitos.

use serde::Serialize;
use std::path::PathBuf;

const MAX_BYTES: u64 = 256 * 1024;

/// Archivo leído para el visor.
#[derive(Debug, Serialize)]
pub struct ArchivoLeido {
    /// Ruta absoluta canonicalizada.
    pub ruta: String,
    /// Número de líneas del contenido devuelto.
    pub lineas: usize,
    /// Contenido UTF-8 completo.
    pub contenido: String,
}

/// Lee un archivo del workspace (`ruta` relativa o absoluta bajo `base`).
#[tauri::command]
pub async fn leer_archivo(base: String, ruta: String) -> Result<ArchivoLeido, String> {
    if ruta.trim().is_empty() {
        return Err("ruta vacía".to_string());
    }
    let base_canon = PathBuf::from(&base)
        .canonicalize()
        .map_err(|_| format!("área de trabajo no válida: {base}"))?;
    let objetivo = base_canon.join(ruta.trim());
    let obj_canon = objetivo
        .canonicalize()
        .map_err(|_| "archivo no encontrado en el área de trabajo".to_string())?;
    if !obj_canon.starts_with(&base_canon) {
        return Err("ruta fuera del área de trabajo".to_string());
    }
    if obj_canon.is_dir() {
        return Err("es una carpeta, no un archivo".to_string());
    }
    let tam = obj_canon
        .metadata()
        .map_err(|e| format!("no se pudo leer: {e}"))?
        .len();
    if tam > MAX_BYTES {
        return Err(format!(
            "archivo demasiado grande ({tam} bytes, máximo {MAX_BYTES})"
        ));
    }
    let contenido = std::fs::read_to_string(&obj_canon)
        .map_err(|_| "no es un archivo de texto (UTF-8)".to_string())?;
    let lineas = contenido.lines().count();
    Ok(ArchivoLeido {
        ruta: obj_canon.to_string_lossy().into_owned(),
        lineas,
        contenido,
    })
}
