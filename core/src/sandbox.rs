/* [29-08-2026] Sandbox de archivos del agente (plan-agente-ia-plugin, Fase 2).
 * SOLO en AGENTE_MODO=local (dev): nunca en producción, ni siquiera admin.
 *
 * Validación de rutas para Windows/OneDrive:
 * - `canonicalize()` la ruta y verificar prefijo con separador + case-insensitive.
 * - Prohibido `..` y escapes fuera del workspace.
 * - Lista negra de secretos ANTES de leer (`.env`, `*.pem`, `.ssh`, `*_KEY`,
 *   `.git/config`) — la negación se aplica antes de mostrar contenido.
 * - Junctions/symlinks: canonicalize los resuelve; el check es sobre la ruta
 *   canónica (no escapa del workspace). */

use crate::error::Error;
use std::path::{Component, Path, PathBuf};

/// Nombres de archivo/segmento que el agente NUNCA puede leer (secretos).
const SECRET_SEGMENTS: &[&str] = &[
    ".env",
    ".env.local",
    ".env.production",
    "id_rsa",
    "id_ed25519",
    "known_hosts",
    "config", // .git/config
    "credentials",
    "secrets",
];

const SECRET_EXTENSIONES: &[&str] = &["pem", "p12", "pfx", "key", "keystore", "jks"];

const SECRET_PREFIJOS: &[&str] = &["*_KEY", "*.key"];

/// Raíz del sandbox (workspace). `new` la canonicaliza; si no existe se crea.
#[derive(Debug, Clone)]
pub struct SandboxArchivos {
    raiz: PathBuf,
}

impl SandboxArchivos {
    /// Crea el sandbox sobre `raiz` (la canonicaliza; error si no se puede).
    pub fn nuevo(raiz: impl Into<PathBuf>) -> Result<Self, Error> {
        let raiz = raiz.into();
        std::fs::create_dir_all(&raiz).map_err(|error| {
            Error::Validacion(format!("No se pudo crear el workspace: {error}"))
        })?;
        let canonica = std::fs::canonicalize(&raiz).map_err(|error| {
            Error::Validacion(format!("Workspace no accesible: {error}"))
        })?;
        Ok(Self { raiz: canonica })
    }

    /// Resuelve una ruta relativa al workspace y valida que quede DENTRO.
    /// Devuelve la ruta canónica (resuelve junctions/symlinks y `..`).
    pub fn resolver(&self, relativa: &str) -> Result<PathBuf, Error> {
        if relativa.trim().is_empty() {
            return Err(Error::Validacion("Ruta vacía".into()));
        }
        let ruta = Path::new(relativa);
        if ruta.is_absolute() {
            return Err(Error::Sandbox(
                "Solo rutas relativas al workspace".into(),
            ));
        }
        /* Prohibir `..` explícitamente (defensa en profundidad). */
        for componente in ruta.components() {
            if matches!(componente, Component::ParentDir) {
                return Err(Error::Sandbox(
                    "No se permiten rutas con '..'".into(),
                ));
            }
        }
        let candidata = self.raiz.join(ruta);
        let canonica = std::fs::canonicalize(&candidata).map_err(|_| {
            Error::NoEncontrado("La ruta no existe dentro del workspace".into())
        })?;
        /* Verificación case-insensitive (Windows) del prefijo + separador. */
        let raiz_lower = self
            .raiz
            .to_string_lossy()
            .to_ascii_lowercase()
            .trim_end_matches(['/', '\\'])
            .to_string();
        let canonica_str = canonica.to_string_lossy().to_ascii_lowercase();
        let dentro = canonica_str == raiz_lower
            || canonica_str
                .strip_prefix(&raiz_lower)
                .map(|resto| resto.starts_with(['/', '\\']))
                .unwrap_or(false);
        if !dentro {
            return Err(Error::Sandbox(
                "La ruta escapa del workspace (junctions/symlinks/..)".into(),
            ));
        }
        Ok(canonica)
    }

    /// Ruta canónica del workspace (para búsquedas y lectura de directorios).
    #[must_use]
    pub fn raiz(&self) -> &Path {
        &self.raiz
    }

