/* [089A-6] Búsqueda por CONTENIDO indexada (content_search).
 *
 * Motor: `tgrep-core` v1.0.5 compilado DENTRO del binario (dependencia git
 * pineada por tag en `core/Cargo.toml` + sha exacto en `Cargo.lock`).
 * Cero dependencia del sistema: sin binario externo, sin PATH, sin variable
 * de entorno. Si el índice no se puede construir ni abrir, la tool responde
 * con un recorrido local acotado (nunca falla por falta de motor).
 *
 * Índice persistente por workspace:
 * - Directorio: `<temp>/glory-harness/tgrep-idx/<hash16>/`, donde el hash
 *   identifica la raíz canónica del workspace. Muchos workspaces → muchos
 *   índices; cada uno se reconstruye solo cuando cambia su workspace.
 * - Frescura: `glory-manifest.json` guarda la foto (ruta, mtime, tamaño) del
 *   mismo paseo que usa el builder (`walk_file_metadata` con opciones por
 *   defecto, idénticas a las de `build_index`). Si la foto actual difiere,
 *   se reconstruye antes de responder: el agente SIEMPRE ve el contenido
 *   vigente, incluidas escrituras de su propia sesión.
 * - Concurrencia: un `Mutex` por directorio de índice serializa
 *   comprobar-manifiesto → reconstruir → consultar dentro del proceso.
 *
 * Verificación: el índice devuelve ficheros candidatos (trigramas); cada
 * línea se confirma con `regex` (misma sintaxis que el plan tgrep).
 * Smart-case como ripgrep: sin mayúsculas en el patrón → insensible.
 *
 * Secretos: la lectura de verificación pasa por `sandbox.leer`, la misma
 * lista negra que `file_read` (`.env`, `*.pem`, …). Un secreto indexado no
 * aflora jamás en la salida. El índice en temp contiene trigramas del
 * workspace: mismo dominio de confianza que el propio workspace local.
 *
 * Cotas: 50 resultados, salida ~12 KB, ficheros ≤ 200 KB, 200 ficheros
 * verificados por consulta, build+consulta ≤ 180 s en hilo dedicado.
 */

use super::tools_archivo::{glob_simple, obtener_sandbox};
use crate::error::{Error, Result};
use crate::sandbox::SandboxArchivos;
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

/// Límites de la tool (alineados con `file_search`: 50 resultados, ~12 KB).
const CONTENT_MAX_RESULTADOS: usize = 50;
const CONTENT_MAX_SALIDA_BYTES: usize = 12 * 1024;
const CONTENT_MAX_BYTES_FICHERO: u64 = 200 * 1024;
const CONTENT_MAX_LINEA_CHARS: usize = 240;
/// Ficheros verificados por consulta (el índice puede devolver miles).
const CONTENT_MAX_VERIFICAR_FICHEROS: usize = 200;
/// Cota total paseo + (posible) build + consulta, en hilo dedicado.
const CONTENT_TIMEOUT_SEGS: u64 = 180;
/// Recorrido local de emergencia: profundidad y nº de ficheros.
const LOCAL_MAX_PROFUNDIDAD: usize = 8;
const LOCAL_MAX_FICHEROS: usize = 500;
/// Manifiesto de frescura dentro del directorio del índice.
const MANIFIESTO_NOMBRE: &str = "glory-manifest.json";

/// Una coincidencia `ruta:línea:contenido` (ruta relativa al workspace, `/`).
struct Coincidencia {
    ruta: String,
    linea: u64,
    texto: String,
}

/// Foto de frescura: la misma admisión que el builder, porque sale del mismo
/// paseo (`walk_file_metadata` con opciones por defecto).
#[derive(Serialize, Deserialize)]
struct Manifiesto {
    raiz: String,
    ficheros: BTreeMap<String, (u64, u64)>,
}

pub struct ToolContentSearch;

