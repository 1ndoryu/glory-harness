//! Cliente MCP (Model Context Protocol) con transporte stdio (Bloque 3 Fase 2).
//! Referencias: claurst `mcp/` (transporte JSON-RPC línea a línea) y opencode
//! (tools MCP dinámicas bajo la política de permisos existente).
//!
//! El núcleo trae el transporte stdio (`McpProveedorStdio`); el consumidor
//! puede aportar su propio `McpProveedor` (HTTP/SSE en una fase posterior).
//! Fail-closed: sin proveedor no hay tools MCP; un error de transporte se
//! propaga como error de tool (nunca éxito falso). Cada herramienta se
//! registra como `mcp_<servidor>_<herramienta>` con categoría `mcp`, así la
//! política F3 (ask/allow/deny por categoría) la cubre sin código extra.

use crate::entorno::aplicar_entorno_minimo;
use crate::error::{Error, Result};
use crate::hooks::{normalizar_bin_hook, MAX_ARGS_HOOK, MAX_BYTES_ARGS_HOOK};
use crate::ports::{McpHerramienta, McpProveedor};
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

/// Normaliza un nombre de servidor/herramienta a un id de tool seguro:
/// minúsculas y `[a-z0-9_]` (los ids alimentan el schema OpenAI y los
/// override de permisos).
#[must_use]
pub fn sanitizar_id(parte: &str) -> String {
    let mut limpio = String::with_capacity(parte.len());
    for c in parte.chars() {
        if c.is_ascii_alphanumeric() {
            limpio.push(c.to_ascii_lowercase());
        } else if c.is_ascii_whitespace() || c == '-' || c == '.' {
            limpio.push('_');
        }
    }
    let limpio = limpio.trim_matches('_').to_string();
    if limpio.is_empty() {
        "servidor".to_string()
    } else {
        limpio
    }
}

/// Una sesión JSON-RPC 2.0 sobre stdio (una respuesta por línea, un request a
/// la vez). Los servidores MCP son single-flight por protocolo: el runtime
/// ejecuta las tools en serie, así que un Mutex por sesión es suficiente.
struct SesionStdio {
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    siguiente_id: u64,
}

impl SesionStdio {
    async fn rpc(&mut self, metodo: &str, params: Value) -> Result<Value> {
        let id = self.siguiente_id;
        self.siguiente_id += 1;
        let mut linea = serde_json::to_string(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": metodo,
            "params": params,
        }))
        .map_err(|e| Error::Interno(format!("serializar JSON-RPC: {e}")))?;
        linea.push('\n');
        self.stdin
            .write_all(linea.as_bytes())
            .await
            .map_err(|e| Error::Proveedor {
                detalle: format!("escribir al servidor MCP ({metodo}): {e}"),
                causa: None,
            })?;
        self.stdin.flush().await.map_err(|e| Error::Proveedor {
            detalle: format!("flush al servidor MCP ({metodo}): {e}"),
            causa: None,
        })?;
        let mut buf = String::new();
        loop {
            buf.clear();
            let leidos = self
                .stdout
                .read_line(&mut buf)
                .await
                .map_err(|e| Error::Proveedor {
                    detalle: format!("leer del servidor MCP ({metodo}): {e}"),
                    causa: None,
                })?;
            if leidos == 0 {
                return Err(Error::Proveedor {
                    detalle: format!("el servidor MCP cerró stdout sin responder a {metodo}"),
                    causa: None,
                });
            }
            let v: Value = serde_json::from_str(&buf).map_err(|e| Error::Proveedor {
                detalle: format!("respuesta JSON-RPC no válida: {e}"),
                causa: None,
            })?;
            // Notificaciones y respuestas de otros ids se descartan.
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return Ok(v);
            }
        }
    }

    /// Aplica el protocolo de inicialización MCP antes de `tools/list`.
    async fn inicializar(&mut self, comando: &str) -> Result<()> {
        let r = self
            .rpc(
                "initialize",
                json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "clientInfo": { "name": "glory-harness", "version": "0.1" },
                }),
            )
            .await?;
        if r.get("error").is_some() {
            return Err(Error::Proveedor {
                detalle: format!("initialize rechazado por el servidor MCP ({comando})"),
                causa: None,
            });
        }
        // `notifications/initialized` (fire-and-forget): el servidor la ignora
        // si llega antes de procesar el initialize; toleramos fallo.
        let _ = self.rpc("notifications/initialized", json!({})).await;
        Ok(())
    }
}

