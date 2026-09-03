/* [31-08-2026] Diff de líneas minimalista (Fase 4, sin crate externo).
 * Calcula un diff por hunks (LCS por líneas) entre dos textos y lo devuelve
 * como texto plano para mostrar en `<pre>` en el front: líneas eliminadas con
 * `-`, añadidas con `+`, contexto con un espacio, hunks separados con cabecera
 * `@@ -a,n +b,m @@`.
 * [039A-2 03-09-2026] El diff emitía el archivo COMPLETO como contexto (una
 * edición de 1 línea mostraba miles de líneas sin cambios). Ahora cada cambio
 * se rodea de CONTEXTO líneas y los tramos sin cambios se eliden con
 * `… N líneas sin cambios …`: la tarjeta muestra la parte específica cambiada.
 * Acotado: si cualquiera de los textos supera MAX_LINEAS se devuelve un aviso
 * en lugar de un diff (la tool ya limita lecturas a 1MB). */

const MAX_LINEAS: usize = 4096;

/// Líneas de contexto sin cambios alrededor de cada cambio.
const CONTEXTO: usize = 3;

/// Devuelve un diff por hunks entre `antes` y `despues`, o `None` si son
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
    /* Emitir el diff por hunks: cada cambio (línea `-`/`+`) se rodea de
     * CONTEXTO líneas de contexto; los tramos intermedios sin cambios se
     * eliden con un placeholder que cuenta las líneas ocultas. */
    #[derive(Clone, Copy)]
    enum Op {
        Igual(usize),
        Baja(usize),
        Alta(usize),
    }
    let mut ops: Vec<Op> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n || j < m {
        if i < n && j < m && emparejadas_antes[i] && emparejadas_despues[j] {
            ops.push(Op::Igual(i));
            i += 1;
            j += 1;
        } else if i < n && !emparejadas_antes[i] {
            ops.push(Op::Baja(i));
            i += 1;
        } else {
            ops.push(Op::Alta(j));
            j += 1;
        }
    }
    /* Intervalos de ops visibles: cada cambio ± CONTEXTO, fusionados si se
     * solapan. Sin cambios no hay hunks (pero antes != despues garantiza
     * al menos uno: split con trailing '\n' puede producirlo vacío). */
    let mut intervalos: Vec<(usize, usize)> = Vec::new();
    for (k, op) in ops.iter().enumerate() {
        if matches!(op, Op::Igual(..)) {
            continue;
        }
        let inicio = k.saturating_sub(CONTEXTO);
        let fin = (k + CONTEXTO + 1).min(ops.len());
        if let Some(ultimo) = intervalos.last_mut() {
            if inicio <= ultimo.1 {
                ultimo.1 = ultimo.1.max(fin);
                continue;
            }
        }
        intervalos.push((inicio, fin));
    }
    let mut salida = String::new();
    let mut anterior_fin = 0usize;
    for &(inicio, fin) in &intervalos {
        let elididas = ops[anterior_fin..inicio]
            .iter()
            .filter(|op| matches!(op, Op::Igual(..)))
            .count();
        if elididas > 0 {
            salida.push_str(&format!("… {elididas} líneas sin cambios …\n"));
        }
        /* Cabecera del hunk con numeración 1-based de cada lado. */
        let (mut la, mut lb) = (0usize, 0usize);
        for op in &ops[..inicio] {
            match *op {
                Op::Igual(..) | Op::Baja(_) => la += 1,
                Op::Alta(_) => {}
            }
            match *op {
                Op::Igual(..) | Op::Alta(_) => lb += 1,
                Op::Baja(_) => {}
            }
        }
        let (mut ca, mut cb) = (0usize, 0usize);
        for op in &ops[inicio..fin] {
            match *op {
                Op::Igual(..) | Op::Baja(_) => ca += 1,
                Op::Alta(_) => {}
            }
            match *op {
                Op::Igual(..) | Op::Alta(_) => cb += 1,
                Op::Baja(_) => {}
            }
        }
        salida.push_str(&format!("@@ -{},{} +{},{} @@\n", la + 1, ca, lb + 1, cb));
        for op in &ops[inicio..fin] {
            match *op {
                Op::Igual(a) => {
                    salida.push(' ');
                    salida.push_str(antes[a]);
                    salida.push('\n');
                }
                Op::Baja(a) => {
                    salida.push('-');
                    salida.push_str(antes[a]);
                    salida.push('\n');
                }
                Op::Alta(b) => {
                    salida.push('+');
                    salida.push_str(despues[b]);
                    salida.push('\n');
                }
            }
        }
        anterior_fin = fin;
    }
    let elididas = ops[anterior_fin..]
        .iter()
        .filter(|op| matches!(op, Op::Igual(..)))
        .count();
    if elididas > 0 {
        salida.push_str(&format!("… {elididas} líneas sin cambios …\n"));
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

    /* [039A-2] El diff colapsa el contexto lejano: un cambio entre 20 líneas
     * iguales solo muestra ±3 de contexto + placeholders con el conteo. */
    #[test]
    fn hunks_colapsan_contexto_largo() {
        let contexto: Vec<String> = (1..=20).map(|k| format!("linea{k:02}")).collect();
        let antes = contexto.join("\n");
        let mut despues_vec = contexto.clone();
        despues_vec[10] = "CAMBIO".to_string();
        let despues = despues_vec.join("\n");
        let diff = diff_lineas(&antes, &despues).expect("diff");
        assert!(diff.contains("-linea11"));
        assert!(diff.contains("+CAMBIO"));
        /* Contexto cercano visible (±3): linea08 y linea14 sí; linea01 no. */
        assert!(diff.contains("linea08"));
        assert!(diff.contains("linea14"));
        assert!(!diff.contains("linea01"));
        assert!(!diff.contains("linea20"));
        /* Placeholders honestos: 8 elididas antes (linea01-08… espera:
         * hunk cubre linea08-14 → elididas linea01-07 = 7) y cola. */
        assert!(diff.contains("… 7 líneas sin cambios …"));
        assert!(diff.contains("@@"));
    }

    #[test]
    fn cambios_juntos_fusionan_en_un_hunk() {
        let diff = diff_lineas("a\nb1\nb2\nc\n", "a\nB1\nB2\nc\n").expect("diff");
        assert_eq!(diff.matches("@@").count(), 2); // una cabecera = 2 marcas
        assert!(diff.contains("-b1"));
        assert!(diff.contains("+B1"));
    }

    #[test]
    fn cambios_separados_generan_dos_hunks() {
        let antes = (1..=20).map(|k| format!("l{k}")).collect::<Vec<_>>().join("\n");
        let mut v: Vec<String> = (1..=20).map(|k| format!("l{k}")).collect();
        v[1] = "X".to_string();
        v[17] = "Y".to_string();
        let diff = diff_lineas(&antes, &v.join("\n")).expect("diff");
        assert_eq!(diff.matches("@@").count(), 4); // dos cabeceras
        assert!(diff.contains("+X"));
        assert!(diff.contains("+Y"));
    }
}