#[async_trait]
impl AgentTool for ToolContentSearch {
    fn id(&self) -> &'static str {
        "content_search"
    }
    fn descripcion(&self) -> &'static str {
        "Busca texto dentro de los archivos del workspace y devuelve coincidencias con número de línea.\nFORMATO DE SALIDA: una coincidencia por línea `ruta:linea:contenido` (ruta relativa al workspace); con `solo_archivos` solo la lista de rutas; el resumen declara el motor usado.\nMOTOR: índice de trigramas (tgrep) compilado dentro del programa, persistente por workspace y reconstruido solo cuando el workspace cambia: sin instalación ni configuración; si el índice no está disponible, recorrido local acotado de emergencia.\nSINTAXIS DEL PATRÓN: expresión regular (p. ej. `TODO|FIXME`, `fn nombre`); smart-case: sin mayúsculas → insensible a mayúsculas.\nLÍMITES: máx 50 coincidencias (`limite` 1-50, defecto 20); salida ~12 KB (se trunca con aviso); ficheros de más de 200 KB y binarios se omiten; misma lista negra de secretos que file_read.\nCUÁNDO USARLA: para localizar dónde aparece un símbolo, un texto o un marcador en el código; para localizar archivos por nombre usa file_search.\nERRORES: patrón vacío o sintaxis inválida; ruta fuera del workspace."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patron": {"type": "string", "description": "Texto o expresión regular a buscar (p. ej. TODO|FIXME). Obligatorio."},
                "glob": {"type": "string", "description": "Filtro simple de nombre: *.rs (extensión) o prefijo* . Opcional."},
                "limite": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Máximo de coincidencias (defecto 20)."},
                "solo_archivos": {"type": "boolean", "description": "true → solo rutas de fichero con coincidencia (defecto false)."}
            },
            "required": ["patron"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let sandbox = obtener_sandbox(ctx)?;
        let patron = argumentos
            .get("patron")
            .and_then(Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if patron.is_empty() {
            return Err(Error::Argumentos(
                "content_search: 'patron' es obligatorio y no puede estar vacío".into(),
            ));
        }
        let glob = argumentos
            .get("glob")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(ToString::to_string);
        let limite = argumentos
            .get("limite")
            .and_then(Value::as_u64)
            .map(|n| n.clamp(1, CONTENT_MAX_RESULTADOS as u64) as usize)
            .unwrap_or(20);
        let solo_archivos = argumentos
            .get("solo_archivos")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        /* Smart-case: sin mayúsculas en el patrón → insensible. */
        let ci = !patron.bytes().any(|b| b.is_ascii_uppercase());
        /* La sintaxis se valida ANTES de tocar disco: un patrón inválido es
         * error de argumentos, no un índice ausente (fail-closed claro). */
        if let Err(detalle) = tgrep_core::query::build_query_plan(&patron, ci) {
            return Err(Error::Argumentos(format!(
                "content_search: patrón inválido: {detalle}"
            )));
        }
        let expresion = regex::RegexBuilder::new(&patron)
            .case_insensitive(ci)
            .build()
            .map_err(|err| Error::Argumentos(format!("content_search: patrón inválido: {err}")))?;
        let base = base_canonica(sandbox);
        let dir = dir_indice_para(&base);
        let entrada = EntradaIndice {
            base: base.clone(),
            dir,
            patron: patron.clone(),
            sin_mayusculas: ci,
        };
        /* Paseo + (posible) build + consulta en hilo dedicado (ver
         * `bloquear_indice`): el build de un workspace grande tarda decenas
         * de segundos y no debe atascar el runtime async. */
        let control = bloquear_indice(entrada).await?;
        let salida = match control {
            ControlIndice::Lista(salida) => salida,
            ControlIndice::SinIndice(causa) => {
                let halladas = recorrido_local(sandbox, &base, &expresion, glob.as_deref());
                let motor = format!("recorrido local; índice no disponible: {causa}");
                return Ok(responder(
                    &patron,
                    halladas,
                    false,
                    &motor,
                    limite,
                    solo_archivos,
                ));
            }
        };
        let extra = if salida.reconstruido {
            ", índice reconstruido"
        } else {
            ""
        };
        let (halladas, mas_posibles) = verificar(
            sandbox,
            &base,
            &salida.candidatas,
            &expresion,
            glob.as_deref(),
            limite,
        );
        let motor = format!("tgrep integrado{extra}");
        Ok(responder(
            &patron,
            halladas,
            mas_posibles,
            &motor,
            limite,
            solo_archivos,
        ))
    }
}

