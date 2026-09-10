/* [29-08-2026] Tools de archivo del agente (plan-agente-ia-plugin, Fase 2).
 * SOLO se registran en AGENTE_MODO=local (dev). En producción no existen,
 * ni siquiera para admin (nunca editar el filesystem del contenedor).
 * file_write/file_patch son `efecto: true` → requieren aprobación en modo
 * predeterminado (política de modos, sección 9.2).
 *
 * Portado a Glory Harness (plan 318A-13, Fase 1c): solo depende de
 * `SandboxArchivos`, `diff` y el trait `AgentTool` del núcleo (agnóstica). */

use crate::diff::diff_lineas;
use crate::error::{Error, Result};
use crate::sandbox::SandboxArchivos;
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

const MAX_LECTURA_BYTES: usize = 1_048_576; // 1MB (truncado con aviso)
const MAX_LECTURA_LINEAS: u64 = 400;

/// Límite de búsqueda de archivos: resultados, profundidad y tamaño agregado.
const FILE_SEARCH_MAX_RESULTADOS: usize = 50;
const FILE_SEARCH_MAX_PROFUNDIDAD: usize = 6;
const FILE_SEARCH_MAX_TAMANO_AGREGADO: u64 = 2 * 1024 * 1024; // 2MB

pub struct ToolFileRead;

#[async_trait]
impl AgentTool for ToolFileRead {
    fn id(&self) -> &'static str {
        "file_read"
    }
    fn descripcion(&self) -> &'static str {
        "Lee un archivo del workspace local y devuelve su contenido.\nFORMATO DE SALIDA: el contenido crudo dentro de un bloque ``` (con aviso si se truncó); con rango, el encabezado declara el rango leído (p. ej. líneas 1-40 de 200).\nLÍMITES: máx 1 MB por lectura (se trunca con aviso); con `offset_linea`/`limite_lineas` (1-based, obligatorios juntos) solo se lee esa ventana, de máximo 400 líneas, y se indica si hay más; solo rutas dentro del workspace; archivos de secretos bloqueados.\nCUÁNDO USARLA: antes de editar un archivo (file_patch/file_write) o para responder sobre código existente; para archivos grandes lee por rangos de ~40-400 líneas encadenando `offset_linea`. Para localizar archivos usa file_search.\nERRORES: ruta inexistente, fuera del workspace, bloqueada por secreto o rango fuera de límites."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string", "description": "Ruta relativa al workspace (ej. src/main.rs)"},
                "offset_linea": {"type": "integer", "minimum": 1, "description": "Primera línea a leer (1-based). Opcional: sin él se lee desde el inicio."},
                "limite_lineas": {"type": "integer", "minimum": 1, "maximum": 400, "description": "Máximo de líneas de la ventana (hasta 400). Opcional: sin él se lee el archivo completo hasta 1 MB."}
            },
            "required": ["ruta"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let sandbox = obtener_sandbox(ctx)?;
        let ruta = argumentos
            .get("ruta")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("ruta requerida".into()))?;
        let offset = argumentos.get("offset_linea").and_then(Value::as_u64);
        let limite = argumentos.get("limite_lineas").and_then(Value::as_u64);
        if limite.is_some_and(|valor| valor > MAX_LECTURA_LINEAS) {
            return Err(Error::Argumentos(format!(
                "limite_lineas no puede superar {MAX_LECTURA_LINEAS}"
            )));
        }
        // Sin rango: comportamiento actual (archivo completo, truncado a 1 MB).
        let (contenido, aviso, resumen_rango) = match (offset, limite) {
            (None, None) => {
                let (contenido, truncado) = sandbox.leer(ruta, MAX_LECTURA_BYTES)?;
                let aviso = if truncado {
                    "\n[AVISO: archivo truncado a 1MB]".to_string()
                } else {
                    String::new()
                };
                (contenido, aviso, None)
            }
            (Some(off), Some(lim)) => {
                let (texto, total, hay_mas) =
                    sandbox.leer_rango_lineas(ruta, off as usize, lim as usize)?;
                // Línea final real de la ventana: el archivo pudo acabar antes
                // de `lim`, así que se deriva del texto devuelto (las líneas se
                // unen con `\n`, luego el nº de saltos = líneas devueltas − 1).
                let fin = if texto.is_empty() {
                    off.saturating_sub(1)
                } else {
                    off + texto.matches('\n').count() as u64
                };
                let ventana = if hay_mas {
                    format!(" (hay más; usa offset_linea: {} para continuar)", fin + 1)
                } else {
                    String::new()
                };
                let resumen = format!("líneas {off}-{fin} de {total}{ventana}");
                (texto, String::new(), Some(resumen))
            }
            // Solo uno de los dos: sin ventana completa, no tiene sentido.
            _ => {
                return Err(Error::Argumentos(
                    "offset_linea y limite_lineas deben ir juntos".into(),
                ));
            }
        };
        let cabecera = match &resumen_rango {
            Some(rango) => format!("[lectura de {rango}]\n"),
            None => String::new(),
        };
        let resumen = match &resumen_rango {
            Some(rango) => format!("lectura {} · {rango}", sandbox.ruta_presentable(ruta)),
            None => format!(
                "lectura {} ({} bytes)",
                sandbox.ruta_presentable(ruta),
                contenido.len()
            ),
        };
        Ok(AgentToolResult::ok(
            format!("{cabecera}```\n{contenido}\n```{aviso}"),
            resumen,
        ))
    }
}

