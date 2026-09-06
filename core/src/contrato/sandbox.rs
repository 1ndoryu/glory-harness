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
use std::io::BufRead;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};

/// [039A-3 P3] Puerto opcional de respaldo de archivos. `SandboxArchivos::`
/// `escribir` llama a este hook ANTES de cada escritura con la ruta relativa
/// **ya validada por el sandbox** y el contenido previo **completo en bytes**
/// (decisión §6.5: no truncado a 1 MB, o restaurar un archivo >1 MB quedaría
/// corrupto). La implementación por defecto (ausente) es no-op: CLI/task no
/// respaldan y el comportamiento de `escribir` no cambia.
///
/// El respaldo es observación, no contrato: si el hook falla, `escribir`
/// continúa (fail-open con log, §2.3) — un respaldo que falla no debe tumbar
/// el turno del agente.
pub trait RespaldoArchivos: Send + Sync {
    /// Guarda el respaldo del contenido previo. `relativa` está normalizada
    /// (ya resuelta por el sandbox, sin `..` ni escapes). `previo_bytes` es el
    /// contenido COMPLETO anterior (vacío si el archivo no existía) y
    /// `nuevo_contenido` es el texto que se va a escribir (para el hash
    /// posterior). El implementador decide la política (dedup, retención,
    /// índice por conversación).
    fn respaldar(
        &self,
        relativa: &str,
        previo_bytes: &[u8],
        nuevo_contenido: &str,
    ) -> Result<(), Error>;
}

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
pub struct SandboxArchivos {
    raiz: PathBuf,
    /// [039A-3 P3] Puerto opcional de respaldo, inyectable por el consumidor
    /// tras construir el runtime (`registry.sandbox()` → `con_respaldo`).
    /// Interior-mutable para no cambiar la API de `escribir` ni exigir `&mut`
    /// en todos los llamadores (tools, plan, todo). `None` → no-op.
    respaldo: Mutex<Option<Arc<dyn RespaldoArchivos>>>,
}

impl SandboxArchivos {
    /// Crea el sandbox sobre `raiz` (la canonicaliza; error si no se puede).
    pub fn nuevo(raiz: impl Into<PathBuf>) -> Result<Self, Error> {
        let raiz = raiz.into();
        std::fs::create_dir_all(&raiz).map_err(|error| {
            Error::Validacion(format!("No se pudo crear el workspace: {error}"))
        })?;
        let canonica = std::fs::canonicalize(&raiz)
            .map_err(|error| Error::Validacion(format!("Workspace no accesible: {error}")))?;
        Ok(Self {
            raiz: canonica,
            respaldo: Mutex::new(None),
        })
    }

    /// [039A-3 P3] Inyecta (o retira, con `None`) el puerto de respaldo. No-op
    /// por defecto; solo el consumidor que quiera vault lo llama, una vez tras
    /// construir el runtime. La exclusión de `.glory-harness/` (§6.4) impide
    /// que el agente escriba bajo su propio vault.
    pub fn con_respaldo(&self, respaldo: Option<Arc<dyn RespaldoArchivos>>) {
        if let Ok(mut r) = self.respaldo.lock() {
            *r = respaldo;
        }
    }