/// Respuesta común a ambos motores: recorta a `limite`, formatea y resume.
fn responder(
    patron: &str,
    halladas: Vec<Coincidencia>,
    mas_posibles: bool,
    motor: &str,
    limite: usize,
    solo_archivos: bool,
) -> AgentToolResult {
    let mut halladas = halladas;
    halladas.truncate(limite);
    let contenido = if halladas.is_empty() {
        format!("Sin resultados para '{patron}' [motor: {motor}].")
    } else if solo_archivos {
        let mut vistas = HashSet::new();
        let mut rutas: Vec<&str> = Vec::new();
        for c in &halladas {
            if vistas.insert(c.ruta.as_str()) {
                rutas.push(&c.ruta);
            }
        }
        formatear_rutas(&rutas)
    } else {
        formatear_coincidencias(&halladas, limite, mas_posibles)
    };
    AgentToolResult::ok(
        contenido,
        format!(
            "{} coincidencias para '{patron}' [motor: {motor}]",
            halladas.len()
        ),
    )
}

/// Raíz canónica del workspace (el sandbox ya canoniza en `nuevo`; esto solo
/// normaliza si el sandbox viniera de otra vía).
fn base_canonica(sandbox: &SandboxArchivos) -> PathBuf {
    std::fs::canonicalize(sandbox.raiz()).unwrap_or_else(|_| sandbox.raiz().to_path_buf())
}

/// Directorio del índice para una raíz: estable entre procesos, aislado por
/// workspace (hash de la ruta canónica).
fn dir_indice_para(base: &Path) -> PathBuf {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut suma = DefaultHasher::new();
    base.to_string_lossy().hash(&mut suma);
    std::env::temp_dir()
        .join("glory-harness")
        .join("tgrep-idx")
        .join(format!("{:016x}", suma.finish()))
}

