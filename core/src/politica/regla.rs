/* [03-09-2026] Motor de reglas de permiso v2 (plan 318A-16, F1): categorías +
 * patrones con wildcard propio, evaluadas con **última coincidencia gana**
 * (patrón opencode `permission/index.ts:28-32`: `findLast` sobre el ruleset
 * ordenado; deny corta solo cuando es la última coincidencia; el default sin
 * regla es `ask`).
 *
 * Diferencia con F3 (318A-15): F3 es permiso por TOOL (entera); aquí el
 * usuario puede decir "esta *clase* de acción" — `escritura_fuera_repo`,
 * `comando:git *`, `red` — sin abrir la tool entera. Las claves de una llamada
 * se resuelven en orden de especificidad (derivada del argumento → categoría
 * estática de la tool → id de la tool); la primera clave con reglas decide.
 *
 * Wildcard propio (sin crate externo): `*` casa cualquier texto SIN cruzar el
 * separador `/` (patrones de ruta segmento a segmento); `**` cruza
 * separadores (subárboles). Los patrones y valores se normalizan `\` → `/`
 * (rutas Windows) antes de comparar.
 */

use crate::permiso::Permiso;
use serde_json::Value;

/* Categorías del plan (port de `external_directory`/`bash`/`edit`/`webfetch`
 * de opencode `code-mode.ts`). */
pub const CAT_ESCRITURA: &str = "escritura";
pub const CAT_ESCRITURA_FUERA: &str = "escritura_fuera_repo";
pub const CAT_LECTURA: &str = "lectura";
pub const CAT_LECTURA_FUERA: &str = "lectura_fuera_repo";
pub const CAT_RED: &str = "red";
pub const CAT_SUBAGENTE: &str = "subagente";
pub const CAT_TODO: &str = "todo";
/// [318A-16 F3] Categoría base de comandos. La clave derivada de una llamada
/// es `comando:<nivel>` (seguro/bajo/medio/alto/critico) para que "permitir
/// siempre" cubra un TIPO de comando, no el comando exacto.
pub const CAT_COMANDO: &str = "comando";
/// [Bloque 3, F2] Tools expuestas por servidores MCP: efecto=true → ask en
/// predeterminado, deny en meta/plan; una regla `mcp:*` o `mcp:<servidor>:*`
/// (clave derivada del id de la tool) controla todo un servidor.
pub const CAT_MCP: &str = "mcp";

/// Una regla de permiso: para la categoría `categoria`, si el valor concreto
/// de la llamada coincide con `patron` (wildcard `*`/`**`), aplicar `accion`.
/// La lista está ordenada por inserción; la ÚLTIMA coincidencia gana
/// (findLast de opencode): las aprobaciones de la sesión se agregan después de
/// las reglas de configuración y, por tanto, mandan sobre ellas.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReglaPermiso {
    pub categoria: String,
    pub patron: String,
    pub accion: Permiso,
}

impl ReglaPermiso {
    #[must_use]
    pub fn nueva(categoria: impl Into<String>, patron: impl Into<String>, accion: Permiso) -> Self {
        Self {
            categoria: categoria.into(),
            patron: patron.into(),
            accion,
        }
    }
}

/// ¿La regla aplica a esta llamada? Categoría EXACTA (no wildcard en la
/// categoría: las categorías son un vocabulario cerrado) y patrón wildcard
/// sobre el valor concreto.
#[must_use]
pub fn coincide_regla(regla: &ReglaPermiso, categoria: &str, valor: &str) -> bool {
    regla.categoria == categoria && coincide_patron(&regla.patron, valor)
}

/// Última regla que coincide (findLast de opencode). `None` → sin regla:
/// decide el default del modo / override de conversación (F3).
#[must_use]
pub fn evaluar_reglas<'a>(
    categoria: &str,
    valor: &str,
    reglas: &'a [ReglaPermiso],
) -> Option<&'a ReglaPermiso> {
    reglas
        .iter()
        .rev()
        .find(|r| coincide_regla(r, categoria, valor))
}

/// Todas las reglas que coinciden, en orden de inserción (para resolver con
/// el override de conversación, F1 §permiso.rs).
#[must_use]
pub fn reglas_coincidentes(
    categoria: &str,
    valor: &str,
    reglas: &[ReglaPermiso],
) -> Vec<ReglaPermiso> {
    reglas
        .iter()
        .filter(|r| coincide_regla(r, categoria, valor))
        .cloned()
        .collect()
}

