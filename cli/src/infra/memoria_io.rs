//! [109A-2] Export/import de recuerdos en Markdown: un archivo por recuerdo
//! con frontmatter (`clave`, `origen`, `usos`, `ultimo_uso`, `actualizada_en`)
//! y el contenido en el cuerpo.
//!
//! Un archivo por recuerdo en vez de un único documento con varios: así el
//! export es diffeable y versionable por recuerdo (destino `project`, dentro
//! del repo) y no hay que inventar un delimitador entre entradas ni escapar
//! los `---` del cuerpo.
//!
//! El import es la frontera de entrada no confiable: aplica
//! [`sanitize_para_memoria`] a clave y contenido, y un recuerdo que parece una
//! credencial se rechaza con motivo en vez de guardarse.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use glory_harness_core::memoria::sanitize_para_memoria;
use glory_harness_core::ports::{AgentPersistence, AmbitoMemoria, MemoriaEntrada};
use uuid::Uuid;

/// Carpeta versionable dentro del área de trabajo (destino `project`).
pub const CARPETA_PROYECTO: &str = ".glory/memorias";

/// Render de un recuerdo a Markdown con frontmatter (termina en salto de
/// línea para que el archivo sea estable al reexportar).
pub fn render_recuerdo(entrada: &MemoriaEntrada) -> String {
    let ultimo = entrada
        .ultimo_uso
        .map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default();
    format!(
        "---\nclave: {}\norigen: {}\nusos: {}\nultimo_uso: {}\nactualizada_en: {}\n---\n{}\n",
        entrada.clave,
        entrada.origen,
        entrada.usos,
        ultimo,
        entrada
            .actualizada_en
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        entrada.contenido,
    )
}

/// Nombre de archivo seguro para una clave: minúsculas ASCII, el resto a `-`.
/// La clave real viaja en el frontmatter, así que el nombre es solo legible.
pub fn nombre_archivo(clave: &str) -> String {
    let mut salida = String::new();
    for c in clave.chars() {
        if c.is_ascii_alphanumeric() {
            salida.push(c.to_ascii_lowercase());
        } else if !salida.ends_with('-') && !salida.is_empty() {
            salida.push('-');
        }
    }
    let slug = salida.trim_matches('-');
    if slug.is_empty() {
        "recuerdo.md".to_string()
    } else {
        format!("{slug}.md")
    }
}

/// Ruta donde escribir `clave` dentro de `dir`: reutiliza el archivo que ya
/// contiene esa misma clave (reexportar no duplica) y, si el nombre ya está
/// tomado por otro recuerdo, añade un sufijo numérico.
pub fn ruta_libre(dir: &Path, clave: &str) -> PathBuf {
    let base = nombre_archivo(clave);
    let candidato = dir.join(&base);
    if !candidato.exists() || archivo_es_de(&candidato, clave) {
        return candidato;
    }
    let tallo = base.trim_end_matches(".md");
    for n in 2..1000 {
        let otro = dir.join(format!("{tallo}-{n}.md"));
        if !otro.exists() || archivo_es_de(&otro, clave) {
            return otro;
        }
    }
    dir.join(format!("{tallo}-{}.md", uuid::Uuid::new_v4()))
}

/// `true` si el archivo tiene ese recuerdo en su frontmatter.
fn archivo_es_de(ruta: &Path, clave: &str) -> bool {
    std::fs::read_to_string(ruta)
        .ok()
        .and_then(|texto| parsear_recuerdo(&texto).ok())
        .is_some_and(|e| e.clave == clave)
}