/// Cerrojo por directorio de índice (serializa foto → build → consulta).
fn cerrojo_para(clave: &str) -> Arc<Mutex<()>> {
    static CERROJOS: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let mapa = CERROJOS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guardia = mapa.lock().unwrap_or_else(|v| v.into_inner());
    guardia
        .entry(clave.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

struct EntradaIndice {
    base: PathBuf,
    dir: PathBuf,
    patron: String,
    sin_mayusculas: bool,
}

struct SalidaIndice {
    candidatas: Vec<String>,
    reconstruido: bool,
}

/// Desenlace del hilo de índice: lista de candidatas o motivo de fallback.
/// El motivo viaja como dato (no como error) porque el recorrido local de
/// emergencia siempre puede responder algo útil.
enum ControlIndice {
    Lista(SalidaIndice),
    SinIndice(String),
}

/// Paseo + (posible) build + consulta en hilo dedicado: el build de un
/// workspace grande tarda decenas de segundos y no debe atascar el runtime
/// async. Todo bajo el cerrojo del índice (`asegurar_y_consultar`).
async fn bloquear_indice(entrada: EntradaIndice) -> Result<ControlIndice> {
    let tarea = tokio::task::spawn_blocking(move || asegurar_y_consultar(&entrada));
    match tokio::time::timeout(Duration::from_secs(CONTENT_TIMEOUT_SEGS), tarea).await {
        Err(_) => Ok(ControlIndice::SinIndice(format!(
            "tiempo agotado ({CONTENT_TIMEOUT_SEGS}s)"
        ))),
        Ok(Err(join)) => Err(Error::Interno(format!(
            "content_search: tarea de índice interrumpida: {join}"
        ))),
        Ok(Ok(Err(causa))) => Ok(ControlIndice::SinIndice(causa)),
        Ok(Ok(Ok(salida))) => Ok(ControlIndice::Lista(salida)),
    }
}

/// Asegura un índice vigente y devuelve las rutas candidatas del patrón.
/// Bloqueante por diseño: corre en `spawn_blocking` bajo el cerrojo del índice.
fn asegurar_y_consultar(entrada: &EntradaIndice) -> std::result::Result<SalidaIndice, String> {
    let clave = entrada.dir.to_string_lossy().into_owned();
    let cerrojo = cerrojo_para(&clave);
    let _bloqueo = cerrojo.lock().unwrap_or_else(|v| v.into_inner());
    let foto = fotografiar(&entrada.base);
    let ruta_manifiesto = entrada.dir.join(MANIFIESTO_NOMBRE);
    let vigente = std::fs::read(&ruta_manifiesto)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Manifiesto>(&bytes).ok())
        .is_some_and(|m| m.raiz == entrada.base.to_string_lossy() && m.ficheros == foto);
    let mut reconstruido = false;
    if !vigente {
        let _ = std::fs::remove_dir_all(&entrada.dir);
        tgrep_core::builder::build_index(&entrada.base, Some(&entrada.dir), false, false, &[])
            .map_err(|err| format!("construir índice: {err}"))?;
        let manifiesto = Manifiesto {
            raiz: entrada.base.to_string_lossy().into_owned(),
            ficheros: foto,
        };
        let bytes = serde_json::to_vec(&manifiesto).map_err(|err| format!("manifiesto: {err}"))?;
        std::fs::write(&ruta_manifiesto, bytes)
            .map_err(|err| format!("guardar manifiesto: {err}"))?;
        reconstruido = true;
    }
    let lector = tgrep_core::reader::IndexReader::open(&entrada.dir)
        .map_err(|err| format!("abrir índice: {err}"))?;
    let plan = tgrep_core::query::build_query_plan(&entrada.patron, entrada.sin_mayusculas)
        .map_err(|err| format!("patrón inválido: {err}"))?;
    let ids = if plan.is_match_all() {
        lector.all_file_ids()
    } else {
        tgrep_core::query::execute_plan_with_masks(&plan, &|tri| {
            lector.lookup_trigram_with_masks(tri)
        })
    };
    let mut candidatas = Vec::with_capacity(ids.len());
    for id in ids {
        if let Some(rel) = lector.file_path(id) {
            candidatas.push(rel.to_string());
        }
    }
    Ok(SalidaIndice {
        candidatas,
        reconstruido,
    })
}

/// Foto (ruta, mtime, tamaño) con el MISMO paseo y opciones que el builder:
/// si la foto coincide con el manifiesto, el índice está vigente.
fn fotografiar(base: &Path) -> BTreeMap<String, (u64, u64)> {
    let paseo = tgrep_core::walker::walk_file_metadata(
        base,
        &tgrep_core::walker::MetaWalkOptions::default(),
    );
    paseo
        .files
        .into_iter()
        .map(|f| (f.relative_path, (f.mtime, f.size)))
        .collect()
}

/// Normaliza una ruta del índice a relativa del workspace con `/`, o `None`
/// si intenta escapar (absoluta, `..`). Defensa en profundidad: el índice lo
/// construye el propio paseo, pero la verificación no confía en ello.
fn rel_segura(rel: &str) -> Option<String> {
    let normal = rel.replace('\\', "/");
    let sin_actual: &str = normal.strip_prefix("./").unwrap_or(&normal);
    if sin_actual.is_empty()
        || sin_actual.starts_with('/')
        || sin_actual.split('/').any(|p| p == "..")
    {
        return None;
    }
    Some(sin_actual.to_string())
}

/// Confirma cada candidata línea a línea con la regex. La lectura pasa por
/// `sandbox.leer`: hereda contención y lista negra de secretos de file_read.
/// Devuelve las coincidencias y si quedaron ficheros sin verificar.
fn verificar(
    sandbox: &SandboxArchivos,
    base: &Path,
    candidatas: &[String],
    expresion: &regex::Regex,
    glob: Option<&str>,
    limite: usize,
) -> (Vec<Coincidencia>, bool) {
    let mut halladas = Vec::new();
    let mut verificados = 0usize;
    for rel in candidatas {
        let Some(rel_s) = rel_segura(rel) else {
            continue;
        };
        if let Some(g) = glob {
            let nombre = rel_s.rsplit('/').next().unwrap_or(&rel_s);
            if nombre != g && !glob_simple(g, nombre) {
                continue;
            }
        }
        /* Prefiltro barato por tamaño antes de leer (TOCTOU aceptado: si el
         * fichero crece entre el stat y la lectura, `leer` lo trunca). */
        let absoluta = base.join(rel_s.replace('/', std::path::MAIN_SEPARATOR_STR));
        if std::fs::metadata(&absoluta).is_ok_and(|m| m.len() > CONTENT_MAX_BYTES_FICHERO) {
            continue;
        }
        if verificados >= CONTENT_MAX_VERIFICAR_FICHEROS {
            return (halladas, true);
        }
        verificados += 1;
        /* `leer` falla en secretos o carreras (borrado entre índice y
         * lectura): se omite el fichero sin romper la búsqueda. */
        let Ok((contenido, _)) = sandbox.leer(&rel_s, CONTENT_MAX_BYTES_FICHERO as usize) else {
            continue;
        };
        if contenido.contains('\0') {
            continue;
        }
        for (n, linea) in contenido.lines().enumerate() {
            if expresion.is_match(linea) {
                halladas.push(Coincidencia {
                    ruta: rel_s.clone(),
                    linea: n as u64 + 1,
                    texto: recortar_linea(linea),
                });
                if halladas.len() >= limite {
                    return (halladas, true);
                }
            }
        }
    }
    (halladas, false)
}

/// Recorrido local acotado de emergencia (índice no disponible): misma
/// confirmación por regex y misma lectura vía `sandbox.leer`.
fn recorrido_local(
    sandbox: &SandboxArchivos,
    base: &Path,
    expresion: &regex::Regex,
    glob: Option<&str>,
) -> Vec<Coincidencia> {
    struct Estado<'a> {
        sandbox: &'a SandboxArchivos,
        base: &'a Path,
        expresion: &'a regex::Regex,
        glob: Option<&'a str>,
        ficheros: usize,
        halladas: Vec<Coincidencia>,
    }
    fn visitar(estado: &mut Estado<'_>, dir: &Path, profundidad: usize) {
        if profundidad > LOCAL_MAX_PROFUNDIDAD || estado.ficheros >= LOCAL_MAX_FICHEROS {
            return;
        }
        let Ok(entradas) = std::fs::read_dir(dir) else {
            return;
        };
        for entrada in entradas.flatten() {
            if estado.ficheros >= LOCAL_MAX_FICHEROS {
                return;
            }
            let ruta = entrada.path();
            if ruta.is_dir() {
                visitar(estado, &ruta, profundidad + 1);
            } else if ruta.is_file() {
                let Ok(rel) = ruta.strip_prefix(estado.base) else {
                    continue;
                };
                let rel_s = rel.to_string_lossy().replace('\\', "/");
                if let Some(g) = estado.glob {
                    let nombre = rel_s.rsplit('/').next().unwrap_or(&rel_s);
                    if nombre != g && !glob_simple(g, nombre) {
                        continue;
                    }
                }
                if entrada
                    .metadata()
                    .is_ok_and(|m| m.len() > CONTENT_MAX_BYTES_FICHERO)
                {
                    continue;
                }
                estado.ficheros += 1;
                let Ok((contenido, _)) = estado
                    .sandbox
                    .leer(&rel_s, CONTENT_MAX_BYTES_FICHERO as usize)
                else {
                    continue;
                };
                if contenido.contains('\0') {
                    continue;
                }
                for (n, linea) in contenido.lines().enumerate() {
                    if estado.expresion.is_match(linea) {
                        estado.halladas.push(Coincidencia {
                            ruta: rel_s.clone(),
                            linea: n as u64 + 1,
                            texto: recortar_linea(linea),
                        });
                    }
                }
            }
        }
    }
    let mut estado = Estado {
        sandbox,
        base,
        expresion,
        glob,
        ficheros: 0,
        halladas: Vec::new(),
    };
    visitar(&mut estado, base, 0);
    estado.halladas
}

