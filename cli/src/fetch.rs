/* [04-09-2026] Proveedor de descarga HTTP del CLI para la tool `web_fetch`
 * (Bloque 3, Fase 1; puerto `WebFetchProvider` del núcleo). Implementación
 * ligera con reqwest (rustls): descarga acotada a `limite_bytes` por stream
 * (nunca la página entera en memoria) y extracción de texto simple y
 * determinista (título + cuerpo sin scripts/estilos/etiquetas). Los errores
 * HTTP no son éxito falso: se propagan como error del núcleo. */

use async_trait::async_trait;
use glory_harness_core::error::{Error as HarnessError, Result as HarnessResult};
use glory_harness_core::ports::{ContenidoWeb, WebFetchProvider};
use std::io::Write;

/// Límite de descarga total: el proveedor nunca acumula más allá del límite
/// pedido por la tool (default 20 KB) aunque la página sea enorme.
const TIEMPO_MAX_MS: u64 = 15_000;

pub struct FetchCli {
    /* [059A-S7] Builder guardado como Result: el build solo falla por backend
     * TLS/proxy mal configurado a nivel máquina, pero al propagarse en el
     * primer uso devuelve un error real al modelo en vez de panickear en
     * construcción (regla expect-produccion-rs). */
    cliente: Result<reqwest::Client, String>,
}

impl FetchCli {
    #[must_use]
    pub fn nuevo() -> Self {
        Self {
            cliente: reqwest::Client::builder()
                .timeout(std::time::Duration::from_millis(TIEMPO_MAX_MS))
                .user_agent(concat!("glory-harness/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| format!("no se pudo crear el cliente HTTP: {e}")),
        }
    }
}

impl Default for FetchCli {
    fn default() -> Self {
        Self::nuevo()
    }
}

#[async_trait]
impl WebFetchProvider for FetchCli {
    async fn obtener(&self, url: &str, limite_bytes: usize) -> HarnessResult<ContenidoWeb> {
        use futures_util::StreamExt;
        let cliente = self
            .cliente
            .as_ref()
            .map_err(|e| HarnessError::Interno(format!("web_fetch {url}: {e}")))?;
        let respuesta = cliente
            .get(url)
            .send()
            .await
            .map_err(|e| HarnessError::Interno(format!("web_fetch {url}: {e}")))?;
        if !respuesta.status().is_success() {
            return Err(HarnessError::Interno(format!(
                "web_fetch {url}: HTTP {}",
                respuesta.status()
            )));
        }
        let mut stream = respuesta.bytes_stream();
        let mut cuerpo: Vec<u8> = Vec::with_capacity(limite_bytes.min(64 * 1024));
        let mut cortado = false;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk
                .map_err(|e| HarnessError::Interno(format!("web_fetch {url}: {e}")))?;
            if cuerpo.len() + chunk.len() > limite_bytes {
                let resto = limite_bytes.saturating_sub(cuerpo.len());
                cuerpo.write_all(&chunk[..resto])
                    .map_err(|e| HarnessError::Interno(format!("web_fetch: {e}")))?;
                cortado = true;
                break;
            }
            cuerpo.write_all(&chunk)
                .map_err(|e| HarnessError::Interno(format!("web_fetch: {e}")))?;
        }
        let html = String::from_utf8_lossy(&cuerpo);
        let (titulo, texto) = extraer_texto(&html);
        let mut texto = texto;
        if cortado && !texto.is_empty() {
            texto.push_str("\n…(contenido truncado por límite de bytes)");
        }
        Ok(ContenidoWeb {
            url: url.to_string(),
            titulo,
            bytes: cuerpo.len(),
            texto,
        })
    }
}

/// Busca `aguja` (ASCII) en `paja` sin distinguir mayúsculas, desde `desde`.
fn buscar_ci(paja: &str, aguja: &str, desde: usize) -> Option<usize> {
    let paja = &paja[desde.min(paja.len())..];
    let mut i = 0;
    while i + aguja.len() <= paja.len() {
        if let Some(slice) = paja.get(i..i + aguja.len()) {
            if slice.eq_ignore_ascii_case(aguja) {
                return Some(desde + i);
            }
        }
        i += 1;
    }
    None
}

/// Extrae `<title>` y el texto legible de un HTML (sin scripts ni estilos).
fn extraer_texto(html: &str) -> (Option<String>, String) {
    let titulo = buscar_ci(html, "<title", 0)
        .and_then(|ini| {
            let desde = ini + "<title".len();
            buscar_ci(html, ">", desde).map(|cierre| cierre + 1)
        })
        .and_then(|inicio| buscar_ci(html, "</title", inicio).map(|fin| (inicio, fin)))
        .map(|(inicio, fin)| html[inicio..fin].trim().to_string())
        .filter(|t| !t.is_empty());

    // Marca de inicio del cuerpo; si no existe, procesa todo.
    let desde = buscar_ci(html, "<body", 0)
        .and_then(|ini| buscar_ci(html, ">", ini).map(|cierre| cierre + 1))
        .unwrap_or(0);

    let mut texto = html[desde..].to_string();
    // Quita comentarios, scripts y estilos (bloques, ci).
    for bloque in ["script", "style", "noscript"] {
        let mut out = String::with_capacity(texto.len());
        let mut resto = texto.as_str();
        loop {
            let ini = buscar_ci(resto, &format!("<{bloque}"), 0);
            let Some(ini) = ini else {
                out.push_str(resto);
                break;
            };
            out.push_str(&resto[..ini]);
            let resto_restante = &resto[ini..];
            let fin = buscar_ci(resto_restante, &format!("</{bloque}"), 0).map(|f| {
                buscar_ci(&resto_restante[f..], ">", 0).map(|g| f + g + 1)
            });
            match fin {
                Some(Some(fin)) => {
                    resto = &resto_restante[fin..];
                }
                _ => {
                    // Bloque sin cierre: descarta el resto.
                    resto = "";
                    break;
                }
            }
        }
        texto = out;
    }

    // Quita etiquetas restantes y colapsa blancos.
    let mut legible = String::with_capacity(texto.len());
    let mut en_etiqueta = false;
    for ch in texto.chars() {
        match ch {
            '<' => en_etiqueta = true,
            '>' if en_etiqueta => en_etiqueta = false,
            _ if !en_etiqueta => legible.push(ch),
            _ => {}
        }
    }
    let legible = desescapar(&legible);
    let legible = legible
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .to_string();
    (titulo, legible)
}

/// Desescapa las entidades HTML más comunes.
fn desescapar(texto: &str) -> String {
    texto
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_titulo_y_texto_sin_scripts() {
        let html = "<html><head><title>Doc de prueba</title></head><body>\
                    <h1>Encabezado</h1><p>Hola <b>mundo</b></p>\
                    <script>var x = \"<b>falso</b>\";</script>\
                    <style>p { color: red; }</style>\
                    </body></html>";
        let (titulo, texto) = extraer_texto(html);
        assert_eq!(titulo.as_deref(), Some("Doc de prueba"));
        assert!(texto.contains("Encabezado"));
        assert!(texto.contains("Hola mundo"));
        assert!(!texto.contains("falso"), "sin contenido de script: {texto}");
        assert!(!texto.contains("color: red"));
    }

    #[test]
    fn desescapa_entidades() {
        assert_eq!(desescapar("a &amp; b &lt;c&gt; &quot;d&quot;"), "a & b <c> \"d\"");
    }
}