/// Markdown → recuerdo. Exige frontmatter válido con `clave` y cuerpo no
/// vacío, y aplica el sanitizado de memoria a ambos (import = entrada
/// externa): devuelve el motivo cuando el recuerdo no es admisible.
pub fn parsear_recuerdo(texto: &str) -> Result<MemoriaEntrada, String> {
    let resto = texto
        .strip_prefix("---")
        .ok_or_else(|| "sin frontmatter (falta la primera línea `---`)".to_string())?;
    let (cabecera, cuerpo) = resto
        .split_once("\n---")
        .ok_or_else(|| "frontmatter sin cierre (`---`)".to_string())?;
    let mut clave = None;
    let mut origen = String::new();
    let mut usos = 0u32;
    let mut ultimo_uso = None;
    let mut actualizada_en = None;
    for linea in cabecera.lines() {
        let Some((etiqueta, valor)) = linea.split_once(':') else {
            continue;
        };
        let valor = valor.trim();
        match etiqueta.trim() {
            "clave" => clave = Some(valor.to_string()),
            "origen" => origen = valor.to_string(),
            "usos" => usos = valor.parse::<u32>().unwrap_or(0),
            "ultimo_uso" => ultimo_uso = parsear_fecha(valor),
            "actualizada_en" => actualizada_en = parsear_fecha(valor),
            _ => {}
        }
    }
    let clave = clave
        .filter(|c| !c.is_empty())
        .ok_or_else(|| "frontmatter sin `clave`".to_string())?;
    let clave = sanitize_para_memoria(&clave)
        .ok_or_else(|| format!("la clave '{clave}' parece una credencial o está vacía"))?;
    let contenido = sanitize_para_memoria(cuerpo.trim())
        .ok_or_else(|| format!("el contenido de '{clave}' parece una credencial o está vacío"))?;
    Ok(MemoriaEntrada {
        clave,
        contenido,
        /* Sin fecha en el archivo se trata como nueva (`Utc::now`), igual que
         * una fila antigua sin `actualizada_en`. */
        actualizada_en: actualizada_en.unwrap_or_else(chrono::Utc::now),
        origen,
        usos,
        ultimo_uso,
    })
}

fn parsear_fecha(valor: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    if valor.is_empty() {
        return None;
    }
    chrono::DateTime::parse_from_rfc3339(valor)
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Resultado de importar una carpeta a un ámbito: cuántos entraron y por qué
/// se rechazó cada archivo omitido (nunca se calla un archivo saltado).
#[derive(Debug, Default, Clone)]
pub struct ResumenImport {
    pub importados: usize,
    pub omitidos: Vec<String>,
}

/// Escribe un archivo por recuerdo dentro de `dir` (lo crea si no existe) y
/// devuelve cuántos se escribieron.
///
/// Compartido por el subcomando `memoria exportar` del CLI y por el panel
/// "Memorias" del escritorio: el formato y la desambiguación de nombres viven
/// aquí una sola vez ([109A-3]).
pub fn exportar_carpeta(dir: &Path, entradas: &[MemoriaEntrada]) -> Result<usize, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("no se pudo crear {}: {e}", dir.display()))?;
    for entrada in entradas {
        let ruta = ruta_libre(dir, &entrada.clave);
        std::fs::write(&ruta, render_recuerdo(entrada))
            .map_err(|e| format!("no se pudo escribir {}: {e}", ruta.display()))?;
    }
    Ok(entradas.len())
}

/// Archivos `.md` de una carpeta, en orden estable (el import es reproducible).
pub fn archivos_markdown(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut rutas = Vec::new();
    let entradas =
        std::fs::read_dir(dir).map_err(|e| format!("no se pudo leer {}: {e}", dir.display()))?;
    for entrada in entradas {
        let entrada =
            entrada.map_err(|e| format!("no se pudo leer una entrada de {}: {e}", dir.display()))?;
        let ruta = entrada.path();
        let es_md = ruta
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("md"));
        if ruta.is_file() && es_md {
            rutas.push(ruta);
        }
    }
    rutas.sort();
    Ok(rutas)
}

