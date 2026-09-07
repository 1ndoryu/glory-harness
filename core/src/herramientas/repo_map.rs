/* [Bloque 3, F7] Mapa del repo (opción B: índice de símbolos, sin LSP).
 * Un cliente LSP por lenguaje (opción A, estilo claurst `lsp_tool.rs`) exige
 * servidores externos configurados por lenguaje, procesos extra y JSON-RPC:
 * alto esfuerzo y nuevas dependencias. Este mapa es barato y agnóstico:
 * recorre el workspace (solo local, fail-closed sin sandbox), extrae
 * símbolos por extensión con analizadores de línea (rs/ts/js/py) y los
 * ordena por relevancia a la consulta (nombre > ruta > tipo). Determinista
 * (sin red, sin procesos) y acotado (archivos, profundidad, bytes).
 *
 * Solo depende de `SandboxArchivos` (raíz, `resolver` y `es_secreto`) y del
 * trait `AgentTool`: la tool `repo_map` es de solo lectura (`efecto: false`).
 */

use crate::error::{Error, Result};
use crate::sandbox::SandboxArchivos;
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Límites del mapa (un walk recursivo sin acotar puede bloquear el turno).
const MAPA_MAX_PROFUNDIDAD: usize = 8;
const MAPA_MAX_ARCHIVOS: usize = 2000;
const MAPA_MAX_BYTES_ARCHIVO: u64 = 200_000;
const MAPA_LIMITE_DEFAULT: usize = 80;
const MAPA_LIMITE_MAX: usize = 200;
const MAPA_MAX_BYTES_SALIDA: usize = 12_000;

/// Carpetas que nunca se indexan (dependencias, build, datos del vault).
const DIRS_EXCLUIDOS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".glory-harness",
    ".quality-tools",
    ".quality-tools-harness",
    ".quality-reports",
    ".sentinel",
];

/// Un símbolo extraído: dónde está y qué es.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Simbolo {
    pub ruta: String,
    pub linea: u32,
    pub tipo: String,
    pub nombre: String,
}

/// Símbolo con su puntuación de relevancia (mayor = más relevante).
#[derive(Debug, Clone)]
pub struct SimboloPuntuado {
    pub simbolo: Simbolo,
    pub puntos: u32,
}

/// Resultado del mapa: símbolos ya ordenados + metadatos para el resumen.
#[derive(Debug, Clone, Default)]
pub struct MapaRepo {
    pub simbolos: Vec<SimboloPuntuado>,
    pub archivos: usize,
    pub total_simbolos: usize,
}

/// Construye el mapa bajo `alcance` (relativa al workspace, "." = todo).
/// `consulta` ordena por relevancia (vacía = listado estable por peso).
/// `limite` acota símbolos devueltos (1..=MAPA_LIMITE_MAX, si no default).
/// Solo lee archivos de código (rs/ts/js/py), nunca secretos ni excluidos.
pub fn construir_mapa(
    sandbox: &SandboxArchivos,
    alcance: &str,
    consulta: &str,
    limite: Option<usize>,
) -> Result<MapaRepo> {
    let base = sandbox.resolver(alcance)?;
    let raiz = sandbox.raiz().to_path_buf();
    let limite = match limite {
        Some(l) if l >= 1 => l.min(MAPA_LIMITE_MAX),
        _ => MAPA_LIMITE_DEFAULT,
    };
    let mut estado = EstadoWalk {
        simbolos: Vec::new(),
        archivos: 0,
    };
    caminar(&raiz, &base, 0, sandbox, &mut estado);
    let tokens = tokens_consulta(consulta);
    let mut puntuados: Vec<SimboloPuntuado> = estado
        .simbolos
        .into_iter()
        .map(|simbolo| {
            let puntos = puntuar(&simbolo, &tokens);
            SimboloPuntuado { simbolo, puntos }
        })
        .collect();
    /* Determinista: a igual puntuación, ruta y luego línea (el orden de
     * lectura del disco varía entre ejecuciones y no debe verse). */
    puntuados.sort_by(|a, b| {
        b.puntos
            .cmp(&a.puntos)
            .then_with(|| a.simbolo.ruta.cmp(&b.simbolo.ruta))
            .then_with(|| a.simbolo.linea.cmp(&b.simbolo.linea))
    });
    let total_simbolos = puntuados.len();
    puntuados.truncate(limite);
    Ok(MapaRepo {
        simbolos: puntuados,
        archivos: estado.archivos,
        total_simbolos,
    })
}