    /// Resuelve una ruta relativa al workspace y valida que quede DENTRO.
    /// Devuelve la ruta canónica (resuelve junctions/symlinks y `..`).
    pub fn resolver(&self, relativa: &str) -> Result<PathBuf, Error> {
        if relativa.trim().is_empty() {
            return Err(Error::Validacion("Ruta vacía".into()));
        }
        let ruta = Path::new(relativa);
        if ruta.is_absolute() {
            return Err(Error::Sandbox("Solo rutas relativas al workspace".into()));
        }
        /* Prohibir `..` explícitamente (defensa en profundidad). */
        for componente in ruta.components() {
            if matches!(componente, Component::ParentDir) {
                return Err(Error::Sandbox("No se permiten rutas con '..'".into()));
            }
        }
        let candidata = self.raiz.join(ruta);
        let canonica = std::fs::canonicalize(&candidata)
            .map_err(|_| Error::NoEncontrado("La ruta no existe dentro del workspace".into()))?;
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
        let normalizada = relativa
            .replace('\\', "/")
            .trim_start_matches("./")
            .to_string();
        let segmentos: Vec<&str> = normalizada.split('/').collect();
        /* [039A-3 P3 §6.4] `.glory-harness/` (vault de respaldos del harness)
         * es zona interna BLOQUEADA: si el agente pudiera escribir ahí podría
         * envenenar sus propios respaldos, y leerlos contaminaría su contexto
         * con bytes binarios/duplicados. Primera comprobación (antes que los
         * secretos de archivo) porque es un directorio de infraestructura. */
        if let Some(primero) = segmentos.first() {
            /* `segmentos` ya no contiene '/' (split previo): basta comparar el
             * primer segmento. Un archivo llamado `.glory-harness-copia.txt`
             * en un subdirectorio NO es la zona (primer segmento distinto). */
            if primero.eq_ignore_ascii_case(".glory-harness") {
                return true;
            }
        }
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
            if SECRET_PREFIJOS
                .iter()
                .any(|p| nombre.ends_with(&p.trim_start_matches('*')))
            {
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

    /// Lee una ventana de líneas (1-based) del archivo, materializando solo el
    /// rango pedido (plan 318A-16 F4). Devuelve el texto del rango, el total de
    /// líneas del archivo y si quedan más líneas después del rango.
    /// Fail-closed: `offset` 0 o mayor que el total de líneas es error, salvo
    /// archivo vacío con `offset` 1 (ventana vacía válida).
    pub fn leer_rango_lineas(
        &self,
        relativa: &str,
        offset: usize,
        limite: usize,
    ) -> Result<(String, usize, bool), Error> {
        if offset == 0 || limite == 0 {
            return Err(Error::Argumentos(
                "offset_linea >= 1 y limite_lineas >= 1".into(),
            ));
        }
        if self.es_secreto(relativa) {
            return Err(Error::Sandbox(
                "El archivo está en la lista negra de secretos y no se puede leer".into(),
            ));
        }
        let ruta = self.resolver(relativa)?;
        let archivo = std::fs::File::open(&ruta)
            .map_err(|error| Error::NoEncontrado(format!("No se pudo leer: {error}")))?;
        let lector = std::io::BufReader::new(archivo);
        let mut lineas = lector.lines();
        // Avanzamos hasta la primera línea del rango (guardando su texto).
        let mut total = 0usize;
        let mut saltadas = 0usize;
        let mut recogidas: Vec<String> = Vec::with_capacity(limite);
        loop {
            match lineas.next() {
                None => break,
                Some(linea) => {
                    total += 1;
                    let linea = linea.map_err(|error| {
                        Error::Validacion(format!("No se pudo leer línea: {error}"))
                    })?;
                    if saltadas < offset - 1 {
                        saltadas += 1;
                    } else if recogidas.len() < limite {
                        recogidas.push(linea);
                    } else {
                        // Ya completamos la ventana; solo contamos el resto.
                        total += lineas.count();
                        break;
                    }
                }
            }
        }
        if offset > total && total != 0 {
            return Err(Error::Argumentos(format!(
                "offset_linea {offset} fuera de rango: el archivo tiene {total} líneas"
            )));
        }
        if offset > 1 && total == 0 {
            return Err(Error::Argumentos(
                "offset_linea fuera de rango: el archivo está vacío".into(),
            ));
        }
        let fin_rango = offset + recogidas.len() - 1;
        let hay_mas = fin_rango < total;
        Ok((recogidas.join("\n"), total, hay_mas))
    }

    /// Escribe un archivo (crea directorios intermedios). Solo archivos.
    ///
    /// [039A-3 P3] Antes de escribir invoca el hook opcional de respaldo
    /// (trait `RespaldoArchivos`) con la `relativa` **ya validada** y el
    /// contenido previo **completo en bytes** (decisión §6.5: leer el archivo
    /// existente con `std::fs::read`, nunca el string truncado que las tools
    /// usan para el diff — restaurar un archivo >1 MB con el previo truncado
    /// produciría un archivo corrupto). El hook es no-op por defecto y su
    /// fallo NO tumba la escritura (fail-open con log: el respaldo es
    /// observación, no contrato del turno).
    pub fn escribir(&self, relativa: &str, contenido: &str) -> Result<PathBuf, Error> {
        if self.es_secreto(relativa) {
            return Err(Error::Sandbox(
                "El archivo está en la lista negra de secretos y no se puede escribir".into(),
            ));
        }
        let ruta = self.resolver_para_escribir(relativa)?;
        /* Ruta relativa CANÓNICA (separador `/`, sin `./`) derivada de la ruta
         * resuelta: el vault usa esta clave para el árbol `<hash>/<ruta>` y el
         * índice JSONL, independiente de cómo el llamador pasó la ruta (con
         * `./`, con `\`, etc.). */
        let relativa_limpia = ruta
            .strip_prefix(&self.raiz)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| relativa.replace('\\', "/"));
        /* Previo completo en bytes (archivo existente o vacío si no existe).
         * Se captura ANTES de `create_dir_all`/`write` para que el respaldo
         * refleje el estado previo real. Fail-open: si la lectura falla por
         * un motivo distinto a "no existe", se continúa sin respaldar (un
         * fallo de respaldo no debe bloquear la escritura legítima). */
        let previo_bytes = std::fs::read(&ruta).unwrap_or_default();
        if let Some(respaldo) = self
            .respaldo
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            if let Err(error) = respaldo.respaldar(&relativa_limpia, &previo_bytes, contenido) {
                tracing::warn!(
                    %error,
                    %relativa,
                    "respaldo de archivo omitido (fail-open): la escritura continúa"
                );
            }
        }
        if let Some(parent) = ruta.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                Error::Validacion(format!("No se pudo crear directorios: {error}"))
            })?;
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
            return Err(Error::Sandbox("Solo rutas relativas al workspace".into()));
        }
        for componente in ruta.components() {
            if matches!(componente, Component::ParentDir) {
                return Err(Error::Sandbox("No se permiten rutas con '..'".into()));
            }
        }
        /* El padre debe quedar dentro (o ser el propio workspace). */
        let padre = self
            .raiz
            .join(ruta.parent().unwrap_or_else(|| Path::new("")));
        let padre_canonico = if padre.exists() {
            std::fs::canonicalize(&padre)
                .map_err(|error| Error::Validacion(format!("Directorio no accesible: {error}")))?
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
        Ok(padre_canonico.join(
            ruta.file_name()
                .ok_or_else(|| Error::Validacion("La ruta no tiene nombre de archivo".into()))?,
        ))
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
        let dir =
            std::env::temp_dir().join(format!("agente-sandbox-{nombre}-{}", std::process::id()));
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
        sb.escribir("grande.txt", &"x".repeat(5000))
            .expect("escribir");
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
        assert!(presentable.ends_with("sub\\nota.txt") || presentable.ends_with("sub/nota.txt"));
        assert!(presentable.contains("agente-sandbox-ruta"));
    }

    /* ===== [039A-3 P3] Vault: hook opcional de respaldo (fixture 4) ===== */

    /// Hook de prueba: registra cada llamada (relativa, previo, nuevo) en un
    /// Vec compartido. Devuelve `Ok(())` salvo `fallar` → error (para probar
    /// que un respaldo que falla NO tumba la escritura: fail-open).
    struct RespaldoRegistro {
        llamadas: std::sync::Mutex<Vec<(String, Vec<u8>, String)>>,
        fallar: bool,
    }

    impl super::RespaldoArchivos for RespaldoRegistro {
        fn respaldar(
            &self,
            relativa: &str,
            previo_bytes: &[u8],
            nuevo_contenido: &str,
        ) -> Result<(), crate::error::Error> {
            if self.fallar {
                return Err(crate::error::Error::Persistencia("fallo simulado".into()));
            }
            self.llamadas.lock().unwrap().push((
                relativa.to_string(),
                previo_bytes.to_vec(),
                nuevo_contenido.to_string(),
            ));
            Ok(())
        }
    }

    fn respaldo_llamadas(sb: &SandboxArchivos) -> std::sync::Arc<RespaldoRegistro> {
        let r = std::sync::Arc::new(RespaldoRegistro {
            llamadas: std::sync::Mutex::new(Vec::new()),
            fallar: false,
        });
        sb.con_respaldo(Some(r.clone()));
        r
    }

    #[test]
    fn vault_sin_hook_es_noop_y_escribe_normal() {
        let sb = sandbox_tmp("vault-noop");
        // Sin con_respaldo → el hook no existe → escribir funciona igual.
        let escrita = sb.escribir("sub/nota.txt", "hola").expect("escribir");
        assert!(escrita.exists());
        let (contenido, _) = sb.leer("sub/nota.txt", 1024).expect("leer");
        assert_eq!(contenido, "hola");
    }

    #[test]
    fn vault_respalda_previo_completo_y_relativa_normalizada() {
        let sb = sandbox_tmp("vault-hook");
        sb.escribir("sub/nota.txt", "original")
            .expect("escribir primera");
        let r = respaldo_llamadas(&sb);

        // Sobrescritura con ruta que usa "./" y "\" → la relativa debe quedar
        // normalizada ("sub/nota.txt", sin "./" ni backslashes).
        sb.escribir(r".\sub\nota.txt", "cambiada")
            .expect("escribir segunda");
        let llamadas = r.llamadas.lock().unwrap();
        assert_eq!(llamadas.len(), 1);
        let (relativa, previo, nuevo) = &llamadas[0];
        assert_eq!(relativa, "sub/nota.txt");
        // Previo COMPLETO en bytes, no truncado ni vacío.
        assert_eq!(previo, b"original");
        assert_eq!(nuevo, "cambiada");
    }

    #[test]
    fn vault_archivo_nuevo_respalda_previo_vacio() {
        let sb = sandbox_tmp("vault-nuevo");
        let r = respaldo_llamadas(&sb);
        // Archivo que no existía → previo_bytes vacío (caso "nuevo archivo").
        sb.escribir("nuevo.txt", "contenido").expect("escribir");
        let llamadas = r.llamadas.lock().unwrap();
        assert_eq!(llamadas.len(), 1);
        assert!(llamadas[0].1.is_empty());
        assert_eq!(llamadas[0].2, "contenido");
    }

    #[test]
    fn vault_fallo_del_hook_no_tumba_la_escritura() {
        let sb = sandbox_tmp("vault-fallar");
        sb.escribir("nota.txt", "antes").expect("escribir antes");
        // Hook que falla: fail-open → la escritura continúa.
        let r = std::sync::Arc::new(RespaldoRegistro {
            llamadas: std::sync::Mutex::new(Vec::new()),
            fallar: true,
        });
        sb.con_respaldo(Some(r.clone()));
        sb.escribir("nota.txt", "después")
            .expect("escribir pese al fallo del hook");
        let (contenido, _) = sb.leer("nota.txt", 1024).expect("leer");
        assert_eq!(contenido, "después");
    }

    #[test]
    fn vault_quita_hook_con_none() {
        let sb = sandbox_tmp("vault-quitar");
        sb.escribir("nota.txt", "v1").expect("escribir v1");
        let r = respaldo_llamadas(&sb);
        sb.escribir("nota.txt", "v2").expect("escribir v2");
        assert_eq!(r.llamadas.lock().unwrap().len(), 1);

        // Retirar el hook → ya no se respalda.
        sb.con_respaldo(None);
        sb.escribir("nota.txt", "v3").expect("escribir v3");
        assert_eq!(r.llamadas.lock().unwrap().len(), 1);
    }

    #[test]
    fn glory_harness_es_secreto_y_no_se_escribe() {
        let sb = sandbox_tmp("vault-zona");
        // La zona interna está bloqueada para leer y escribir.
        assert!(sb.es_secreto(".glory-harness/backups/abc/nota.txt"));
        assert!(sb.es_secreto(".glory-harness"));
        assert!(sb.es_secreto(r".glory-harness\backups\x"));
        let err = sb
            .escribir(".glory-harness/backups/abc/nota.txt", "x")
            .unwrap_err();
        assert!(err.to_string().contains("lista negra"));
        // Archivo normal NO es secreto.
        assert!(!sb.es_secreto("notas.txt"));
        assert!(!sb.es_secreto("sub/.glory-harness-copia.txt"));
    }
}