/// Fusiona los `.md` de `dir` en el ámbito pedido (upsert por clave).
///
/// Cada archivo es entrada externa: pasa por [`parsear_recuerdo`] y uno que
/// parece una credencial se omite con motivo, sin guardar a medias ni abortar
/// el resto. Lo usan el subcomando `memoria importar` del CLI y el panel
/// "Memorias" del escritorio ([109A-3]).
pub async fn importar_carpeta(
    dir: &Path,
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    ambito: AmbitoMemoria,
) -> Result<ResumenImport, String> {
    let mut resumen = ResumenImport::default();
    for ruta in archivos_markdown(dir)? {
        let texto = std::fs::read_to_string(&ruta)
            .map_err(|e| format!("no se pudo leer {}: {e}", ruta.display()))?;
        match parsear_recuerdo(&texto) {
            Ok(entrada) => {
                persistencia
                    .memoria_upsert(user_id, ambito, &entrada)
                    .await
                    .map_err(|e| e.to_string())?;
                resumen.importados += 1;
            }
            Err(motivo) => resumen.omitidos.push(format!("{}: {motivo}", ruta.display())),
        }
    }
    Ok(resumen)
}

#[cfg(test)]
mod pruebas {
    //! [109A-2] Ida y vuelta del formato y frontera de entrada externa.
    use super::*;

    fn entrada(clave: &str, contenido: &str) -> MemoriaEntrada {
        MemoriaEntrada::nueva(clave.to_string(), contenido.to_string(), "prueba".to_string())
    }

    #[test]
    fn ida_y_vuelta_conserva_metadatos() {
        let original = MemoriaEntrada {
            clave: "preferencias-editor".to_string(),
            contenido: "Usa 4 espacios, no tabuladores.".to_string(),
            actualizada_en: chrono::Utc::now(),
            origen: "turno".to_string(),
            usos: 3,
            ultimo_uso: Some(chrono::Utc::now()),
        };
        let vuelta = parsear_recuerdo(&render_recuerdo(&original)).expect("parsea");
        assert_eq!(vuelta.clave, original.clave);
        assert_eq!(vuelta.contenido, original.contenido);
        assert_eq!(vuelta.origen, original.origen);
        assert_eq!(vuelta.usos, original.usos);
        // El render usa el mismo formato que la BD (segundos): la vuelta no
        // mueve el instante, solo pierde la parte sub-segundo.
        assert_eq!(
            vuelta.actualizada_en.timestamp(),
            original.actualizada_en.timestamp()
        );
        assert_eq!(
            vuelta.ultimo_uso.map(|d| d.timestamp()),
            original.ultimo_uso.map(|d| d.timestamp())
        );
    }

    #[test]
    fn sin_ultimo_uso_no_inventa_fecha() {
        let vuelta = parsear_recuerdo(&render_recuerdo(&entrada("nota", "cuerpo"))).expect("parsea");
        assert!(vuelta.ultimo_uso.is_none());
        assert_eq!(vuelta.usos, 0);
    }

    #[test]
    fn rechaza_credenciales_y_cuerpo_vacio() {
        // La clave también es entrada externa: pasa por el sanitizado.
        let clave_credencial = "---\nclave: ghp_9f2c8a\n---\nnota normal\n";
        assert!(
            parsear_recuerdo(clave_credencial).is_err(),
            "una clave con credencial se rechaza"
        );
        let cuerpo_credencial = "---\nclave: nota\n---\npassword = qwerty\n";
        assert!(
            parsear_recuerdo(cuerpo_credencial).is_err(),
            "un cuerpo con credencial se rechaza"
        );
        let sin_cuerpo = "---\nclave: nota\n---\n   \n";
        assert!(
            parsear_recuerdo(sin_cuerpo).is_err(),
            "sin contenido no hay recuerdo"
        );
    }

    #[test]
    fn frontmatter_invalido_es_error_con_motivo() {
        assert!(parsear_recuerdo("# Título\n\ncuerpo\n").is_err(), "sin frontmatter");
        assert!(
            parsear_recuerdo("---\norigen: turno\n---\ncuerpo\n").is_err(),
            "frontmatter sin clave"
        );
        assert!(
            parsear_recuerdo("---\nclave: nota\nsin cierre\n").is_err(),
            "frontmatter sin cerrar"
        );
    }

    #[test]
    fn nombre_archivo_es_seguro() {
        assert_eq!(nombre_archivo("Preferencias de editor"), "preferencias-de-editor.md");
        assert_eq!(nombre_archivo("a/b\\c:d"), "a-b-c-d.md");
        assert_eq!(nombre_archivo("///"), "recuerdo.md", "sin letras usa el nombre genérico");
        assert_eq!(nombre_archivo(""), "recuerdo.md");
    }