struct EstadoWalk {
    simbolos: Vec<Simbolo>,
    archivos: usize,
}

fn caminar(
    raiz: &std::path::Path,
    dir: &std::path::Path,
    profundidad: usize,
    sandbox: &SandboxArchivos,
    estado: &mut EstadoWalk,
) {
    if profundidad > MAPA_MAX_PROFUNDIDAD || estado.archivos >= MAPA_MAX_ARCHIVOS {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    /* Orden estable: el orden del filesystem no es determinista. */
    let mut rutas: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    rutas.sort();
    for ruta in rutas {
        if estado.archivos >= MAPA_MAX_ARCHIVOS {
            return;
        }
        let nombre = ruta
            .file_name()
            .map(|n| n.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ruta.is_dir() {
            if DIRS_EXCLUIDOS.contains(&nombre.as_str()) {
                continue;
            }
            caminar(raiz, &ruta, profundidad + 1, sandbox, estado);
        } else if extension_codigo(&nombre).is_some() {
            let rel = ruta
                .strip_prefix(raiz)
                .unwrap_or(&ruta)
                .to_string_lossy()
                .replace('\\', "/");
            /* Nunca indexar secretos (aunque la extensión coincida). */
            if sandbox.es_secreto(&rel) {
                continue;
            }
            if let Ok(metadata) = std::fs::metadata(&ruta) {
                if metadata.len() > MAPA_MAX_BYTES_ARCHIVO {
                    continue;
                }
            }
            let Ok(contenido) = std::fs::read_to_string(&ruta) else {
                continue;
            };
            estado.archivos += 1;
            extraer_simbolos(&rel, &nombre, &contenido, &mut estado.simbolos);
        }
    }
}

fn extension_codigo(nombre_minusculas: &str) -> Option<&'static str> {
    if nombre_minusculas.ends_with(".rs") {
        Some("rs")
    } else if nombre_minusculas.ends_with(".ts")
        || nombre_minusculas.ends_with(".tsx")
        || nombre_minusculas.ends_with(".mts")
        || nombre_minusculas.ends_with(".cts")
    {
        Some("ts")
    } else if nombre_minusculas.ends_with(".js") || nombre_minusculas.ends_with(".jsx") {
        Some("js")
    } else if nombre_minusculas.ends_with(".py") {
        Some("py")
    } else {
        None
    }
}

fn extraer_simbolos(ruta: &str, nombre: &str, contenido: &str, salida: &mut Vec<Simbolo>) {
    let lang = match extension_codigo(nombre) {
        Some(l) => l,
        None => return,
    };
    for (i, linea) in contenido.lines().enumerate() {
        let extraido = match lang {
            "rs" => simbolo_rust(linea),
            "ts" | "js" => simbolo_ts(linea),
            "py" => simbolo_py(linea),
            _ => None,
        };
        if let Some((tipo, nombre)) = extraido {
            salida.push(Simbolo {
                ruta: ruta.to_string(),
                linea: (i + 1) as u32,
                tipo: tipo.to_string(),
                nombre,
            });
        }
    }
}

/// Nombre hasta el primer delimitador (`main<T>(` → `main`, `FOO: u32` → `FOO`).
/// La llave se expresa como `char::from(123)` (no literal `{` en el fuente):
/// los contadores de llaves textuales del gate la desbalancean y alargan la
/// función ficticiamente (mismo caso que `export {` en `simbolo_ts`).
fn hasta_delimitador(s: &str) -> &str {
    let fin = s
        .find(|c: char| "<(:;= \t".contains(c) || c == char::from(123))
        .unwrap_or(s.len());
    s[..fin].trim()
}

fn simbolo_rust(linea: &str) -> Option<(&'static str, String)> {
    let mut s = linea.trim_start();
    if s.is_empty() || s.starts_with("//") || s.starts_with("#[") || s.starts_with("#!") {
        return None;
    }
    for pref in [
        "pub(crate) ",
        "pub(super) ",
        "pub(self) ",
        "pub ",
        "async ",
        "unsafe ",
        "extern ",
        "default ",
    ] {
        if let Some(resto) = s.strip_prefix(pref) {
            s = resto;
        }
    }
    /* `pub(` sin espacio (p. ej. `pub(in crate::x) fn`): quitar hasta `)`. */
    if let Some(resto) = s.strip_prefix("pub(") {
        if let Some(fin) = resto.find(')') {
            s = resto[fin + 1..].trim_start();
        }
    }
    let (tipo, resto) = if let Some(r) = s.strip_prefix("fn ") {
        ("fn", r)
    } else if let Some(r) = s.strip_prefix("struct ") {
        ("struct", r)
    } else if let Some(r) = s.strip_prefix("enum ") {
        ("enum", r)
    } else if let Some(r) = s.strip_prefix("trait ") {
        ("trait", r)
    } else if let Some(r) = s.strip_prefix("mod ") {
        ("mod", r)
    } else if let Some(r) = s.strip_prefix("type ") {
        ("type", r)
    } else if let Some(r) = s.strip_prefix("const ") {
        ("const", r)
    } else if let Some(r) = s.strip_prefix("static ") {
        ("static", r)
    } else if let Some(r) = s.strip_prefix("macro_rules!") {
        ("macro", r.trim_start_matches('!').trim_start())
    } else if let Some(cuerpo) = s.strip_prefix("impl ").or_else(|| s.strip_prefix("impl<")) {
        let cuerpo = cuerpo.trim_start();
        /* `impl Display for Foo` → Foo; `impl Foo` → Foo (sin genéricos). */
        let objetivo = match cuerpo.split_once(" for ") {
            Some((_, nombre)) => nombre,
            None => cuerpo,
        };
        ("impl", objetivo)
    } else {
        return None;
    };
    let nombre = hasta_delimitador(resto);
    if nombre.is_empty() {
        return None;
    }
    Some((tipo, nombre.to_string()))
}

fn simbolo_ts(linea: &str) -> Option<(&'static str, String)> {
    let mut s = linea.trim_start();
    /* `export {…}` / `export *` (re-exports sin símbolo propio) se detectan
     * sin llaves literales en el fuente: los contadores de llaves ingenuos
     * (como el del gate) las desbalancean y alargan la función ficticiamente. */
    let es_reexport = s.starts_with("export") && !s.starts_with("export ");
    if s.is_empty()
        || s.starts_with("//")
        || s.starts_with("*")
        || s.starts_with("import ")
        || s.starts_with("from ")
        || es_reexport
        || s.starts_with('@')
    {
        return None;
    }
    for pref in ["export default ", "export ", "declare ", "async "] {
        if let Some(resto) = s.strip_prefix(pref) {
            s = resto;
        }
    }
    let (tipo, resto) = if let Some(r) = s.strip_prefix("function ") {
        ("function", r)
    } else if let Some(r) = s.strip_prefix("class ") {
        ("class", r)
    } else if let Some(r) = s.strip_prefix("interface ") {
        ("interface", r)
    } else if let Some(r) = s.strip_prefix("enum ") {
        ("enum", r)
    } else if let Some(r) = s.strip_prefix("type ") {
        ("type", r)
    } else if let Some(r) = s.strip_prefix("const ") {
        ("const", r)
    } else {
        return None;
    };
    let nombre = hasta_delimitador(resto);
    if nombre.is_empty() {
        return None;
    }
    Some((tipo, nombre.to_string()))
}

fn simbolo_py(linea: &str) -> Option<(&'static str, String)> {
    let mut s = linea.trim_start();
    if s.is_empty()
        || s.starts_with('#')
        || s.starts_with("import ")
        || s.starts_with("from ")
        || s.starts_with('@')
    {
        return None;
    }
    if let Some(resto) = s.strip_prefix("async ") {
        s = resto;
    }
    let (tipo, resto) = if let Some(r) = s.strip_prefix("def ") {
        ("def", r)
    } else if let Some(r) = s.strip_prefix("class ") {
        ("class", r)
    } else {
        return None;
    };
    let nombre = hasta_delimitador(resto);
    if nombre.is_empty() {
        return None;
    }
    Some((tipo, nombre.to_string()))
}

fn tokens_consulta(consulta: &str) -> Vec<String> {
    consulta
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 2)
        .map(str::to_string)
        .collect()
}

