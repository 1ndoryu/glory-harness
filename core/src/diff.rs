/* [31-08-2026] Diff de líneas minimalista (Fase 4, sin crate externo).
 * Calcula un diff unificado simple (LCS por líneas) entre dos textos y lo
 * devuelve como texto plano para mostrar en `<pre>` en el front: líneas
 * eliminadas con `-`, añadidas con `+`, contexto sin prefijo.
 * Acotado: si cualquiera de los textos supera MAX_LINEAS se devuelve un aviso
 * en lugar de un diff (la tool ya limita lecturas a 1MB). */

const MAX_LINEAS: usize = 4096;

/// Devuelve un diff unificado entre `antes` y `despues`, o `None` si son
/// idénticos. Si un archivo es demasiado grande para el diff LCS, devuelve
/// un texto de aviso (nunca un diff a medias).
pub fn diff_lineas(antes: &str, despues: &str) -> Option<String> {
    if antes == despues {
        return None;
    }
    let antes: Vec<&str> = antes.split('\n').collect();
    let despues: Vec<&str> = despues.split('\n').collect();
    if antes.len() > MAX_LINEAS || despues.len() > MAX_LINEAS {
        return Some(
            "AVISO: archivo demasiado grande para mostrar el diff (se omite).".to_string(),
        );
    }
    let (n, m) = (antes.len(), despues.len());
    /* DP: len[i][j] = LCS de antes[i..] y despues[j..]. */
    let mut len = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            len[i][j] = if antes[i] == despues[j] {
                len[i + 1][j + 1] + 1
            } else {
                len[i + 1][j].max(len[i][j + 1])
            };
        }
    }
    /* Reconstruir el camino LCS: pares (i, j) de líneas iguales. */
    let mut emparejadas_antes = vec![false; n];
    let mut emparejadas_despues = vec![false; m];
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if antes[i] == despues[j] && len[i][j] == len[i + 1][j + 1] + 1 {
            emparejadas_antes[i] = true;
            emparejadas_despues[j] = true;
            i += 1;
            j += 1;
        } else if len[i + 1][j] >= len[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    /* Emitir el diff caminando ambas secuencias: línea igual → contexto,
     * línea de `antes` sin par → `-`, línea de `despues` sin par → `+`. */
    let mut salida = String::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n || j < m {
        if i < n && j < m && emparejadas_antes[i] && emparejadas_despues[j] {
            salida.push_str(antes[i]);
            salida.push('\n');
            i += 1;
            j += 1;
        } else if i < n && !emparejadas_antes[i] {
            salida.push('-');
            salida.push_str(antes[i]);
            salida.push('\n');
            i += 1;
        } else {
            salida.push('+');
            salida.push_str(despues[j]);
            salida.push('\n');
            j += 1;
        }
    }
    Some(salida)
}

#[cfg(test)]
mod tests {
    use super::diff_lineas;

    #[test]
    fn identicos_no_generan_diff() {
        assert!(diff_lineas("hola\nmundo\n", "hola\nmundo\n").is_none());
    }

    #[test]
    fn linea_añadida_marca_mas() {
        let diff = diff_lineas("hola\n", "hola\nmundo\n").expect("diff");
        assert!(diff.contains("+mundo"));
        assert!(diff.contains("hola"));
    }

    #[test]
    fn linea_eliminada_marca_menos() {
        let diff = diff_lineas("hola\nmundo\n", "hola\n").expect("diff");
        assert!(diff.contains("-mundo"));
    }

    #[test]
    fn reemplazo_marca_menos_y_mas() {
        let diff = diff_lineas("a\nviejo\nb\n", "a\nnuevo\nb\n").expect("diff");
        assert!(diff.contains("-viejo"));
        assert!(diff.contains("+nuevo"));
        assert!(diff.contains("a"));
        assert!(diff.contains("b"));
    }
}