    #[test]
    fn ruta_libre_reutiliza_y_desambigua() {
        let dir = std::env::temp_dir().join(format!("glory-mem-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("carpeta temporal");

        let primera = ruta_libre(&dir, "color");
        std::fs::write(&primera, render_recuerdo(&entrada("color", "azul"))).expect("escribir");
        assert_eq!(ruta_libre(&dir, "color"), primera, "reexportar no duplica");

        // "color!" da el mismo slug pero es otro recuerdo: no debe pisarlo.
        let otra = ruta_libre(&dir, "color!");
        assert_ne!(otra, primera);
        assert_eq!(
            otra.file_name().and_then(|n| n.to_str()),
            Some("color-2.md")
        );
        std::fs::write(&otra, render_recuerdo(&entrada("color!", "rojo"))).expect("escribir");
        assert_eq!(ruta_libre(&dir, "color!"), otra, "cada clave conserva su archivo");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// [109A-3] El escritorio usa estas funciones de carpeta; la ida y vuelta
    /// completa (exportar → importar) debe conservar los recuerdos del ámbito.
    #[tokio::test]
    async fn exportar_e_importar_conservan_los_recuerdos() {
        let dir = std::env::temp_dir().join(format!("glory-mem-io-{}", uuid::Uuid::new_v4()));
        let tienda: Arc<dyn AgentPersistence> =
            Arc::new(crate::persistencia::PersistenciaMemoria::nuevo());
        let user_id = Uuid::new_v4();
        let ambito = AmbitoMemoria::Proyecto(Uuid::new_v4());
        tienda
            .memoria_upsert(user_id, ambito, &entrada("color", "azul"))
            .await
            .expect("siembra");

        let escritas = exportar_carpeta(&dir, &tienda.memoria_listar(user_id, ambito).await.expect("listar"))
            .expect("exportar");
        assert_eq!(escritas, 1);

        // Importar en otro ámbito NO toca el origen ni mezcla ámbitos.
        let otro = AmbitoMemoria::Proyecto(Uuid::new_v4());
        let resumen = importar_carpeta(&dir, &tienda, user_id, otro)
            .await
            .expect("importar");
        assert_eq!(resumen.importados, 1);
        assert!(resumen.omitidos.is_empty(), "{:?}", resumen.omitidos);
        assert_eq!(
            tienda.memoria_listar(user_id, otro).await.expect("listar").len(),
            1,
            "el destino recibe el recuerdo"
        );
        assert_eq!(
            tienda.memoria_listar(user_id, ambito).await.expect("listar").len(),
            1,
            "el origen no se duplica"
        );
        assert!(
            tienda
                .memoria_listar(user_id, AmbitoMemoria::Global)
                .await
                .expect("listar")
                .is_empty(),
            "el global queda intacto"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Un archivo con credencial se omite con motivo y no aborta el resto.
    #[tokio::test]
    async fn importar_omite_credenciales_sin_abortar() {
        let dir = std::env::temp_dir().join(format!("glory-mem-om-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("carpeta temporal");
        std::fs::write(dir.join("bueno.md"), render_recuerdo(&entrada("nota", "cuerpo"))).expect("ok");
        std::fs::write(dir.join("malo.md"), "---\nclave: token\n---\nghp_9f2c8ab1\n").expect("ok");

        let tienda: Arc<dyn AgentPersistence> =
            Arc::new(crate::persistencia::PersistenciaMemoria::nuevo());
        let user_id = Uuid::new_v4();
        let resumen = importar_carpeta(&dir, &tienda, user_id, AmbitoMemoria::Global)
            .await
            .expect("importar");
        assert_eq!(resumen.importados, 1);
        assert_eq!(resumen.omitidos.len(), 1, "{:?}", resumen.omitidos);
        assert!(
            resumen.omitidos[0].contains("credencial"),
            "el motivo explica el rechazo: {}",
            resumen.omitidos[0]
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