fn peso_tipo(tipo: &str) -> u32 {
    match tipo {
        "fn" | "function" | "def" | "struct" | "class" | "interface" | "trait" | "enum" => 2,
        _ => 1,
    }
}

/// Relevancia: el nombre manda sobre la ruta; el tipo desempata.
fn puntuar(simbolo: &Simbolo, tokens: &[String]) -> u32 {
    if tokens.is_empty() {
        return peso_tipo(&simbolo.tipo);
    }
    let nombre = simbolo.nombre.to_lowercase();
    let ruta = simbolo.ruta.to_lowercase();
    let aciertos_nombre = tokens
        .iter()
        .filter(|t| nombre.contains(t.as_str()))
        .count() as u32;
    let aciertos_ruta = tokens.iter().filter(|t| ruta.contains(t.as_str())).count() as u32;
    10 * aciertos_nombre + 5 * aciertos_ruta + peso_tipo(&simbolo.tipo)
}

/// Render del mapa: una línea `ruta:linea:tipo nombre` por símbolo, con
/// cabecera de conteos y aviso de truncado (corte por límite o por bytes).
pub fn render_mapa(mapa: &MapaRepo) -> String {
    let mut lineas = vec![format!(
        "mapa del repo ({} símbolos de {} archivos):",
        mapa.total_simbolos, mapa.archivos
    )];
    for p in &mapa.simbolos {
        lineas.push(format!(
            "{}:{}:{} {}",
            p.simbolo.ruta, p.simbolo.linea, p.simbolo.tipo, p.simbolo.nombre
        ));
    }
    if mapa.simbolos.len() < mapa.total_simbolos {
        lineas.push(format!(
            "[truncado: {} símbolos más; acota con consulta o ruta]",
            mapa.total_simbolos - mapa.simbolos.len()
        ));
    }
    let texto = lineas.join("\n");
    truncar_por_bytes(&texto, MAPA_MAX_BYTES_SALIDA)
}