pub struct ToolFileWrite;

#[async_trait]
impl AgentTool for ToolFileWrite {
    fn id(&self) -> &'static str {
        "file_write"
    }
    fn descripcion(&self) -> &'static str {
        "Escribe un archivo COMPLETO en el workspace (crea o sobrescribe). Requiere aprobación en modo predeterminado.\nREGLAS DE USO: archivo nuevo o reescritura de casi todo (~>= 80%) → file_write. Cambio puntual de < ~20% del archivo → file_patch (más seguro).\nFORMATO DE SALIDA: confirmación con ruta y bytes escritos.\nLÍMITES: contenido no vacío, máx 1 MB; solo rutas dentro del workspace.\nERRORES: ruta fuera del workspace o bloqueada por el sandbox."
    }
    fn efecto(&self) -> bool {
        true
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string", "description": "Ruta relativa al workspace"},
                "contenido": {"type": "string", "description": "Contenido COMPLETO del archivo (reescribe todo; para cambios puntuales usa file_patch)"}
            },
            "required": ["ruta", "contenido"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let sandbox = obtener_sandbox(ctx)?;
        let ruta = argumentos
            .get("ruta")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("ruta requerida".into()))?;
        let contenido = argumentos
            .get("contenido")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("contenido requerido".into()))?;
        /* Diff contra el contenido previo (archivo nuevo → vacío) para
         * mostrarlo en el front (Fase 4). */
        let previo = sandbox
            .leer(ruta, MAX_LECTURA_BYTES)
            .map(|(contenido_previo, _)| contenido_previo)
            .unwrap_or_default();
        /* [318A-16 F5] Modo plan: la escritura NO se aplica; se registra la
         * propuesta (diff contra el contenido actual) en la store del plan.
         * El humano aprueba el diff acumulado y `aplicar_plan` escribe. */
        if let Some(plan) = &ctx.plan {
            let diff = crate::plan::registrar_cambio(plan, ruta, &previo, contenido);
            return Ok(AgentToolResult::ok_con_diff(
                format!(
                    "PROPUESTA (modo plan) para '{}': registrada, NO aplicada. Espera la aprobación del plan para escribir.",
                    sandbox.ruta_presentable(ruta)
                ),
                format!("propuesta {}", sandbox.ruta_presentable(ruta)),
                diff,
            ));
        }
        sandbox.escribir(ruta, contenido)?;
        Ok(AgentToolResult::ok_con_diff(
            format!(
                "Archivo '{}' escrito ({} bytes).",
                sandbox.ruta_presentable(ruta),
                contenido.len()
            ),
            format!("escritura {}", sandbox.ruta_presentable(ruta)),
            diff_lineas(&previo, contenido),
        ))
    }
}

pub struct ToolFilePatch;