/// [139A-8 F3n/K7] Runtimes que pueden hospedar un servidor MCP stdio.
/// Nada de shells (`sh`, `cmd`, `powershell`) ni utilidades del sistema:
/// el `comando` lo declara la config del operador (`GLORY_MCP_CONFIG`) y sin
/// allowlist un comando inyectado ahí sería RCE en el arranque. Servidores
/// compilados propios o rutas dedicadas entran por `GLORY_MCP_ALLOW`
/// (basenames con `,`, bajo responsabilidad del operador). Misma
/// normalización por basename que K4 ([`crate::hooks`]), mismos topes de
/// argv (anti-bomba).
pub const MCP_BINARIOS_PERMITIDOS: &[&str] = &[
    "node", "npx", "python", "python3", "uv", "uvx", "bun", "deno",
];

/// Extras del operador desde `GLORY_MCP_ALLOW` (coma-separados).
fn extras_mcp_desde_env() -> Vec<String> {
    std::env::var("GLORY_MCP_ALLOW")
        .unwrap_or_default()
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Valida (comando, args) de un servidor MCP stdio. `extras` = allowlist
/// adicional (el wrapper público la lee de `GLORY_MCP_ALLOW`; los tests la
/// inyectan para no mutar el entorno del proceso).
pub fn validar_servidor_mcp_con_extras(
    comando: &str,
    args: &[String],
    extras: &[String],
) -> std::result::Result<(), String> {
    let bin = normalizar_bin_hook(comando);
    let permitido = MCP_BINARIOS_PERMITIDOS.iter().any(|b| *b == bin)
        || extras.iter().any(|e| normalizar_bin_hook(e) == bin);
    if !permitido {
        return Err(format!(
            "servidor MCP denegado ('{comando}'): binario fuera de la allowlist K7 \
             (ampliable con GLORY_MCP_ALLOW)"
        ));
    }
    if args.len() > MAX_ARGS_HOOK {
        return Err(format!(
            "servidor MCP denegado ('{comando}'): {} args superan el tope {MAX_ARGS_HOOK}",
            args.len()
        ));
    }
    let bytes: usize = args.iter().map(|a| a.len()).sum();
    if bytes > MAX_BYTES_ARGS_HOOK {
        return Err(format!(
            "servidor MCP denegado ('{comando}'): {bytes} bytes de argv superan el tope {MAX_BYTES_ARGS_HOOK}"
        ));
    }
    Ok(())
}

/// Valida con los extras del entorno actual.
pub fn validar_servidor_mcp(comando: &str, args: &[String]) -> std::result::Result<(), String> {
    validar_servidor_mcp_con_extras(comando, args, &extras_mcp_desde_env())
}

/// Proveedor MCP estándar: arranca `comando argumentos...` como proceso hijo
/// y habla JSON-RPC por stdin/stdout.
pub struct McpProveedorStdio {
    sesion: Arc<Mutex<SesionStdio>>,
    comando: String,
}

impl McpProveedorStdio {
    /// Spawn del proceso hijo. Fallar aquí es un error de config del
    /// consumidor (el binario no existe, etc.) y se propaga antes de que
    /// ninguna tool se registre (fail-closed).
    pub async fn nuevo(comando: &str, argumentos: Vec<String>) -> Result<Self> {
        // [139A-8 F3n/K7] Allowlist de binarios + topes de argv ANTES del
        // spawn: config denegada = arranque abortado (fail-closed).
        validar_servidor_mcp(comando, &argumentos).map_err(|motivo| Error::Proveedor {
            detalle: format!("servidor MCP `{comando}` bloqueado por política K7: {motivo}"),
            causa: None,
        })?;
        let mut spawn = Command::new(comando);
        spawn
            .args(&argumentos)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit());
        // [139A-8 F3n/K2] El servidor NO hereda el entorno del operador
        // (claves LLM): solo el subconjunto mínimo.
        aplicar_entorno_minimo(&mut spawn);
        let mut child = spawn.spawn().map_err(|e| Error::Proveedor {
            detalle: format!("arrancar servidor MCP `{comando}`: {e}"),
            causa: None,
        })?;
        let stdin = child.stdin.take().ok_or_else(|| Error::Proveedor {
            detalle: format!("servidor MCP `{comando}` sin stdin"),
            causa: None,
        })?;
        let stdout = child.stdout.take().ok_or_else(|| Error::Proveedor {
            detalle: format!("servidor MCP `{comando}` sin stdout"),
            causa: None,
        })?;
        let mut sesion = SesionStdio {
            stdin,
            stdout: BufReader::new(stdout),
            siguiente_id: 1,
        };
        sesion.inicializar(comando).await?;
        Ok(Self {
            sesion: Arc::new(Mutex::new(sesion)),
            comando: comando.to_string(),
        })
    }
}

