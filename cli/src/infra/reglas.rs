/* [318A-15 F2] Carga de reglas del repositorio (AGENTS.md) para la ranura
 * `[REGLAS]` del system prompt (patrón opencode `docs/rules`: el archivo más
 * cercano a la RAÍZ gana sobre los de subcarpetas).
 *
 * Jerarquía: se sube desde el workspace hacia la raíz del filesystem, se
 * recogen todos los `AGENTS.md` encontrados y gana el de menor profundidad
 * (la raíz del repo manda). Ausencia de AGENTS.md → `None` (el núcleo no
 * emite la ranura huérfana). Caché por directorio consultado.
 */
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Caché global por directorio consultado (una entrada por workspace distinto).
fn cache() -> &'static Mutex<HashMap<PathBuf, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<PathBuf, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Contenido de `AGENTS.md` aplicable a `workspace` (jerarquía de ancestros,
/// raíz gana), o `None` si no hay ninguno. Con caché por directorio.
pub fn cargar_reglas(workspace: &Path) -> Option<String> {
    let mut cache = cache().lock().unwrap_or_else(|p| p.into_inner());
    if let Some(reglas) = cache.get(workspace) {
        return reglas.clone();
    }
    let reglas = cargar_reglas_sin_cache(workspace);
    cache.insert(workspace.to_path_buf(), reglas.clone());
    reglas
}

/// Implementación sin caché, reutilizada por los tests con directorios
/// temporales (cada fixture pasa su propio `HashMap`).
fn cargar_reglas_sin_cache(workspace: &Path) -> Option<String> {
    let mut candidatos: Vec<(usize, PathBuf)> = Vec::new();
    let mut actual: Option<&Path> = Some(workspace);
    while let Some(dir) = actual {
        let candidato = dir.join("AGENTS.md");
        if candidato.is_file() {
            // Menor profundidad = más cerca de la raíz = gana.
            candidatos.push((candidato.components().count(), candidato));
        }
        actual = dir.parent();
    }
    candidatos.sort_by_key(|(profundidad, _)| *profundidad);
    let (_profundidad, ganador) = candidatos.into_iter().next()?;
    std::fs::read_to_string(&ganador)
        .map(|contenido| contenido.trim().to_string())
        .ok()
        .filter(|contenido| !contenido.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Crea un árbol temporal `raiz/sub/deep` con AGENTS.md opcionales en
    /// `raiz` y `raiz/sub`; devuelve (raiz, workspace) para limpiar luego.
    fn arbol_temporal(agents_raiz: Option<&str>, agents_sub: Option<&str>) -> (PathBuf, PathBuf) {
        let base =
            std::env::temp_dir().join(format!("gh-reglas-test-{}", uuid::Uuid::new_v4().simple()));
        let raiz = base.join("raiz");
        let sub = raiz.join("sub");
        let deep = sub.join("deep");
        fs::create_dir_all(&deep).unwrap();
        if let Some(c) = agents_raiz {
            fs::write(raiz.join("AGENTS.md"), c).unwrap();
        }
        if let Some(c) = agents_sub {
            fs::write(sub.join("AGENTS.md"), c).unwrap();
        }
        (base, deep)
    }

    #[test]
    fn f2_raiz_gana_a_subcarpeta() {
        let (base, workspace) = arbol_temporal(Some("reglas de la raiz"), Some("reglas de la sub"));
        let reglas = cargar_reglas_sin_cache(&workspace);
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(reglas.as_deref(), Some("reglas de la raiz"));
    }

    #[test]
    fn f2_solo_subcarpeta_la_usa() {
        let (base, workspace) = arbol_temporal(None, Some("reglas de la sub"));
        let reglas = cargar_reglas_sin_cache(&workspace);
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(reglas.as_deref(), Some("reglas de la sub"));
    }

    #[test]
    fn f2_ausencia_no_rompe() {
        let (base, workspace) = arbol_temporal(None, None);
        let reglas = cargar_reglas_sin_cache(&workspace);
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(reglas, None);
    }

    #[test]
    fn f2_caché_devuelve_lo_mismo_sin_releer() {
        let (base, workspace) = arbol_temporal(Some("reglas de la raiz"), None);
        // Primera lectura puebla la caché; borramos el archivo y la segunda
        // llamada debe devolver la entrada cacheada (no re-leer del disco).
        let primera = cargar_reglas(&workspace);
        // El AGENTS.md vive en la raíz (base/raiz), no en el padre directo.
        fs::remove_file(
            workspace
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("AGENTS.md"),
        )
        .unwrap();
        let segunda = cargar_reglas(&workspace);
        fs::remove_dir_all(&base).unwrap();
        assert_eq!(primera, segunda);
        assert_eq!(primera.as_deref(), Some("reglas de la raiz"));
    }

    #[test]
    fn f2_e2e_reglas_llegan_a_la_ranura() {
        // E2E determinista: AGENTS.md falso → loader → prompt ensamblado,
        // verificado dentro de [REGLAS] con fecha fija (sin modelo real).
        let (base, workspace) = arbol_temporal(Some("Regla 1: no borres archivos ajenos."), None);
        let reglas = cargar_reglas_sin_cache(&workspace).unwrap();
        let config = crate::run::turno_config_default(Some(workspace.clone()));
        let prompt =
            glory_harness_core::runtime::ensamblar_prompt_sistema(&config, &reglas, "2026-09-03");
        fs::remove_dir_all(&base).unwrap();

        assert!(prompt.contains("[REGLAS]"), "marcador apertura presente");
        assert!(
            prompt.contains("Regla 1: no borres archivos ajenos."),
            "contenido del AGENTS.md dentro del prompt"
        );
        assert!(prompt.contains("[/REGLAS]"), "marcador cierre presente");
        let apertura = prompt.find("[REGLAS]").unwrap();
        let cierre = prompt.find("[/REGLAS]").unwrap();
        let contenido = &prompt[apertura..cierre];
        assert!(
            contenido.contains("Regla 1"),
            "el contenido vive entre los marcadores"
        );
        assert!(
            prompt.find("[ENTORNO]").unwrap() > cierre,
            "[ENTORNO] va después de la ranura [REGLAS]"
        );
    }
}