#[async_trait]
impl AgentTool for ToolFilePatch {
    fn id(&self) -> &'static str {
        "file_patch"
    }
    fn descripcion(&self) -> &'static str {
        "Aplica un reemplazo puntual (buscar → reemplazar) dentro de un archivo. Requiere aprobación en modo predeterminado.\nREGLAS DE USO: cambio puntual de < ~20% del archivo → file_patch; crear o reescribir casi todo → file_write.\nREQUISITOS: 'buscar' debe ser texto EXACTO y ÚNICO en el archivo (respeta indentación). Si aparece N veces la tool falla con error claro: amplía el fragmento hasta que sea único o usa file_write.\nFORMATO DE SALIDA: confirmación con la ruta parcheada.\nERRORES: buscar vacío, no encontrado, o ambiguo (N ocurrencias)."
    }
    fn efecto(&self) -> bool {
        true
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string", "description": "Ruta relativa al workspace"},
                "buscar": {"type": "string", "description": "Texto EXACTO y ÚNICO a reemplazar (incluye indentación; si aparece varias veces, amplía el fragmento)"},
                "reemplazar": {"type": "string", "description": "Texto nuevo que sustituye a 'buscar'"}
            },
            "required": ["ruta", "buscar", "reemplazar"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let sandbox = obtener_sandbox(ctx)?;
        let ruta = argumentos
            .get("ruta")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("ruta requerida".into()))?;
        let buscar = argumentos
            .get("buscar")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("buscar requerido".into()))?;
        let reemplazar = argumentos
            .get("reemplazar")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        if buscar.is_empty() {
            return Err(Error::Argumentos("buscar no puede estar vacío".into()));
        }
        let (original, _) = sandbox.leer(ruta, MAX_LECTURA_BYTES)?;
        let ocurrencias = original.matches(buscar).count();
        if ocurrencias == 0 {
            return Err(Error::NoEncontrado(format!(
                "No se encontró '{buscar}' en '{ruta}'"
            )));
        }
        if ocurrencias > 1 {
            return Err(Error::Argumentos(format!(
                "'{buscar}' aparece {ocurrencias} veces; usa file_write o un patrón más específico"
            )));
        }
        let nuevo = original.replacen(buscar, &reemplazar, 1);
        /* [318A-16 F5] Modo plan: mismo desvío que file_write — se registra
         * la propuesta y NO se escribe hasta la aprobación del plan. */
        if let Some(plan) = &ctx.plan {
            let diff = crate::plan::registrar_cambio(plan, ruta, &original, &nuevo);
            return Ok(AgentToolResult::ok_con_diff(
                format!(
                    "PROPUESTA (modo plan) para '{}': registrada, NO aplicada. Espera la aprobación del plan para aplicar el parche.",
                    sandbox.ruta_presentable(ruta)
                ),
                format!("propuesta {}", sandbox.ruta_presentable(ruta)),
                diff,
            ));
        }
        sandbox.escribir(ruta, &nuevo)?;
        Ok(AgentToolResult::ok_con_diff(
            format!("Parche aplicado en '{}'.", sandbox.ruta_presentable(ruta)),
            format!("parche {}", sandbox.ruta_presentable(ruta)),
            diff_lineas(&original, &nuevo),
        ))
    }
}

/// Búsqueda de archivos dentro del workspace con límites (profundidad,
/// resultados y tamaño agregado — un glob recursivo sobre OneDrive puede
/// bloquear el proceso si no se acota).
pub struct ToolFileSearch;

#[async_trait]
impl AgentTool for ToolFileSearch {
    fn id(&self) -> &'static str {
        "file_search"
    }
    fn descripcion(&self) -> &'static str {
        "Busca archivos por nombre o patrón dentro del workspace y devuelve rutas relativas.\nFORMATO DE SALIDA: una ruta relativa por línea (máx 50).\nLÍMITES: profundidad 6, 2 MB agregados; ignora node_modules, target, .git, .next y dist. Patrones: subcadena ('main'), '*.rs' (termina en) o 'main*' (empieza por).\nCUÁNDO USARLA: localizar archivos antes de leer/editarlos; NO devuelve contenido.\nERRORES: sin resultados → 'Sin resultados.'"
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patron": {"type": "string", "description": "Subcadena o patrón del nombre (ej. 'main', '*.rs')"}
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
            .ok_or_else(|| Error::Argumentos("patron requerido".into()))?
            .to_ascii_lowercase();
        let raiz = sandbox
            .resolver(".")
            .unwrap_or_else(|_| sandbox.raiz().to_path_buf());
        let mut resultados: Vec<String> = Vec::new();
        let mut tamano_agregado = 0u64;
        buscar_recursivo(
            &raiz,
            &raiz,
            &patron,
            0,
            &mut resultados,
            &mut tamano_agregado,
        );
        let contenido = if resultados.is_empty() {
            "Sin resultados.".to_string()
        } else {
            resultados.join("\n")
        };
        Ok(AgentToolResult::ok(
            contenido,
            format!("{} archivos para '{patron}'", resultados.len()),
        ))
    }
}