/// Wildcard propio: `*` no cruza `/`; `**` cruza todo. Normaliza `\` → `/`
/// para que las rutas Windows se comporten como las POSIX.
#[must_use]
pub fn coincide_patron(patron: &str, valor: &str) -> bool {
    fn glob(patron: &[u8], valor: &[u8]) -> bool {
        match patron.split_first() {
            None => valor.is_empty(),
            Some((b'*', resto)) => {
                if let Some((b'*', resto2)) = resto.split_first() {
                    /* `**`: cruza separadores — probar consumir cualquier
                     * prefijo (0..=len) y seguir con el resto. */
                    (0..=valor.len()).any(|n| glob(resto2, &valor[n..]))
                } else {
                    /* `*`: NO cruza el separador. */
                    let tope = valor.iter().position(|&c| c == b'/').unwrap_or(valor.len());
                    (0..=tope).any(|n| glob(resto, &valor[n..]))
                }
            }
            Some((&c, resto)) => valor
                .split_first()
                .is_some_and(|(&d, resto_v)| c == d && glob(resto, resto_v)),
        }
    }
    glob(
        patron.replace('\\', "/").as_bytes(),
        valor.replace('\\', "/").as_bytes(),
    )
}

/* ── Clasificadores: de los argumentos de una llamada a (categoría, patrón).
 * `None` si la llamada no lleva el argumento relevante (la categoría estática
 * de la tool cubre ese caso con patrón `*`). */

fn es_ruta_fuera(ruta: &str) -> bool {
    let drive = ruta
        .as_bytes()
        .first()
        .map(|c| c.is_ascii_alphabetic())
        .unwrap_or(false);
    ruta.starts_with("../")
        || ruta.starts_with('/')
        || (drive && ruta.len() >= 2 && ruta.as_bytes()[1] == b':')
}

/// Clasifica una ruta de archivo: dentro del workspace → `lectura`/`escritura`
/// con la ruta como patrón; intento de salir (`../`, absoluta, drive Windows)
/// → `*_fuera_repo` (opencode `external_directory`). El sandbox igualmente
/// rechaza la fuga; la regla decide ANTES (ask/deny sin llegar al sandbox).
#[must_use]
pub fn clasificar_ruta_archivo(escritura: bool, ruta: &str) -> (String, String) {
    let ruta = ruta.replace('\\', "/");
    let categoria = match (escritura, es_ruta_fuera(&ruta)) {
        (true, true) => CAT_ESCRITURA_FUERA,
        (true, false) => CAT_ESCRITURA,
        (false, true) => CAT_LECTURA_FUERA,
        (false, false) => CAT_LECTURA,
    };
    (categoria.to_string(), ruta)
}

fn ruta_de(args: &Value) -> Option<String> {
    args.get("ruta").and_then(Value::as_str).map(str::to_string)
}

/// Clasificador de `file_read` (y tools de lectura con `ruta`).
#[must_use]
pub fn clasificar_lectura(args: &Value) -> Option<(String, String)> {
    ruta_de(args).map(|r| clasificar_ruta_archivo(false, &r))
}

/// Clasificador de `file_write`/`file_patch` (tools de escritura con `ruta`).
#[must_use]
pub fn clasificar_escritura(args: &Value) -> Option<(String, String)> {
    ruta_de(args).map(|r| clasificar_ruta_archivo(true, &r))
}