/// Recorta una línea a `CONTENT_MAX_LINEA_CHARS` (por caracteres, no bytes).
fn recortar_linea(linea: &str) -> String {
    let sin_retorno = linea.trim_end_matches(['\r', '\n']);
    if sin_retorno.chars().count() <= CONTENT_MAX_LINEA_CHARS {
        return sin_retorno.to_string();
    }
    sin_retorno.chars().take(CONTENT_MAX_LINEA_CHARS).collect()
}

/// Formatea `ruta:linea:contenido` acotando la salida total a ~12 KB.
fn formatear_coincidencias(halladas: &[Coincidencia], limite: usize, mas_posibles: bool) -> String {
    let mut lineas = Vec::new();
    let mut bytes = 0usize;
    for c in halladas {
        let linea = format!("{}:{}:{}", c.ruta, c.linea, c.texto);
        bytes += linea.len() + 1;
        if bytes > CONTENT_MAX_SALIDA_BYTES {
            break;
        }
        lineas.push(linea);
    }
    let mut salida = lineas.join("\n");
    if lineas.len() < halladas.len() || halladas.len() >= limite || mas_posibles {
        salida.push_str("\n[truncado: hay más resultados; afina con `glob` o `limite`]");
    }
    salida
}

/// Formatea la variante `solo_archivos`: una ruta por línea.
fn formatear_rutas(rutas: &[&str]) -> String {
    rutas.join("\n")
}

