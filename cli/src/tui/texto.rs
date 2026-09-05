//! [059A-15 S2] Split mecánico de `tui.rs`: helpers de texto puro (byte_index/envolver_*). Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::*;

pub(crate) fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Envuelve un texto plano en filas de ancho visual ≤ `ancho`, cortando en los
/// espacios (word wrap). Una palabra más ancha que `ancho` se parte por
/// caracteres para no desbordar. Devuelve filas sin espacios finales.
pub(crate) fn envolver_linea(texto: &str, ancho: usize) -> Vec<String> {
    let mut filas: Vec<String> = Vec::new();
    if ancho == 0 || texto.is_empty() {
        return filas;
    }
    let mut fila = String::new();
    let mut ancho_fila = 0usize;
    for token in texto.split_inclusive(' ') {
        // `split_inclusive` deja el espacio al final de cada token salvo el último.
        let (palabra, tiene_espacio) = match token.strip_suffix(' ') {
            Some(p) => (p, true),
            None => (token, false),
        };
        let ancho_palabra = UnicodeWidthStr::width(palabra);
        let ancho_token = ancho_palabra + usize::from(tiene_espacio);

        if ancho_fila + ancho_token <= ancho {
            fila.push_str(palabra);
            if tiene_espacio {
                fila.push(' ');
            }
            ancho_fila += ancho_token;
            continue;
        }

        // No cabe en la fila actual → cerrar la fila y empezar otra.
        if !fila.is_empty() {
            filas.push(fila.trim_end().to_string());
            fila.clear();
            ancho_fila = 0;
        }
        if ancho_palabra > ancho {
            // Palabra más ancha que la línea: partir por caracteres.
            let mut resto = palabra;
            while !resto.is_empty() {
                let mut trozo = String::new();
                let mut w = 0usize;
                for ch in resto.chars() {
                    let cw = UnicodeWidthStr::width(ch.to_string().as_str());
                    if w + cw > ancho {
                        break;
                    }
                    trozo.push(ch);
                    w += cw;
                }
                let tam = trozo.len();
                filas.push(trozo);
                resto = &resto[tam..];
            }
        } else {
            fila.push_str(palabra);
            if tiene_espacio {
                fila.push(' ');
            }
            ancho_fila += ancho_token;
        }
    }
    if !fila.is_empty() {
        filas.push(fila.trim_end().to_string());
    }
    filas
}

/// Envuelve un texto con un prefijo en la primera fila y un colgante del mismo
/// ancho en las siguientes (indentado de párrafo). Útil para el cuerpo del
/// usuario y las tools.
pub(crate) fn envolver_con_prefijo(texto: &str, ancho: usize, prefijo: &str) -> Vec<String> {
    let ancho_prefijo = UnicodeWidthStr::width(prefijo);
    let ancho_util = ancho.saturating_sub(ancho_prefijo).max(1);
    let mut filas = envolver_linea(texto, ancho_util);
    if filas.is_empty() {
        return filas;
    }
    let relleno = " ".repeat(ancho_prefijo);
    for (i, fila) in filas.iter_mut().enumerate() {
        let indent = if i == 0 { prefijo } else { &relleno };
        *fila = format!("{indent}{fila}");
    }
    filas
}