    /// Ruta absoluta presentable de un archivo relativo al workspace, para los
    /// resúmenes de las tools: `canonicalize` deja el prefijo verbatim de
    /// Windows (`\\?\`); se quita para mostrar la ruta completa limpia.
    pub fn ruta_presentable(&self, relativa: &str) -> String {
        let completa = self.raiz.join(relativa);
        let s = completa.to_string_lossy();
        let sin_prefijo = s.strip_prefix(r"\\?\").unwrap_or(&s);
        sin_prefijo.to_string()
    }

    /// ¿La ruta (relativa) es un secreto que el agente no puede leer?
    pub fn es_secreto(&self, relativa: &str) -> bool {
        let normalizada = relativa.replace('\\', "/").trim_start_matches("./").to_string();
        let segmentos: Vec<&str> = normalizada.split('/').collect();
        if let Some(archivo) = segmentos.last() {
            let nombre = archivo.to_ascii_lowercase();
            if SECRET_SEGMENTS
                .iter()
                .any(|s| nombre == *s || nombre.ends_with(&format!(".{s}")))
            {
                return true;
            }
            if SECRET_EXTENSIONES.iter().any(|ext| {
                Path::new(&nombre)
                    .extension()
                    .map(|e| e.to_string_lossy().eq_ignore_ascii_case(ext))
                    .unwrap_or(false)
            }) {
                return true;
            }
            if SECRET_PREFIJOS.iter().any(|p| nombre.ends_with(&p.trim_start_matches('*'))) {
                return true;
            }
        }
        // `.git/` completo es secreto (config, objetos, credenciales).
        segmentos.first() == Some(&".git")
    }

    /// Lee un archivo del workspace con límite de tamaño (truncado con aviso).
    pub fn leer(&self, relativa: &str, max_bytes: usize) -> Result<(String, bool), Error> {
        if self.es_secreto(relativa) {
            return Err(Error::Sandbox(
                "El archivo está en la lista negra de secretos y no se puede leer".into(),
            ));
        }
        let ruta = self.resolver(relativa)?;
        let datos = std::fs::read(&ruta)
            .map_err(|error| Error::NoEncontrado(format!("No se pudo leer: {error}")))?;
        let truncado = datos.len() > max_bytes;
        let contenido = String::from_utf8_lossy(&datos[..datos.len().min(max_bytes)]).to_string();
        Ok((contenido, truncado))
    }

    /// Escribe un archivo (crea directorios intermedios). Solo archivos.
    pub fn escribir(&self, relativa: &str, contenido: &str) -> Result<PathBuf, Error> {
        if self.es_secreto(relativa) {
            return Err(Error::Sandbox(
                "El archivo está en la lista negra de secretos y no se puede escribir".into(),
            ));
        }
        let ruta = self.resolver_para_escribir(relativa)?;
        if let Some(parent) = ruta.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Error::Validacion(format!("No se pudo crear directorios: {error}")))?;
        }
        std::fs::write(&ruta, contenido)
            .map_err(|error| Error::Validacion(format!("No se pudo escribir: {error}")))?;
        Ok(ruta)
    }

    /// Resolución para escritura: la ruta puede no existir aún, así que se
    /// valida lexicográficamente (sin canonicalizar el archivo final; sí el
    /// padre si existe, para no escapar por junction del directorio).
    fn resolver_para_escribir(&self, relativa: &str) -> Result<PathBuf, Error> {
        let ruta = Path::new(relativa);
        if ruta.is_absolute() {
            return Err(Error::Sandbox(
                "Solo rutas relativas al workspace".into(),
            ));
        }
        for componente in ruta.components() {
            if matches!(componente, Component::ParentDir) {
                return Err(Error::Sandbox(
                    "No se permiten rutas con '..'".into(),
                ));
            }
        }
        /* El padre debe quedar dentro (o ser el propio workspace). */
        let padre = self.raiz.join(ruta.parent().unwrap_or_else(|| Path::new("")));
        let padre_canonico = if padre.exists() {
            std::fs::canonicalize(&padre).map_err(|error| {
                Error::Validacion(format!("Directorio no accesible: {error}"))
            })?
        } else {
            /* El padre no existe: validar lexicográficamente sobre la raíz
             * canónica (sin resolver, no hay junction posible en un dir nuevo). */
            padre
        };
        let dentro = contiene(&self.raiz, &padre_canonico);
        if !dentro {
            return Err(Error::Sandbox(
                "La ruta de escritura escapa del workspace".into(),
            ));
        }
        Ok(padre_canonico.join(ruta.file_name().ok_or_else(|| {
            Error::Validacion("La ruta no tiene nombre de archivo".into())
        })?))
    }
}