/// Extrae el texto legible de `result.content` (array de bloques `{type:
/// "text", text}`) — el formato estándar de respuesta de `tools/call`.
#[must_use]
fn extraer_texto(resultado: &Value) -> String {
    let mut partes: Vec<String> = Vec::new();
    if let Some(contenido) = resultado.get("content").and_then(Value::as_array) {
        for bloque in contenido {
            if let Some(texto) = bloque.get("text").and_then(Value::as_str) {
                partes.push(texto.to_string());
            }
        }
    }
    if partes.is_empty() {
        serde_json::to_string_pretty(resultado).unwrap_or_else(|_| "(resultado sin texto)".into())
    } else {
        partes.join("\n")
    }
}

#[async_trait]
impl McpProveedor for McpProveedorStdio {
    async fn listar_herramientas(&self) -> Result<Vec<McpHerramienta>> {
        let mut sesion = self.sesion.lock().await;
        let r = sesion.rpc("tools/list", json!({})).await?;
        if let Some(error) = r.get("error") {
            return Err(Error::Proveedor {
                detalle: format!(
                    "tools/list del servidor MCP `{}`: {}",
                    self.comando,
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                ),
                causa: None,
            });
        }
        let tools = r
            .get("result")
            .and_then(|x| x.get("tools"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        Ok(tools
            .into_iter()
            .filter_map(|t| {
                let nombre = t.get("name").and_then(Value::as_str)?.to_string();
                Some(McpHerramienta {
                    nombre,
                    descripcion: t
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("(sin descripción MCP)")
                        .to_string(),
                    schema: t.get("inputSchema").cloned().unwrap_or_else(|| json!({})),
                })
            })
            .collect())
    }

    async fn llamar(&self, herramienta: &str, argumentos: Value) -> Result<Value> {
        let mut sesion = self.sesion.lock().await;
        let r = sesion
            .rpc(
                "tools/call",
                json!({ "name": herramienta, "arguments": argumentos }),
            )
            .await?;
        if let Some(error) = r.get("error") {
            return Err(Error::Proveedor {
                detalle: format!(
                    "tools/call `{herramienta}` (MCP `{}`): {}",
                    self.comando,
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("error")
                ),
                causa: None,
            });
        }
        let resultado = r.get("result").cloned().unwrap_or_else(|| json!({}));
        if resultado.get("isError").and_then(Value::as_bool) == Some(true) {
            return Err(Error::Proveedor {
                detalle: format!(
                    "el servidor MCP `{}` falló en `{herramienta}`: {}",
                    self.comando,
                    extraer_texto(&resultado)
                ),
                causa: None,
            });
        }
        Ok(resultado)
    }
}

/// Adapter `AgentTool` de una herramienta MCP ya listada. El runtime lo usa
/// exactamente igual que cualquier otra tool: permisos F3 (categoría `mcp`,
/// efecto=true → ask en modo predeterminado), auditar acciones, SSE.
pub struct ToolMcpAdapter {
    id: String,
    herramienta: McpHerramienta,
    proveedor: Arc<dyn McpProveedor>,
}

impl ToolMcpAdapter {
    #[must_use]
    pub fn nuevo(
        id: String,
        herramienta: McpHerramienta,
        proveedor: Arc<dyn McpProveedor>,
    ) -> Self {
        Self {
            id,
            herramienta,
            proveedor,
        }
    }
}

#[async_trait]
impl AgentTool for ToolMcpAdapter {
    fn id(&self) -> &str {
        &self.id
    }

    fn descripcion(&self) -> &str {
        &self.herramienta.descripcion
    }

    fn schema(&self) -> Value {
        self.herramienta.schema.clone()
    }

    /// Las tools MCP tienen efecto remoto por definición: la política F3 las
    /// trata como `ask` en modo predeterminado (fail-closed).
    fn efecto(&self) -> bool {
        true
    }

    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let resultado = self
            .proveedor
            .llamar(&self.herramienta.nombre, argumentos)
            .await?;
        let contenido = extraer_texto(&resultado);
        let resumen = contenido
            .chars()
            .take(120)
            .collect::<String>()
            .replace(['\n', '\r'], " ");
        Ok(AgentToolResult::ok(contenido, resumen))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::AgentToolRegistry;
    use serde_json::json;

    /// Servidor MCP stub (guion): devuelve 2 herramientas y responde a cada
    /// llamada con texto fijo. E2E determinista sin proceso real (mismo
    /// patrón de fixture que F5/F6).
    struct McpStub;

    #[async_trait]
    impl McpProveedor for McpStub {
        async fn listar_herramientas(&self) -> Result<Vec<McpHerramienta>> {
            Ok(vec![
                McpHerramienta {
                    nombre: "leer".into(),
                    descripcion: "Lee un archivo remoto".into(),
                    schema: json!({
                        "type": "object",
                        "properties": { "ruta": { "type": "string" } },
                        "required": ["ruta"],
                    }),
                },
                McpHerramienta {
                    nombre: "escribir".into(),
                    descripcion: "Escribe un archivo remoto".into(),
                    schema: json!({ "type": "object" }),
                },
            ])
        }

        async fn llamar(&self, herramienta: &str, _argumentos: Value) -> Result<Value> {
            Ok(json!({
                "content": [
                    { "type": "text", "text": format!("stub:{herramienta}:ok") }
                ]
            }))
        }
    }

    #[tokio::test]
    async fn registra_mcp_en_registry_con_categoria_y_schema() {
        let mut registry = AgentToolRegistry::new();
        registry
            .registrar_mcp("Mi Servidor", Arc::new(McpStub))
            .await
            .expect("registro MCP");
        let ids = registry.ids();
        assert!(ids.contains(&"mcp_mi_servidor_leer"), "ids: {ids:?}");
        assert!(ids.contains(&"mcp_mi_servidor_escribir"), "ids: {ids:?}");
        // Aparecen en el schema del modelo (deny silencioso no las oculta sin
        // regla/override) con la descripción del servidor.
        let schemas = registry.schemas_openai(None, "predeterminado");
        let nombres: Vec<String> = schemas
            .iter()
            .filter_map(|s| s["function"]["name"].as_str().map(str::to_string))
            .collect();
        assert!(
            nombres.contains(&"mcp_mi_servidor_leer".into()),
            "{nombres:?}"
        );
        let leer = schemas
            .iter()
            .find(|s| s["function"]["name"] == "mcp_mi_servidor_leer")
            .expect("schema de leer");
        assert_eq!(leer["function"]["description"], "Lee un archivo remoto");
        assert_eq!(
            leer["function"]["parameters"]["required"][0], "ruta",
            "el schema del servidor viaja al modelo"
        );
    }

    #[tokio::test]
    async fn deny_por_categoria_oculta_tools_mcp_del_schema() {
        let mut registry = AgentToolRegistry::new();
        registry
            .registrar_mcp("fs", Arc::new(McpStub))
            .await
            .expect("registro MCP");
        // deny silencioso de la categoría mcp → las tools no se ofrecen.
        registry.establecer_regla(crate::regla::ReglaPermiso::nueva(
            crate::regla::CAT_MCP,
            "*",
            crate::permiso::Permiso::Deny,
        ));
        let schemas = registry.schemas_openai(None, "predeterminado");
        assert!(
            schemas.iter().all(|s| !s["function"]["name"]
                .as_str()
                .is_some_and(|n| n.starts_with("mcp_"))),
            "deny mcp debe ocultarlas del schema: {schemas:?}"
        );
    }

    #[tokio::test]
    async fn adapter_ejecuta_y_devuelve_texto_del_servidor() {
        let adapter = ToolMcpAdapter::nuevo(
            "mcp_fs_leer".into(),
            McpHerramienta {
                nombre: "leer".into(),
                descripcion: "Lee un archivo remoto".into(),
                schema: json!({}),
            },
            Arc::new(McpStub),
        );
        // ctx mínimo (solo persistencia usada por herramientas de dominio;
        // las MCP no la tocan).
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = crate::tool::AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: uuid::Uuid::new_v4(),
            persistencia: &persistencia,
            web_search: None,
            web_fetch: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
            conversacion_id: uuid::Uuid::nil(),
            tx_eventos: None,
        };
        let resultado = adapter
            .ejecutar(&ctx, json!({ "ruta": "/tmp/a.txt" }))
            .await
            .expect("llamada MCP stub");
        assert!(resultado.ok);
        assert_eq!(resultado.contenido, "stub:leer:ok");
        assert_eq!(resultado.resumen, "stub:leer:ok");
    }

    #[test]
    fn sanitizar_ids_dinamicos() {
        assert_eq!(sanitizar_id("Mi Servidor"), "mi_servidor");
        assert_eq!(sanitizar_id("Servidor-A.Py"), "servidor_a_py");
        assert_eq!(sanitizar_id("héroe"), "hroe"); // no-ASCII descartado
        assert_eq!(sanitizar_id("!!!"), "servidor"); // vacío → default
    }

    #[test]
    fn extraer_texto_de_content() {
        let r = json!({
            "content": [
                {"type": "text", "text": "primera"},
                {"type": "image", "data": "zzz"},
                {"type": "text", "text": "segunda"}
            ]
        });
        assert_eq!(extraer_texto(&r), "primera\nsegunda");
    }

    #[test]
    fn extraer_texto_fallback_json() {
        let r = json!({"clave": "valor"});
        assert!(extraer_texto(&r).contains("clave"));
    }

    /// [139A-8 F3n/K7] La allowlist acepta runtimes MCP habituales y deniega
    /// shells y utilidades del sistema, vengan pelados, con ruta o con
    /// extensión Windows.
    #[test]
    fn k7_acepta_runtimes_y_deniega_shells() {
        let sin_args: Vec<String> = vec![];
        for bin in [
            "node", "npx", "python", "python3", "uv", "uvx", "bun", "deno",
        ] {
            assert!(
                validar_servidor_mcp_con_extras(bin, &sin_args, &[]).is_ok(),
                "{bin} debería pasar"
            );
        }
        for bin in [
            "sh",
            "bash",
            "cmd",
            "powershell",
            "pwsh",
            "rm",
            "curl",
            r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe",
            "/bin/sh",
            "../node_modules/.bin/malicioso",
        ] {
            assert!(
                validar_servidor_mcp_con_extras(bin, &sin_args, &[]).is_err(),
                "{bin} debería denegarse"
            );
        }
    }

    /// [139A-8 F3n/K7] En Windows la extensión no cuela un binario distinto
    /// (`node.exe` = `node`); fuera de Windows no hay stripping (el FS
    /// distingue: `node.exe` no es `node`).
    #[test]
    fn k7_normaliza_extension_solo_en_windows() {
        let sin_args: Vec<String> = vec![];
        if cfg!(windows) {
            assert!(validar_servidor_mcp_con_extras(r"C:\tools\NODE.EXE", &sin_args, &[]).is_ok());
            assert!(validar_servidor_mcp_con_extras("powershell", &sin_args, &[]).is_err());
        } else {
            assert!(validar_servidor_mcp_con_extras("node", &sin_args, &[]).is_ok());
        }
    }

    /// [139A-8 F3n/K7] `GLORY_MCP_ALLOW` (aquí inyectado) abre la puerta a un
    /// servidor compilado del operador; los topes de argv frenan la
    /// bomba de argumentos.
    #[test]
    fn k7_extras_y_topes_de_argv() {
        let sin_args: Vec<String> = vec![];
        let extras = vec!["mi-mcp-propio".to_string()];
        assert!(validar_servidor_mcp_con_extras("mi-mcp-propio", &sin_args, &[]).is_err());
        assert!(validar_servidor_mcp_con_extras("mi-mcp-propio", &sin_args, &extras).is_ok());
        let muchos: Vec<String> = (0..40).map(|i| format!("arg{i}")).collect();
        assert!(validar_servidor_mcp_con_extras("node", &muchos, &[]).is_err());
        let gordo = vec!["x".repeat(70 * 1024)];
        assert!(validar_servidor_mcp_con_extras("node", &gordo, &[]).is_err());
    }
}
