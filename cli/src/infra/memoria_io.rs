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

use glory_harness_core::memoria::sanitize_para_memoria;
use glory_harness_core::ports::MemoriaEntrada;

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
}