fn buscar_recursivo(
    raiz: &std::path::Path,
    dir: &std::path::Path,
    patron: &str,
    profundidad: usize,
    resultados: &mut Vec<String>,
    tamano_agregado: &mut u64,
) {
    if profundidad > FILE_SEARCH_MAX_PROFUNDIDAD
        || resultados.len() >= FILE_SEARCH_MAX_RESULTADOS
        || *tamano_agregado >= FILE_SEARCH_MAX_TAMANO_AGREGADO
    {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let ruta = entry.path();
        let nombre = entry.file_name().to_string_lossy().to_lowercase();
        if ruta.is_dir() {
            /* Saltar carpetas que son claramente no-código y enormes. */
            if matches!(
                nombre.as_str(),
                "node_modules" | "target" | ".git" | ".next" | "dist"
            ) {
                continue;
            }
            buscar_recursivo(
                raiz,
                &ruta,
                patron,
                profundidad + 1,
                resultados,
                tamano_agregado,
            );
        } else if nombre.contains(patron) || glob_simple(patron, &nombre) {
            if let Ok(metadata) = entry.metadata() {
                *tamano_agregado += metadata.len();
            }
            let rel = ruta
                .strip_prefix(raiz)
                .unwrap_or(&ruta)
                .to_string_lossy()
                .replace('\\', "/");
            resultados.push(rel);
        }
    }
}

/// Soporte mínimo de glob: `*.rs` → termina en .rs; `main*` → empieza por main.
pub(crate) fn glob_simple(patron: &str, nombre: &str) -> bool {
    if let Some(resto) = patron.strip_prefix("*.") {
        return nombre.ends_with(&format!(".{resto}"));
    }
    if patron.starts_with('*') {
        return nombre.ends_with(patron.trim_start_matches('*'));
    }
    if patron.ends_with('*') {
        return nombre.starts_with(patron.trim_end_matches('*'));
    }
    false
}

/// Obtiene el sandbox del contexto. El runtime lo inyecta en el contexto de
/// las tools cuando AGENTE_MODO=local; si no hay sandbox (prod), error claro.
pub(crate) fn obtener_sandbox<'a>(ctx: &'a AgentToolContext<'a>) -> Result<&'a SandboxArchivos> {
    ctx.sandbox_archivos
        .as_ref()
        .map(|s| s.as_ref())
        .ok_or_else(|| {
            Error::Validacion(
                "Las tools de archivo solo están disponibles en modo local (AGENTE_MODO=local)"
                    .into(),
            )
        })
}