#[cfg(test)]
mod pruebas {
    use super::*;
    use std::sync::Arc;

    fn dir_aislada(nombre: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-cs-{}-{}-{nombre}",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("crear dir temporal");
        dir
    }

    fn ctx_con_sandbox(
        sandbox: Arc<SandboxArchivos>,
        persistencia: &crate::contrato_tests::PersistenciaMock,
    ) -> AgentToolContext<'_> {
        AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: uuid::Uuid::new_v4(),
            persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: Some(sandbox),
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        }
    }

    /// Limpia workspace e índice para no acumular temp entre corridas.
    fn limpiar(dir: &Path, sandbox: &SandboxArchivos) {
        let base = base_canonica(sandbox);
        std::fs::remove_dir_all(dir_indice_para(&base)).ok();
        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn encuentra_texto_con_indice_integrado() {
        let dir = dir_aislada("integrado");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("a.rs", "fn registrar_tools_archivo() {}\n// nada\n")
            .expect("seed a.rs");
        sandbox
            .escribir("b.txt", "registrar_tools_archivo aparece aquí\n")
            .expect("seed b.txt");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "registrar_tools_archivo"}))
            .await
            .expect("el motor integrado responde");
        assert!(
            resultado.contenido.contains("a.rs:1:"),
            "coincidencia con línea: {}",
            resultado.contenido
        );
        assert!(resultado.contenido.contains("b.txt:1:"));
        assert!(
            resultado.resumen.contains("[motor: tgrep integrado"),
            "el resumen declara el motor: {}",
            resultado.resumen
        );
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }

    #[tokio::test]
    async fn respeta_glob_y_solo_archivos() {
        let dir = dir_aislada("glob");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("main.rs", "llamada centinela_uno()\n")
            .expect("seed rs");
        sandbox
            .escribir("nota.md", "centinela_uno también aquí\n")
            .expect("seed md");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolContentSearch
            .ejecutar(
                &ctx,
                json!({"patron": "centinela_uno", "glob": "*.rs", "solo_archivos": true}),
            )
            .await
            .expect("búsqueda con glob");
        assert_eq!(resultado.contenido.trim(), "main.rs");
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }

    #[tokio::test]
    async fn ve_contenido_escrito_despues_del_primer_indice() {
        let dir = dir_aislada("frescura");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("viejo.rs", "contenido inicial sin marca\n")
            .expect("seed");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let primero = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "marca_fresca"}))
            .await
            .expect("primera búsqueda");
        assert!(primero.contenido.contains("Sin resultados"));

        /* Escritura posterior a través del sandbox (misma sesión del
         * agente): la siguiente búsqueda debe verla (rebuild por foto). */
        let sandbox_escritor = ctx.sandbox_archivos.as_ref().expect("sandbox en contexto");
        sandbox_escritor
            .escribir("nuevo.rs", "aquí está la marca_fresca\n")
            .expect("escritura posterior");
        let segundo = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "marca_fresca"}))
            .await
            .expect("segunda búsqueda");
        assert!(
            segundo.contenido.contains("nuevo.rs:1:"),
            "ve la escritura posterior: {}",
            segundo.contenido
        );
        assert!(
            segundo.resumen.contains("índice reconstruido"),
            "el resumen avisa del rebuild: {}",
            segundo.resumen
        );
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }

    #[tokio::test]
    async fn patron_vacio_e_invalido_fallan_cerrado() {
        let dir = dir_aislada("args");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let error = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "   "}))
            .await
            .expect_err("patrón vacío → error");
        assert!(error.to_string().contains("vacío"));
        let error = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "("}))
            .await
            .expect_err("sintaxis inválida → error");
        assert!(error.to_string().contains("inválido"));
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }

    #[tokio::test]
    async fn ignora_binarios_y_secretos() {
        let dir = dir_aislada("binsec");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        let mut binario = b"cabecera marca_oculta".to_vec();
        binario.push(0);
        binario.extend_from_slice(b"cola marca_oculta");
        std::fs::write(dir.join("a.bin"), binario).expect("seed binario");
        /* `.env` está en la lista negra de secretos: `escribir` lo
         * rechaza, así que se siembra por filesystem para probar que la
         * lectura de verificación lo excluye aunque esté indexado. */
        std::fs::write(dir.join(".env"), "marca_oculta=secreto\n").expect("seed secreto");
        sandbox
            .escribir("ok.txt", "aquí sí hay marca_oculta\n")
            .expect("seed texto");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "marca_oculta"}))
            .await
            .expect("búsqueda");
        assert!(
            !resultado.contenido.contains("a.bin"),
            "el binario no aparece: {}",
            resultado.contenido
        );
        assert!(
            !resultado.contenido.contains(".env"),
            "el secreto no aflora: {}",
            resultado.contenido
        );
        assert!(resultado.contenido.contains("ok.txt:1:"));
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }

    #[tokio::test]
    async fn alternancia_regex_funciona() {
        let dir = dir_aislada("alternancia");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("t.rs", "// alfa: uno\n// beta: dos\n// nada\n")
            .expect("seed");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolContentSearch
            .ejecutar(&ctx, json!({"patron": "alfa|beta"}))
            .await
            .expect("alternancia");
        assert!(resultado.contenido.contains("t.rs:1:"));
        assert!(resultado.contenido.contains("t.rs:2:"));
        assert!(!resultado.contenido.contains("t.rs:3:"));
        let sandbox2 = SandboxArchivos::nuevo(&dir).expect("sandbox");
        limpiar(&dir, &sandbox2);
    }
}