/// Corta por bytes sin partir un carácter UTF-8, con aviso.
fn truncar_por_bytes(texto: &str, max_bytes: usize) -> String {
    if texto.len() <= max_bytes {
        return texto.to_string();
    }
    let mut corte = max_bytes;
    while corte > 0 && !texto.is_char_boundary(corte) {
        corte -= 1;
    }
    format!(
        "{}\n[truncado por tamaño; acota con consulta o ruta]",
        &texto[..corte]
    )
}

/// Tool `repo_map`: índice de símbolos del workspace para orientarse antes de
/// leer/editar. Solo lectura (sin efecto → sin aprobación en ningún modo).
pub struct ToolRepoMap;

#[async_trait]
impl AgentTool for ToolRepoMap {
    fn id(&self) -> &'static str {
        "repo_map"
    }
    fn descripcion(&self) -> &'static str {
        "Mapa de símbolos del workspace local (funciones, structs, clases, módulos) ordenado por relevancia a la consulta.\nFORMATO DE SALIDA: cabecera con conteos + una línea `ruta:linea:tipo nombre` por símbolo (máx `limite`); aviso `[truncado]` si hay más.\nLÍMITES: solo código rs/ts/js/py (máx 200 KB por archivo); ignora dependencias, build, .git y secretos; profundidad 8, máx 2000 archivos; `limite` 1-200 (defecto 80); salida máx ~12 KB.\nCUÁNDO USARLA: orientarse en código desconocido antes de file_read/file_search; localizar dónde vive un símbolo; con `ruta` acota a un subdirectorio.\nERRORES: alcance fuera del workspace, sin símbolos ('mapa vacío'), solo disponible en modo local."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "consulta": {"type": "string", "description": "Texto para ordenar por relevancia (ej. 'vault restaurar'). Opcional: vacía = listado estable."},
                "ruta": {"type": "string", "description": "Subdirectorio del workspace donde buscar (ej. 'src'). Opcional (defecto: todo)."},
                "limite": {"type": "integer", "minimum": 1, "maximum": 200, "description": "Máximo de símbolos (defecto 80)."}
            },
            "required": []
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let sandbox = ctx
            .sandbox_archivos
            .as_ref()
            .map(|s| s.as_ref())
            .ok_or_else(|| {
                Error::Validacion(
                    "repo_map solo está disponible en modo local (AGENTE_MODO=local)".into(),
                )
            })?;
        let consulta = argumentos
            .get("consulta")
            .and_then(Value::as_str)
            .unwrap_or("");
        let alcance = argumentos
            .get("ruta")
            .and_then(Value::as_str)
            .unwrap_or(".");
        let limite = argumentos
            .get("limite")
            .and_then(Value::as_u64)
            .map(|l| l as usize);
        let mapa = construir_mapa(sandbox, alcance, consulta, limite)?;
        if mapa.total_simbolos == 0 {
            return Ok(AgentToolResult::ok(
                "mapa vacío: sin símbolos de código en el alcance.".to_string(),
                format!("mapa vacío en '{alcance}'"),
            ));
        }
        let contenido = render_mapa(&mapa);
        Ok(AgentToolResult::ok(
            contenido,
            format!(
                "mapa {} símbolos ({} archivos){}",
                mapa.total_simbolos,
                mapa.archivos,
                if consulta.is_empty() {
                    String::new()
                } else {
                    format!(" para '{consulta}'")
                }
            ),
        ))
    }
}

