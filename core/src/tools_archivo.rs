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
        "Lee un archivo del workspace local del proyecto. Solo rutas dentro del workspace; archivos de secretos bloqueados."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string", "description": "Ruta relativa al workspace (ej. src/main.rs)"}
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
        let (contenido, truncado) = sandbox.leer(ruta, MAX_LECTURA_BYTES)?;
        let aviso = if truncado {
            "\n[AVISO: archivo truncado a 1MB]".to_string()
        } else {
            String::new()
        };
        Ok(AgentToolResult::ok(
            format!("```\n{contenido}\n```{aviso}"),
            format!(
                "lectura {} ({} bytes)",
                sandbox.ruta_presentable(ruta),
                contenido.len()
            ),
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
        "Escribe un archivo completo en el workspace local (reemplaza el contenido). Requiere aprobación en modo predeterminado."
    }
    fn efecto(&self) -> bool {
        true
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string", "description": "Ruta relativa al workspace"},
                "contenido": {"type": "string", "description": "Contenido completo del archivo"}
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
        "Aplica un reemplazo puntual (viejo → nuevo) en un archivo del workspace. Requiere aprobación en modo predeterminado."
    }
    fn efecto(&self) -> bool {
        true
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "ruta": {"type": "string"},
                "buscar": {"type": "string", "description": "Texto exacto a reemplazar"},
                "reemplazar": {"type": "string", "description": "Texto nuevo"}
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
        "Busca archivos por nombre/patrón dentro del workspace (acotado a 50 resultados, profundidad 6, 2MB agregados)."
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
            if matches!(nombre.as_str(), "node_modules" | "target" | ".git" | ".next" | "dist") {
                continue;
            }
            buscar_recursivo(raiz, &ruta, patron, profundidad + 1, resultados, tamano_agregado);
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
fn glob_simple(patron: &str, nombre: &str) -> bool {
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
fn obtener_sandbox<'a>(ctx: &'a AgentToolContext<'a>) -> Result<&'a SandboxArchivos> {
    ctx.sandbox_archivos.as_ref().map(|s| s.as_ref()).ok_or_else(|| {
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
    fn registra_las_cuatro_tools_con_sandbox() {
        let (registry, _dir) = registry_con_sandbox();
        let ids = registry.ids();
        assert!(ids.contains(&"file_read"));
        assert!(ids.contains(&"file_write"));
        assert!(ids.contains(&"file_patch"));
        assert!(ids.contains(&"file_search"));
        /* write/patch son efecto; read/search no. */
        assert!(registry.tiene_efecto("file_write"));
        assert!(registry.tiene_efecto("file_patch"));
        assert!(!registry.tiene_efecto("file_read"));
    }
}