/// ¿`candidata` está dentro de `raiz`? Case-insensitive + prefijo con separador.
fn contiene(raiz: &Path, candidata: &Path) -> bool {
    let raiz_s = raiz.to_string_lossy().to_ascii_lowercase();
    let raiz_s = raiz_s.trim_end_matches(['/', '\\']).to_string();
    let candidata_s = candidata.to_string_lossy().to_ascii_lowercase();
    candidata_s == raiz_s
        || candidata_s
            .strip_prefix(&raiz_s)
            .map(|resto| resto.starts_with(['/', '\\']))
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::SandboxArchivos;
    use std::fs;

    fn sandbox_tmp(nombre: &str) -> SandboxArchivos {
        let dir = std::env::temp_dir().join(format!("agente-sandbox-{nombre}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("crear tmp");
        SandboxArchivos::nuevo(&dir).expect("sandbox")
    }

    #[test]
    fn rechaza_escapar_con_dotdot() {
        let sb = sandbox_tmp("dotdot");
        let err = sb.resolver("../fuera.txt").unwrap_err();
        assert!(err.to_string().contains("'..'"));
    }

    #[test]
    fn rechaza_rutas_absolutas() {
        let sb = sandbox_tmp("abs");
        assert!(sb.resolver("C:\\Windows\\system32\\config").is_err());
    }

    #[test]
    fn bloquea_secretos_antes_de_leer() {
        let sb = sandbox_tmp("secrets");
        let ruta = sb.raiz.join(".env");
        fs::write(&ruta, "SECRETO=1").expect("escribir .env");
        assert!(sb.es_secreto(".env"));
        let err = sb.leer(".env", 1024).unwrap_err();
        assert!(err.to_string().contains("lista negra"));
        assert!(sb.es_secreto("carpeta/id_rsa"));
        assert!(sb.es_secreto("credenciales.pem"));
        assert!(sb.es_secreto(".git/config"));
        assert!(!sb.es_secreto("notas.txt"));
    }

    #[test]
    fn lee_y_escribe_dentro_del_workspace() {
        let sb = sandbox_tmp("rw");
        let escrita = sb.escribir("sub/dir/nota.txt", "hola").expect("escribir");
        assert!(escrita.exists());
        let (contenido, truncado) = sb.leer("sub/dir/nota.txt", 1024).expect("leer");
        assert_eq!(contenido, "hola");
        assert!(!truncado);
    }

    #[test]
    fn trunca_con_aviso() {
        let sb = sandbox_tmp("trunc");
        sb.escribir("grande.txt", &"x".repeat(5000)).expect("escribir");
        let (contenido, truncado) = sb.leer("grande.txt", 100).expect("leer");
        assert_eq!(contenido.len(), 100);
        assert!(truncado);
    }

    #[test]
    fn ruta_presentable_incluye_raiz_y_quita_prefijo_verbatim() {
        let sb = sandbox_tmp("ruta");
        let presentable = sb.ruta_presentable("sub/nota.txt");
        /* Nunca debe aparecer el prefijo verbatim de Windows en el resumen. */
        assert!(!presentable.contains(r"\\?\"));
        /* Debe terminar en la ruta relativa y empezar con la raíz absoluta. */
        assert!(presentable.ends_with(&format!("sub\\nota.txt")) || presentable.ends_with("sub/nota.txt"));
        assert!(presentable.contains("agente-sandbox-ruta"));
    }
}
