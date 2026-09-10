//! [109A-4] Comandos `/` propios del área activa (`.glory/comandos/*.md`).
//!
//! El escritorio no inventa un formato de comandos: reutiliza el del núcleo
//! (`glory_harness_core::herramientas::skill`), el MISMO que consumen el CLI y
//! la TUI. Aquí solo se decide la CARPETA (el área activa, resuelta por el
//! backend como en `memoria.rs`) y se expone el catálogo y la expansión.
//!
//! Los comandos integrados (`/ayuda`, `/modelo`, …) NO pasan por aquí: son
//! fijos del front y su efecto no depende del área.

use std::path::PathBuf;

use glory_harness::Workspace;
use glory_harness_core::skill::{descubrir_comandos, expandir_comando};
use tauri::State;

use crate::{area_activa, Estado, Sesion};

/// Carpeta de comandos del área (`.glory/comandos`, igual que el CLI).
const CARPETA: &str = ".glory/comandos";

/// Un comando del área tal como lo pinta el menú `/`.
#[derive(serde::Serialize)]
pub(crate) struct ComandoVista {
    pub(crate) nombre: String,
    pub(crate) descripcion: String,
}

/// Área activa (auto-registrada si la ruta es válida) y su carpeta de comandos.
fn area_y_carpeta(sesion: &Sesion) -> Result<Option<(Workspace, PathBuf)>, String> {
    let Some(area) = area_activa(sesion)? else {
        return Ok(None);
    };
    let carpeta = PathBuf::from(&area.ruta).join(CARPETA);
    Ok(Some((area, carpeta)))
}

/// Comandos markdown del área activa. Sin área (o sin carpeta) → lista vacía:
/// el menú sigue ofreciendo los integrados y nunca falla por esto.
#[tauri::command]
pub(crate) async fn comandos_listar(estado: State<'_, Estado>) -> Result<Vec<ComandoVista>, String> {
    let sesion = crate::sesion_actual(&estado)?;
    let Some((_, carpeta)) = area_y_carpeta(&sesion)? else {
        return Ok(Vec::new());
    };
    Ok(descubrir_comandos(&carpeta)
        .into_iter()
        .map(|c| ComandoVista {
            nombre: c.nombre,
            descripcion: c.descripcion,
        })
        .collect())
}

/// Expande la plantilla de un comando del área con sus argumentos (incluye
/// `@archivo` y `$ARGUMENTOS`, resueltos relativos al área activa).
#[tauri::command]
pub(crate) async fn comando_expandir(
    nombre: String,
    argumentos: Option<String>,
    estado: State<'_, Estado>,
) -> Result<String, String> {
    let sesion = crate::sesion_actual(&estado)?;
    let Some((area, carpeta)) = area_y_carpeta(&sesion)? else {
        return Err(format!(
            "el comando /{nombre} necesita un área de trabajo activa"
        ));
    };
    let comandos = descubrir_comandos(&carpeta);
    let texto = match argumentos.as_deref().unwrap_or("").trim() {
        "" => format!("/{nombre}"),
        args => format!("/{nombre} {args}"),
    };
    expandir_comando(&texto, &comandos, Some(&PathBuf::from(&area.ruta)))
        .ok_or_else(|| format!("el área activa no define el comando /{nombre}"))
}