/// Registra las tools de archivo SOLO si hay sandbox (local). Devuelve false
/// en producción (fail-closed: no se registran, ni siquiera admin).
pub fn registrar_tools_archivo(
    registry: &mut crate::tool::AgentToolRegistry,
    sandbox: Option<Arc<SandboxArchivos>>,
) -> bool {
    let Some(sandbox) = sandbox else {
        return false;
    };
    /* El sandbox viaja en el contexto vía registro adjunto al runtime. Las
     * tools lo leen del contexto. */
    registry.registrar_sandbox(sandbox);
    registry.registrar(Box::new(ToolFileRead));
    registry.registrar(Box::new(ToolFileWrite));
    registry.registrar(Box::new(ToolFilePatch));
    registry.registrar(Box::new(ToolFileSearch));
    /* [089A-6] Búsqueda por contenido (índice tgrep integrado + fallback
     * local; vive en `content_search.rs`). */
    registry.registrar(Box::new(super::content_search::ToolContentSearch));
    /* [Bloque 3, F7] El mapa necesita la raíz del workspace: mismo
     * fail-closed (solo con sandbox local). */
    crate::repo_map::registrar_tool_repo_map(registry);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_con_sandbox() -> (crate::tool::AgentToolRegistry, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("agente-tools-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        let mut registry = crate::tool::AgentToolRegistry::new();
        registrar_tools_archivo(&mut registry, Some(Arc::new(sandbox)));
        (registry, dir)
    }

    #[test]
    fn fail_closed_sin_sandbox() {
        let mut registry = crate::tool::AgentToolRegistry::new();
        assert!(!registrar_tools_archivo(&mut registry, None));
        assert!(registry.ids().is_empty());
    }

    #[test]
    fn registra_las_tools_con_sandbox() {
        let (registry, _dir) = registry_con_sandbox();
        let ids = registry.ids();
        assert!(ids.contains(&"file_read"));
        assert!(ids.contains(&"file_write"));
        assert!(ids.contains(&"file_patch"));
        assert!(ids.contains(&"file_search"));
        /* [089A-6] content_search viaja con las tools de archivo. */
        assert!(ids.contains(&"content_search"));
        /* [Bloque 3, F7] repo_map viaja con las tools de archivo. */
        assert!(ids.contains(&"repo_map"));
        /* write/patch son efecto; read/search/map no. */
        assert!(registry.tiene_efecto("file_write"));
        assert!(registry.tiene_efecto("file_patch"));
        assert!(!registry.tiene_efecto("file_read"));
        assert!(!registry.tiene_efecto("content_search"));
        assert!(!registry.tiene_efecto("repo_map"));
    }

    fn dir_aislada(nombre: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-f5-{}-{}-{nombre}",
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

    fn ctx_con_plan(
        sandbox: Arc<SandboxArchivos>,
        persistencia: &crate::contrato_tests::PersistenciaMock,
        plan: crate::plan::PlanCompartida,
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
            plan: Some(plan),
            navegador: None,
        }
    }

    /* [318A-16 F5] Modo plan: file_write NO escribe; registra la propuesta
     * en la store del plan y devuelve su diff. El disco queda intacto. */
    #[tokio::test]
    async fn modo_plan_file_write_registra_propuesta_sin_tocar_disco() {
        let dir = dir_aislada("plan-write");
        std::fs::write(dir.join("doc.txt"), "linea1\n").expect("seed");
        let sandbox = Arc::new(SandboxArchivos::nuevo(&dir).expect("sandbox"));
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let plan = Arc::new(std::sync::RwLock::new(crate::plan::PlanPropuesto::default()));
        let ctx = ctx_con_plan(sandbox.clone(), &persistencia, plan.clone());

        let resultado = ToolFileWrite
            .ejecutar(
                &ctx,
                json!({"ruta": "doc.txt", "contenido": "linea1\nlinea2\n"}),
            )
            .await
            .expect("tool responde");
        assert!(resultado.contenido.contains("PROPUESTA (modo plan)"));
        assert!(resultado.diff.is_some(), "el diff llega al humano");
        /* El disco NO cambió; la propuesta está en la store. */
        let leido = sandbox.leer("doc.txt", 1024).expect("leer");
        assert_eq!(leido.0, "linea1\n", "modo plan nunca escribe");
        assert!(crate::plan::tiene_cambios(&plan));
        let resumen = crate::plan::resumen_plan(&plan);
        assert!(resumen.contains("+linea2"));

        /* Y la aprobación aplica exactamente ese diff (regla de una sola
         * aplicación cubierta en plan::tests). */
        let ok = crate::plan::aplicar_plan(&plan, &sandbox).expect("aplicar");
        assert!(ok.contains("Propuesta aplicada"));
        let aplicado = sandbox.leer("doc.txt", 1024).expect("leer");
        assert_eq!(aplicado.0, "linea1\nlinea2\n");
        std::fs::remove_dir_all(&dir).ok();
    }

    /* [318A-16 F5] Modo plan: file_patch idem — propuesta, sin escribir. */
    #[tokio::test]
    async fn modo_plan_file_patch_registra_propuesta_sin_tocar_disco() {
        let dir = dir_aislada("plan-patch");
        std::fs::write(dir.join("doc.txt"), "hola\n").expect("seed");
        let sandbox = Arc::new(SandboxArchivos::nuevo(&dir).expect("sandbox"));
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let plan = Arc::new(std::sync::RwLock::new(crate::plan::PlanPropuesto::default()));
        let ctx = ctx_con_plan(sandbox.clone(), &persistencia, plan.clone());

        let resultado = ToolFilePatch
            .ejecutar(
                &ctx,
                json!({"ruta": "doc.txt", "buscar": "hola", "reemplazar": "adiós"}),
            )
            .await
            .expect("tool responde");
        assert!(resultado.contenido.contains("PROPUESTA (modo plan)"));
        let leido = sandbox.leer("doc.txt", 1024).expect("leer");
        assert_eq!(leido.0, "hola\n", "modo plan nunca aplica el parche");
        assert!(crate::plan::resumen_plan(&plan).contains("-hola"));
        std::fs::remove_dir_all(&dir).ok();
    }

    /* [318A-15 F5] `file_patch` exige que el fragmento `buscar` exista y sea
     * ÚNICO (paridad opencode `edit`): si no aparece o aparece N veces, falla
     * con mensaje claro en vez de parchear la primera ocurrencia a ciegas. */
    #[tokio::test]
    async fn file_patch_old_duplicado_falla_con_mensaje_claro() {
        let dir = dir_aislada("patch-duplicado");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("a.txt", "primera x\nsegunda x\n")
            .expect("escribir");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let error = ToolFilePatch
            .ejecutar(
                &ctx,
                json!({"ruta": "a.txt", "buscar": "x", "reemplazar": "z"}),
            )
            .await
            .expect_err("'x' aparece 2 veces → debe fallar");
        let mensaje = error.to_string();
        assert!(
            mensaje.contains("2 veces") && mensaje.contains("más específico"),
            "mensaje claro sobre la ambigüedad: {mensaje}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_patch_old_ausente_falla_no_encontrado() {
        let dir = dir_aislada("patch-ausente");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        sandbox
            .escribir("a.txt", "contenido estable\n")
            .expect("escribir");
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let error = ToolFilePatch
            .ejecutar(
                &ctx,
                json!({"ruta": "a.txt", "buscar": "fantasma", "reemplazar": "z"}),
            )
            .await
            .expect_err("sin ocurrencias → debe fallar");
        let mensaje = error.to_string();
        assert!(
            mensaje.contains("No se encontró") && mensaje.contains("fantasma"),
            "mensaje de no encontrado: {mensaje}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /* [318A-15 F5] E2E con fixture (sin proveedor): secuencia scriptada que
     * replica el flujo real del runtime — el "modelo" crea el plan con `todo`,
     * edita con file_patch y completa el ítem. Aserciones deterministas sobre
     * el resultado que el runtime devuelve al contexto del LLM. */
    #[tokio::test]
    async fn e2e_fixture_todo_patch_y_cierre_de_plan() {
        let dir = dir_aislada("e2e-todo-patch");
        std::fs::write(dir.join("app.txt"), "hola mundo\n").expect("seed");
        let sandbox = Arc::new(SandboxArchivos::nuevo(&dir).expect("sandbox"));
        let mut registry = crate::tool::AgentToolRegistry::new();
        assert!(registrar_tools_archivo(
            &mut registry,
            Some(sandbox.clone())
        ));
        crate::todo::registrar_tool_todo(&mut registry);
        assert!(registry.ids().contains(&"file_patch"));
        assert!(registry.ids().contains(&"todo"));

        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: uuid::Uuid::new_v4(),
            persistencia: &persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: registry.sandbox(),
            dominio: None,
            todo: registry.todo(),
            plan: None,
            navegador: None,
        };

        let plan = registry
            .ejecutar(
                "todo",
                &ctx,
                json!({"accion": "crear", "texto": "Editar el saludo"}),
            )
            .await
            .expect("todo crear");
        assert!(plan.ok && plan.contenido.contains("[ ] Editar el saludo"));

        let parche = registry
            .ejecutar(
                "file_patch",
                &ctx,
                json!({"ruta": "app.txt", "buscar": "hola mundo", "reemplazar": "adiós mundo"}),
            )
            .await
            .expect("file_patch");
        assert!(
            parche.ok && parche.diff.is_some(),
            "el patch devuelve su diff"
        );

        let cierre = registry
            .ejecutar("todo", &ctx, json!({"accion": "completar", "id": 1}))
            .await
            .expect("todo completar");
        assert!(
            cierre.contenido.contains("[x] Editar el saludo"),
            "el plan actualizado vuelve al contexto del modelo: {}",
            cierre.contenido
        );
        let leido = SandboxArchivos::nuevo(&dir)
            .expect("sandbox 2")
            .leer("app.txt", 1024)
            .expect("leer");
        assert_eq!(leido.0, "adiós mundo\n", "la edición quedó aplicada");
        std::fs::remove_dir_all(&dir).ok();
    }

    // ── [318A-16 F4] file_read por rangos de líneas ────────────────────────

    /// Fixture: archivo de `n` líneas "línea k" (1-based).
    fn archivo_de_lineas(sandbox: &SandboxArchivos, nombre: &str, n: usize) {
        let contenido = (1..=n)
            .map(|k| format!("línea {k}"))
            .collect::<Vec<_>>()
            .join("\n");
        sandbox
            .escribir(nombre, &contenido)
            .expect("escribir fixture");
    }

    #[tokio::test]
    async fn file_read_rango_valido_devuelve_ventana_con_cabecera() {
        let dir = dir_aislada("f4-rango");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        archivo_de_lineas(&sandbox, "grande.rs", 200);
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolFileRead
            .ejecutar(
                &ctx,
                json!({"ruta": "grande.rs", "offset_linea": 1, "limite_lineas": 40}),
            )
            .await
            .expect("lectura por rango");
        let contenido = resultado.contenido;
        assert!(
            contenido.contains("[lectura de líneas 1-40 de 200 (hay más"),
            "cabecera con rango y total: {contenido}"
        );
        assert!(contenido.contains("línea 1") && contenido.contains("línea 40"));
        assert!(!contenido.contains("línea 41"), "no debe salir la ventana");
        assert!(
            resultado.resumen.contains("líneas 1-40 de 200"),
            "resumen con rango: {}",
            resultado.resumen
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_read_rango_medio_sugiere_continuar_desde_el_fin_real() {
        let dir = dir_aislada("f4-rango-medio");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        archivo_de_lineas(&sandbox, "grande.rs", 200);
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolFileRead
            .ejecutar(
                &ctx,
                json!({"ruta": "grande.rs", "offset_linea": 41, "limite_lineas": 40}),
            )
            .await
            .expect("lectura por rango");
        assert!(resultado.contenido.contains("línea 41"));
        assert!(resultado.contenido.contains("línea 80"));
        assert!(!resultado.contenido.contains("línea 81"));
        assert!(resultado.contenido.contains("offset_linea: 81"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_read_rango_fuera_de_limites_falla_cerrado() {
        let dir = dir_aislada("f4-fuera");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        archivo_de_lineas(&sandbox, "corto.txt", 10);
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let error = ToolFileRead
            .ejecutar(
                &ctx,
                json!({"ruta": "corto.txt", "offset_linea": 50, "limite_lineas": 5}),
            )
            .await
            .expect_err("offset 50 sobre 10 líneas → fuera de rango");
        assert!(error.to_string().contains("fuera de rango"));

        // Un solo parámetro sin el otro: también fail-closed.
        let error2 = ToolFileRead
            .ejecutar(&ctx, json!({"ruta": "corto.txt", "offset_linea": 2}))
            .await
            .expect_err("offset sin límite → error");
        assert!(error2.to_string().contains("juntos"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn file_read_sin_rango_conserva_comportamiento_anterior() {
        let dir = dir_aislada("f4-sin-rango");
        let sandbox = SandboxArchivos::nuevo(&dir).expect("sandbox");
        archivo_de_lineas(&sandbox, "corto.txt", 3);
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = ctx_con_sandbox(Arc::new(sandbox), &persistencia);

        let resultado = ToolFileRead
            .ejecutar(&ctx, json!({"ruta": "corto.txt"}))
            .await
            .expect("lectura completa");
        assert!(resultado.contenido.contains("línea 1"));
        assert!(resultado.contenido.contains("línea 3"));
        assert!(
            !resultado.contenido.contains("[lectura de líneas"),
            "sin cabecera de rango"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