/// Registra `repo_map` junto a las tools de archivo (mismo fail-closed: solo
/// con sandbox local; necesita la raíz del workspace para caminar).
pub fn registrar_tool_repo_map(registry: &mut crate::tool::AgentToolRegistry) {
    registry.registrar(Box::new(ToolRepoMap));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn dir_aislada(nombre: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-map-{}-{}-{nombre}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    fn sandbox_con_fixture() -> (Arc<SandboxArchivos>, std::path::PathBuf) {
        let dir = dir_aislada("f7");
        std::fs::write(dir.join("main.rs"), "fn main() {\n}\nstruct App;\n").expect("seed");
        std::fs::write(
            dir.join("app.ts"),
            "export class Tienda {\n}\nexport function comprar() {}\n",
        )
        .expect("seed");
        std::fs::write(dir.join("notas.md"), "# título\n").expect("seed");
        std::fs::create_dir_all(dir.join("node_modules")).expect("seed");
        std::fs::write(dir.join("node_modules").join("lib.js"), "function x() {}").expect("seed");
        let sandbox = Arc::new(SandboxArchivos::nuevo(&dir).expect("sandbox"));
        (sandbox, dir)
    }

    #[test]
    fn extrae_simbolos_rust_ts_y_omite_no_codigo() {
        let (sandbox, dir) = sandbox_con_fixture();
        let mapa = construir_mapa(&sandbox, ".", "", None).expect("mapa");
        let lineas: Vec<String> = mapa
            .simbolos
            .iter()
            .map(|p| format!("{}:{} {}", p.simbolo.ruta, p.simbolo.tipo, p.simbolo.nombre))
            .collect();
        assert!(
            lineas.contains(&"main.rs:fn main".to_string()),
            "{lineas:?}"
        );
        assert!(
            lineas.contains(&"main.rs:struct App".to_string()),
            "{lineas:?}"
        );
        assert!(
            lineas.contains(&"app.ts:class Tienda".to_string()),
            "{lineas:?}"
        );
        assert!(
            lineas.contains(&"app.ts:function comprar".to_string()),
            "{lineas:?}"
        );
        assert!(
            !lineas.iter().any(|l| l.contains("notas.md")),
            "md no se indexa"
        );
        assert!(
            !lineas.iter().any(|l| l.contains("node_modules")),
            "deps no se indexan"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn la_consulta_ordena_nombre_sobre_solo_ruta() {
        let (sandbox, dir) = sandbox_con_fixture();
        /* `otra` solo coincide por ruta (tiendax/); `Tienda` por nombre. */
        std::fs::create_dir_all(dir.join("tiendax")).expect("seed");
        std::fs::write(
            dir.join("tiendax").join("otro.py"),
            "def otra():\n    pass\n",
        )
        .expect("seed");
        let mapa = construir_mapa(&sandbox, ".", "tienda", None).expect("mapa");
        let nombres: Vec<&str> = mapa
            .simbolos
            .iter()
            .map(|p| p.simbolo.nombre.as_str())
            .collect();
        let pos_tienda = nombres
            .iter()
            .position(|n| *n == "Tienda")
            .expect("Tienda indexada");
        let pos_otra = nombres
            .iter()
            .position(|n| *n == "otra")
            .expect("otra indexada");
        assert!(
            pos_tienda < pos_otra,
            "el nombre gana a la sola ruta: {nombres:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn el_limite_acota_con_aviso_en_render() {
        let (sandbox, dir) = sandbox_con_fixture();
        let mapa = construir_mapa(&sandbox, ".", "", Some(2)).expect("mapa");
        assert_eq!(mapa.simbolos.len(), 2);
        assert!(mapa.total_simbolos > 2);
        let texto = render_mapa(&mapa);
        assert!(texto.contains("[truncado:"), "avisa del corte por límite");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn el_alcance_fuera_del_workspace_falla() {
        let (sandbox, dir) = sandbox_con_fixture();
        let err = construir_mapa(&sandbox, "../fuera", "", None).expect_err("escape");
        assert!(!err.to_string().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ignora_vault_secretos_y_archivos_grandes() {
        let dir = dir_aislada("f7-secretos");
        std::fs::write(dir.join(".env"), "CLAVE=secreta\n").expect("seed");
        std::fs::write(dir.join("grande.rs"), "fn g() {}\n").expect("seed");
        std::fs::write(dir.join("pesado.rs"), "fn pesado() {}\n".repeat(20_000)).expect("seed");
        let vault = dir.join(".glory-harness").join("backups").join("x");
        std::fs::create_dir_all(&vault).expect("seed");
        std::fs::write(vault.join("oculto.rs"), "fn oculto() {}\n").expect("seed");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        assert!(sandbox.es_secreto(".env"), "precondición: .env es secreto");
        assert!(
            sandbox.es_secreto(".glory-harness/backups/x/oculto.rs"),
            "precondición: el vault es zona bloqueada"
        );
        let mapa = construir_mapa(&sandbox, ".", "", None).expect("mapa");
        assert!(
            !mapa.simbolos.iter().any(|p| p.simbolo.ruta == ".env"),
            "secretos fuera del mapa"
        );
        assert!(
            !mapa.simbolos.iter().any(|p| p.simbolo.nombre == "oculto"),
            "el vault nunca se indexa"
        );
        assert!(mapa.simbolos.iter().any(|p| p.simbolo.nombre == "g"));
        assert!(
            !mapa.simbolos.iter().any(|p| p.simbolo.nombre == "pesado"),
            "archivos >200 KB se saltan"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn impl_rust_apunta_al_tipo_y_mod_se_indexa() {
        assert_eq!(
            simbolo_rust("impl VaultArchivos {"),
            Some(("impl", "VaultArchivos".to_string()))
        );
        assert_eq!(
            simbolo_rust("impl Display for Salida {"),
            Some(("impl", "Salida".to_string()))
        );
        assert_eq!(
            simbolo_rust("pub(crate) fn leer(&self) {"),
            Some(("fn", "leer".to_string()))
        );
        assert_eq!(simbolo_rust("use std::fs;"), None, "imports fuera");
        assert_eq!(simbolo_rust("    // comentario"), None, "comentarios fuera");
        assert_eq!(
            simbolo_ts("export default function principal() {"),
            Some(("function", "principal".to_string()))
        );
        assert_eq!(
            simbolo_py("async def turno():"),
            Some(("def", "turno".to_string()))
        );
    }

    #[tokio::test]
    async fn tool_ok_con_resumen_y_fail_closed_sin_sandbox() {
        let (sandbox, dir) = sandbox_con_fixture();
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = AgentToolContext {
            user_id: uuid::Uuid::new_v4(),
            persistencia: &persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: Some(sandbox),
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        };
        let resultado = ToolRepoMap
            .ejecutar(&ctx, json!({"consulta": "tienda"}))
            .await
            .expect("tool responde");
        assert!(resultado.contenido.contains("mapa del repo"));
        assert!(resultado.contenido.contains("Tienda"));
        assert!(resultado.resumen.contains("símbolos"));
        /* Sin sandbox: error claro, nunca falso éxito. */
        let ctx_sin = AgentToolContext {
            sandbox_archivos: None,
            ..ctx
        };
        let err = ToolRepoMap
            .ejecutar(&ctx_sin, json!({}))
            .await
            .expect_err("sin sandbox debe fallar");
        assert!(err.to_string().contains("modo local"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