/// Clasificador de `web_search`/`web_fetch`: patrón = consulta o URL.
#[must_use]
pub fn clasificar_red(args: &Value) -> Option<(String, String)> {
    let valor = args
        .get("query")
        .or_else(|| args.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("*");
    Some((CAT_RED.to_string(), valor.to_string()))
}

/// Clasificador de la tool `task` (subagente): patrón = perfil delegado.
#[must_use]
pub fn clasificar_subagente(args: &Value) -> Option<(String, String)> {
    let perfil = args.get("agente").and_then(Value::as_str).unwrap_or("*");
    Some((CAT_SUBAGENTE.to_string(), perfil.to_string()))
}

/// [318A-16 F3] Clasificador de la tool `comando`: la clave derivada es
/// `(comando:<nivel>, **)` — el nivel de riesgo del comando clasificado por
/// el port fiel de claurst. Una regla `comando:critico` → deny cubre todos
/// los comandos críticos; `comando:seguro` → allow solo los seguros.
#[must_use]
pub fn clasificar_comando_llamada(args: &Value) -> Option<(String, String)> {
    let comando = args.get("comando").and_then(Value::as_str)?;
    let nivel = crate::bash_clasificar::clasificar_comando(comando);
    Some((format!("{CAT_COMANDO}:{}", nivel.clave()), "**".to_string()))
}

/// Clasificador de una llamada: de los argumentos a la clave (categoría
/// derivada, valor concreto). `None` si la llamada no lleva el argumento que
/// la categoría necesita (se cubre con la categoría estática y patrón `*`).
pub type Clasificador = fn(&Value) -> Option<(String, String)>;

/// Tabla estática de las tools del núcleo: `(tool_id, categoría estática,
/// clasificador desde argumentos)`. Las tools del consumidor (dominio) no
/// están aquí y caen a su id como clave con patrón `*` (comportamiento F3).
pub fn categorias_core() -> Vec<(&'static str, &'static str, Option<Clasificador>)> {
    vec![
        ("file_read", CAT_LECTURA, Some(clasificar_lectura)),
        ("file_search", CAT_LECTURA, None),
        /* [089A-6] `content_search` es lectura acotada al workspace (igual
         * que `file_search`): categoría lectura, sin clasificador. */
        ("content_search", CAT_LECTURA, None),
        /* [Bloque 3, F7] `repo_map` es solo lectura sobre el workspace local
         * ya acotado por el sandbox: categoría lectura, sin clasificador. */
        ("repo_map", CAT_LECTURA, None),
        ("file_write", CAT_ESCRITURA, Some(clasificar_escritura)),
        ("file_patch", CAT_ESCRITURA, Some(clasificar_escritura)),
        ("web_search", CAT_RED, Some(clasificar_red)),
        /* [Bloque 3, F3] `skill` lee un `.md` local ya descubierto (índice en
         * [REGLAS]): categoría lectura, sin clasificador (no hay ruta libre). */
        ("skill", CAT_LECTURA, None),
        ("task", CAT_SUBAGENTE, Some(clasificar_subagente)),
        ("todo", CAT_TODO, None),
        /* [318A-16 F3] Tools de comando: la tabla estática existe siempre
         * (las reglas F1 aplican aunque el runner no esté inyectado); la
         * tool en sí solo se registra con runner. */
        ("comando", CAT_COMANDO, Some(clasificar_comando_llamada)),
        ("comando_status", CAT_COMANDO, None),
        ("comando_matar", CAT_COMANDO, None),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /* Wildcard propio: `*` no cruza `/`; `**` cruza. */
    #[test]
    fn wildcard_estrella_no_cruza_separador() {
        assert!(coincide_patron("src/*.rs", "src/main.rs"));
        assert!(!coincide_patron("src/*.rs", "src/sub/main.rs"));
        assert!(coincide_patron("*.rs", "main.rs"));
        assert!(!coincide_patron("*.rs", "sub/main.rs"));
    }

    #[test]
    fn wildcard_doble_estrella_cruza_subarboles() {
        assert!(coincide_patron("src/**", "src/a.rs"));
        assert!(coincide_patron("src/**", "src/sub/deep/a.rs"));
        assert!(!coincide_patron("src/**", "otro/a.rs"));
        assert!(coincide_patron("**/*.rs", "a/b/c.rs"));
    }

    #[test]
    fn patrones_de_comando_por_palabras() {
        /* `git *` cubre cualquier subcomando git (regla "permitir siempre"
         * generalizada: no el comando exacto). */
        assert!(coincide_patron("git *", "git status"));
        assert!(coincide_patron("git *", "git push origin main"));
        assert!(!coincide_patron("git *", "npm run build"));
    }

    #[test]
    fn normaliza_backslashes_windows() {
        assert!(coincide_patron("src/**", r"src\sub\main.rs"));
        assert!(coincide_patron(r"src\*.rs", "src/main.rs"));
    }

    #[test]
    fn evaluar_ultima_coincidencia_gana() {
        let reglas = vec![
            ReglaPermiso::nueva("escritura", "src/**", Permiso::Allow),
            ReglaPermiso::nueva("escritura", "src/secreto/**", Permiso::Deny),
        ];
        /* Dentro de lo permitido → allow. */
        assert_eq!(
            evaluar_reglas("escritura", "src/lib.rs", &reglas).map(|r| r.accion),
            Some(Permiso::Allow)
        );
        /* La deny más reciente y más específica gana (plan: no la tapa la
         * regla allow genérica). */
        assert_eq!(
            evaluar_reglas("escritura", "src/secreto/claves.rs", &reglas).map(|r| r.accion),
            Some(Permiso::Deny)
        );
        /* Sin coincidencia → None (decide default/override). */
        assert_eq!(evaluar_reglas("red", "x", &reglas), None);
    }

    #[test]
    fn deny_con_patron_asterisco_cubre_toda_la_categoria() {
        let reglas = vec![ReglaPermiso::nueva("red", "*", Permiso::Deny)];
        assert_eq!(
            evaluar_reglas("red", "alguna-url", &reglas).map(|r| r.accion),
            Some(Permiso::Deny)
        );
        assert_eq!(evaluar_reglas("escritura", "a.txt", &reglas), None);
    }

    #[test]
    fn clasifica_rutas_dentro_y_fuera() {
        assert_eq!(
            clasificar_ruta_archivo(true, "src/main.rs"),
            ("escritura".to_string(), "src/main.rs".to_string())
        );
        assert_eq!(
            clasificar_ruta_archivo(false, "../secrets.env"),
            (
                "lectura_fuera_repo".to_string(),
                "../secrets.env".to_string()
            )
        );
        /* Windows: drive absoluto es fuera. */
        assert_eq!(
            clasificar_ruta_archivo(true, "C:\\Windows\\x.txt"),
            (
                "escritura_fuera_repo".to_string(),
                "C:/Windows/x.txt".to_string()
            )
        );
        assert_eq!(
            clasificar_ruta_archivo(false, "notas.md"),
            ("lectura".to_string(), "notas.md".to_string())
        );
    }
}
