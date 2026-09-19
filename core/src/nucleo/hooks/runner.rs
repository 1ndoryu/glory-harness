// [Bloque 3, F2] Ejecucion de hooks: SalidaHook, RunnerHook y RunnerComandoHttp.
// K4 (allowlist) y K6 (egreso) viven aqui; sin tipos de contrato ni despacho.
use crate::entorno::aplicar_entorno_minimo;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::IpAddr;
use std::time::Duration;
use super::tipos::{EventoHook, Hook, TIMEOUT_HOOK_DEFAULT, TipoHook, validar_comando_hook};

/// Resultado de ejecutar un hook.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SalidaHook {
    /// `true` = el hook pide bloquear la acción (exit code 2 del comando).
    pub bloqueo: bool,
    /// Ajuste opcional devuelto como JSON. Solo `PreCompact` lo consume;
    /// los demás eventos lo ignoran para no ampliar su contrato accidentalmente.
    pub ajuste: Option<Value>,
}

impl SalidaHook {
    pub const CONTINUAR: Self = Self {
        bloqueo: false,
        ajuste: None,
    };
    pub const BLOQUEAR: Self = Self {
        bloqueo: true,
        ajuste: None,
    };

    #[must_use]
    pub fn con_ajuste(ajuste: Value) -> Self {
        Self {
            bloqueo: false,
            ajuste: Some(ajuste),
        }
    }
}

/// [Bloque 3, F4] Seam de ejecución: el dispatcher no conoce procesos ni
/// HTTP. Los tests inyectan un runner que graba; producción usa
/// [`RunnerComandoHttp`].
#[async_trait]
pub trait RunnerHook: Send + Sync {
    /// Ejecuta el hook con el payload ya serializable. `Err` = fallo del
    /// runner (se registra y el turno continúa; jamás aborta).
    async fn correr(&self, hook: &Hook, payload: &Value)
        -> std::result::Result<SalidaHook, String>;
}

/// Runner de producción: `command` → proceso local con el JSON en stdin y
/// timeout (exit 0/1 = continúa, exit 2 = bloquea); `http` → POST JSON
/// (informativo: 2xx = continúa, el resto se registra sin bloquear).
pub struct RunnerComandoHttp {
    /// Timeout aplicado cuando el hook no trae el suyo.
    pub timeout: Duration,
    /// [139A-8 F3n/K6] Hosts extra permitidos aunque caigan en rangos
    /// denegados (intranet del operador, `localhost` en dev…). Vacío por
    /// defecto (fail-closed); bajo responsabilidad del operador.
    pub egreso_extra: Vec<String>,
}

impl Default for RunnerComandoHttp {
    fn default() -> Self {
        Self {
            timeout: TIMEOUT_HOOK_DEFAULT,
            egreso_extra: Vec::new(),
        }
    }
}

const MAX_SALIDA_HOOK: usize = 64 * 1024;

fn timeout_efectivo(configurado: Duration, defecto: Duration) -> Duration {
    let timeout = if configurado.is_zero() { defecto } else { configurado };
    if timeout.is_zero() {
        TIMEOUT_HOOK_DEFAULT
    } else {
        timeout
    }
}

async fn limpiar_proceso_y_lector(
    child: &mut tokio::process::Child,
    lector: &mut tokio::task::JoinHandle<std::result::Result<Vec<u8>, String>>,
) {
    /* `kill` + `wait` evita dejar el proceso directo vivo. En Windows no
     * garantiza por sí solo la terminación de descendientes creados por el
     * hook; el comando se mantiene sin shell para reducir esa superficie y
     * esta limitación queda cubierta por la política/documentación del hook. */
    let _ = child.kill().await;
    let _ = child.wait().await;
    lector.abort();
    let _ = lector.await;
}

async fn ejecutar_comando_local(
    comando: &str,
    args: &[String],
    payload: &Value,
    timeout: Duration,
) -> std::result::Result<(std::process::ExitStatus, Vec<u8>), String> {
    let cuerpo = serde_json::to_string(payload).map_err(|e| e.to_string())?;
    let inicio = tokio::time::Instant::now();
    // [139A-8 F3n/K2] El hijo del hook NO hereda el entorno del operador
    // (claves LLM): solo el subconjunto mínimo.
    let mut spawn = tokio::process::Command::new(comando);
    spawn
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    aplicar_entorno_minimo(&mut spawn);
    let mut child = spawn.spawn().map_err(|e| format!("no se pudo lanzar '{comando}': {e}"))?;
    let stdout = match child.stdout.take() {
        Some(stdout) => stdout,
        None => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(format!("'{comando}' no expuso stdout"));
        }
    };
    /* El lector empieza antes de escribir stdin: un hook que produce salida o
     * espera el cierre de stdin no bloquea el pipe. El mismo presupuesto cubre
     * escritura, espera y drenaje; los errores limpian proceso y lector. */
    let mut lector = tokio::spawn(async move {
        use tokio::io::AsyncReadExt;
        let mut salida = Vec::new();
        stdout
            .take((MAX_SALIDA_HOOK + 1) as u64)
            .read_to_end(&mut salida)
            .await
            .map(|_| salida)
            .map_err(|e| format!("no se pudo leer stdout de hook: {e}"))
    });

    let resultado = tokio::time::timeout(
        timeout.saturating_sub(inicio.elapsed()),
        async {
            if let Some(mut stdin) = child.stdin.take() {
                use tokio::io::AsyncWriteExt;
                stdin.write_all(cuerpo.as_bytes()).await.map_err(|e| {
                    format!("no se pudo escribir el payload a '{comando}': {e}")
                })?;
                /* Cerrar stdin es parte del contrato: permite que el hook
                 * detecte EOF y termine sin esperar otro mensaje. */
            }
            let estado = child
                .wait()
                .await
                .map_err(|e| format!("'{comando}' falló: {e}"))?;
            let salida = (&mut lector)
                .await
                .map_err(|e| format!("lector de hook abortado: {e}"))??;
            Ok::<_, String>((estado, salida))
        },
    )
    .await;

    match resultado {
        Ok(Ok(resultado)) => Ok(resultado),
        Ok(Err(error)) => {
            limpiar_proceso_y_lector(&mut child, &mut lector).await;
            Err(error)
        }
        Err(_) => {
            limpiar_proceso_y_lector(&mut child, &mut lector).await;
            Err(format!(
                "'{comando}' excedió el timeout de {}ms",
                timeout.as_millis()
            ))
        }
    }
}

fn interpretar_salida_comando(
    evento: EventoHook,
    comando: &str,
    estado: std::process::ExitStatus,
    salida: Vec<u8>,
) -> std::result::Result<SalidaHook, String> {
    if salida.len() > MAX_SALIDA_HOOK {
        return Err(format!(
            "'{comando}' excedió el límite de stdout de {MAX_SALIDA_HOOK} bytes"
        ));
    }
    let codigo = estado.code();
    let mut resultado = if codigo == Some(2) {
        SalidaHook::BLOQUEAR
    } else {
        if codigo != Some(0) {
            tracing::warn!(
                comando = %comando,
                codigo = ?codigo,
                "hook terminó con error no bloqueante; se continúa"
            );
        }
        SalidaHook::CONTINUAR
    };
    if evento == EventoHook::PreCompact
        && !salida.is_empty()
        && !salida.iter().all(u8::is_ascii_whitespace)
    {
        let ajuste: Value = serde_json::from_slice(&salida)
            .map_err(|e| format!("stdout de '{comando}' no es JSON válido: {e}"))?;
        resultado.ajuste = Some(validar_ajuste_precompact(ajuste, comando)?);
    }
    Ok(resultado)
}

fn destino_http_seguro(url: &str) -> String {
    let Ok(destino) = reqwest::Url::parse(url) else {
        return "<url de hook inválida>".into();
    };
    let host = destino.host_str().unwrap_or("<host>");
    let host = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let puerto = destino
        .port()
        .map(|puerto| format!(":{puerto}"))
        .unwrap_or_default();
    format!("{}://{host}{puerto}", destino.scheme())
}

/// [139A-8 F3n/K6] Hostnames que jamás son egreso legítimo de un hook
/// (metadata cloud, resolución local). Comparación exacta en minúsculas;
/// los subdominios de metadata también caen (p. ej. `x.metadata.google.internal`).
fn host_bloqueado_por_nombre(host: &str) -> bool {
    const BLOQUEADOS: &[&str] = &[
        "localhost",
        "metadata.google.internal",
        "instance-data",
        "instance-data-compute",
        "wpad",
    ];
    BLOQUEADOS
        .iter()
        .any(|b| host == *b || host.ends_with(&format!(".{b}")))
        || host.ends_with(".localhost")
        || host.ends_with(".internal")
}

/// [139A-8 F3n/K6] `true` = IP a la que un hook http JAMÁS debe POSTear
/// (loopback, privadas, link-local, metadata `169.254.169.254`, multicast,
/// sin-especificar, documentación, broadcast). Las IPv4-mapeadas (`::ffff:a.b.c.d`,
/// forma habitual de escribir un bypass) se juzgan por su IPv4 interior.
fn ip_egreso_denegada(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_multicast()
                || v4.is_unspecified()
                || v4.is_documentation()
                // 169.254.169.254 (metadata cloud) es link-local: ya cae
                // arriba; se deja explícito por legibilidad del invariante.
                || v4.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(v6) => {
            if let Some(mapeada) = v6.to_ipv4_mapped() {
                return ip_egreso_denegada(IpAddr::V4(mapeada));
            }
            v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unspecified()
                || v6.is_unicast_link_local()
                || v6.is_unique_local()
                || ((v6.segments()[0] & 0xff00) == 0x2000 && v6.segments()[1] == 0x0db8)
        }
    }
}

/// Resuelve el host a IPs (bloqueante → `spawn_blocking`, patrón R1/R2).
/// Fallar al resolver = denegar (fail-closed): un hook no debe POSTear a
/// lo que no se puede auditar. Residual documentado: TOCTOU de DNS
/// (re-resolución entre el check y el POST); el riesgo restante exige
/// controlar el DNS del operador.
async fn ips_resueltas(host: &str, puerto: u16) -> Result<Vec<IpAddr>, String> {
    let objetivo = format!("{host}:{puerto}");
    let host_auditable = host.to_string();
    tokio::task::spawn_blocking(move || {
        use std::net::ToSocketAddrs;
        objetivo
            .to_socket_addrs()
            .map(|addrs| addrs.map(|a| a.ip()).collect())
            .map_err(|e| format!("no se pudo resolver '{host_auditable}': {e}"))
    })
    .await
    .map_err(|e| format!("resolución DNS interrumpida: {e}"))?
}

/// [139A-8 F3n/K6] Valida el egreso de un hook http ANTES del POST.
/// Deniega: esquema no http/https, IP literal en rango denegado, hostname
/// bloqueado, o cualquier IP resuelta en rango denegado. `extra` (hosts
/// exactos, case-insensitive) exime: vía de escape explícita del operador.
pub async fn validar_egreso_http(url: &str, extra: &[String]) -> Result<(), String> {
    let destino =
        reqwest::Url::parse(url).map_err(|_| "hook http denegado: URL inválida".to_string())?;
    if destino.scheme() != "http" && destino.scheme() != "https" {
        return Err(format!(
            "hook http denegado ('{}'): solo http/https",
            destino.scheme()
        ));
    }
    let host = destino.host_str().unwrap_or_default().to_lowercase();
    if host.is_empty() {
        return Err("hook http denegado: sin host".to_string());
    }
    if extra.iter().any(|e| e.to_lowercase() == host) {
        return Ok(());
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if ip_egreso_denegada(ip) {
            return Err(format!(
                "hook http denegado ('{host}'): IP en rango prohibido (SSRF)"
            ));
        }
        return Ok(());
    }
    if host_bloqueado_por_nombre(&host) {
        return Err(format!(
            "hook http denegado ('{host}'): hostname reservado (SSRF)"
        ));
    }
    let puerto = destino
        .port()
        .unwrap_or(if destino.scheme() == "https" { 443 } else { 80 });
    for ip in ips_resueltas(&host, puerto).await? {
        if ip_egreso_denegada(ip) {
            return Err(format!(
                "hook http denegado ('{host}'): resuelve a IP prohibida {ip} (SSRF)"
            ));
        }
    }
    Ok(())
}

async fn ejecutar_http(
    url: &str,
    payload: &Value,
    timeout: Duration,
) -> std::result::Result<SalidaHook, String> {
    let cliente = reqwest::Client::new();
    let destino = destino_http_seguro(url);
    match tokio::time::timeout(timeout, cliente.post(url).json(payload).send()).await {
        Ok(Ok(respuesta)) => {
            let status = respuesta.status();
            if !status.is_success() {
                tracing::warn!(destino = %destino, status = %status, "hook http respondió no-2xx (se continúa)");
            }
            Ok(SalidaHook::CONTINUAR)
        }
        Ok(Err(_)) => Err("POST del hook HTTP falló".into()),
        Err(_) => Err(format!(
            "POST del hook HTTP excedió el timeout de {}s",
            timeout.as_secs()
        )),
    }
}

fn validar_ajuste_precompact(valor: Value, comando: &str) -> std::result::Result<Value, String> {
    let objeto = valor
        .as_object()
        .ok_or_else(|| format!("stdout de '{comando}' debe ser un objeto JSON"))?;
    let mut ajuste = serde_json::Map::new();
    for (clave, valor) in objeto {
        if clave != "resumen_llm" && clave != "resumen" {
            return Err(format!(
                "stdout de '{comando}' contiene la clave no permitida '{clave}'"
            ));
        }
        let texto = valor.as_str().map(str::trim).filter(|texto| !texto.is_empty()).ok_or_else(|| {
            format!("stdout de '{comando}' requiere que '{clave}' sea texto no vacío")
        })?;
        ajuste.insert(clave.clone(), Value::String(texto.to_owned()));
    }
    if ajuste.is_empty() {
        return Err(format!("stdout de '{comando}' no contiene un ajuste reconocido"));
    }
    Ok(Value::Object(ajuste))
}

#[async_trait]
impl RunnerHook for RunnerComandoHttp {
    async fn correr(
        &self,
        hook: &Hook,
        payload: &Value,
    ) -> std::result::Result<SalidaHook, String> {
        let timeout = timeout_efectivo(hook.timeout, self.timeout);
        match &hook.tipo {
            TipoHook::Comando { comando, args } => {
                // [139A-8 F3n/K4] Los externos pasan la allowlist; los
                // internos (código) ya vienen fijados. Denegar = no lanzar
                // (el dispatcher registra el error y el turno continúa).
                if !hook.interno {
                    validar_comando_hook(comando, args).map_err(|motivo| {
                        format!("hook '{}' bloqueado por política K4: {motivo}", hook.nombre)
                    })?;
                }
                let (estado, salida) = ejecutar_comando_local(comando, args, payload, timeout).await?;
                interpretar_salida_comando(hook.evento, comando, estado, salida)
            }
            TipoHook::Http { url } => {
                // [139A-8 F3n/K6] Sin egreso a red interna/metadata.
                validar_egreso_http(url, &self.egreso_extra)
                    .await
                    .map_err(|motivo| {
                        format!("hook '{}' bloqueado por política K6: {motivo}", hook.nombre)
                    })?;
                ejecutar_http(url, payload, timeout).await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::{DispatcherHooks, EventoHook, Hook};
    use std::time::Duration;

    #[test]
    fn salida_con_ajuste_no_bloquea_por_defecto() {
        let salida = SalidaHook::con_ajuste(serde_json::json!({"resumen_llm": "ok"}));
        assert!(!salida.bloqueo);
        assert!(salida.ajuste.is_some());
    }
    fn comando_pwsh(script: &str) -> Hook {
        // [139A-8 F3n/K4] Hook de SISTEMA (args fijados en código): interno,
        // sin allowlist. Un `Hook::comando` externo con powershell se deniega.
        Hook::comando_interno(
            "proceso-real",
            EventoHook::PreCompact,
            "powershell.exe",
            vec!["-NoProfile".into(), "-Command".into(), script.into()],
        )
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn runner_real_lee_payload_y_acepta_ajuste() {
        let hook = comando_pwsh(
            "$input | ConvertFrom-Json | Out-Null; Write-Output '{\"resumen\":\"ajuste real\"}'",
        )
        .con_timeout(Duration::from_secs(5));
        let salida = RunnerComandoHttp::default()
            .correr(&hook, &serde_json::json!({"evento":"PreCompact"}))
            .await
            .expect("el proceso real debe terminar");
        assert_eq!(
            salida.ajuste,
            Some(serde_json::json!({"resumen":"ajuste real"}))
        );
        assert!(!salida.bloqueo);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn runner_real_exit_1_continua_y_se_observa() {
        let hook = comando_pwsh("$input | Out-Null; exit 1").con_timeout(Duration::from_secs(5));
        let salida = RunnerComandoHttp::default()
            .correr(&hook, &serde_json::json!({}))
            .await
            .expect("exit 1 sigue siendo no bloqueante");
        assert!(!salida.bloqueo);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn runner_real_exit_2_veta() {
        let hook = comando_pwsh("$input | Out-Null; exit 2").con_timeout(Duration::from_secs(5));
        let salida = RunnerComandoHttp::default()
            .correr(&hook, &serde_json::json!({}))
            .await
            .expect("exit 2 sigue siendo una salida válida");
        assert!(salida.bloqueo);
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn runner_real_timeout_subsegundo_no_muestra_cero() {
        let hook = comando_pwsh("Start-Sleep -Milliseconds 500").con_timeout(Duration::from_millis(25));
        let error = RunnerComandoHttp::default()
            .correr(&hook, &serde_json::json!({}))
            .await
            .expect_err("debe aplicar el timeout pequeño");
        assert!(error.contains("25ms"), "mensaje inesperado: {error}");
    }

    #[cfg(windows)]
    #[tokio::test]
    async fn runner_real_rechaza_stdout_invalido_sin_bloquear_dispatcher() {
        let hook = comando_pwsh("$input | Out-Null; Write-Output texto").con_timeout(Duration::from_secs(5));
        let mut dispatcher = DispatcherHooks::vacia();
        dispatcher.registrar(hook);
        let salida = dispatcher
            .disparar_con_salida(EventoHook::PreCompact, serde_json::json!({}))
            .await;
        assert!(!salida.bloqueo);
        assert!(salida.ajuste.is_none());
    }

    #[test]
    fn ajuste_precompact_rechaza_claves_y_texto_vacio() {
        assert!(validar_ajuste_precompact(serde_json::json!([]), "x").is_err());
        assert!(validar_ajuste_precompact(serde_json::json!({"otro":"x"}), "x").is_err());
        assert!(validar_ajuste_precompact(serde_json::json!({"resumen":" "}), "x").is_err());
        assert_eq!(
            validar_ajuste_precompact(serde_json::json!({"resumen":"  ok  "}), "x").unwrap(),
            serde_json::json!({"resumen":"ok"})
        );
    }

    #[test]
    fn destino_http_no_expone_credenciales_ni_ruta() {
        let destino = destino_http_seguro(
            "https://usuario:secreto@example.test:8443/hooks/pre?token=privado#fragmento",
        );
        assert_eq!(destino, "https://example.test:8443");
        assert!(!destino.contains("secreto"));
        assert!(!destino.contains("privado"));
        assert!(!destino.contains("/hooks"));
    }

    /// runner (fail-closed: error K4, el turno continúa) mientras el gemelo
    /// INTERNO (código) sí corre. Solo Windows (necesita proceso real).
    #[cfg(windows)]
    #[tokio::test]
    async fn k4_runner_bloquea_externo_powershell_y_permite_interno() {
        let externo = Hook::comando(
            "externo-malicioso",
            EventoHook::Stop,
            "powershell.exe",
            vec!["-NoProfile".into(), "-Command".into(), "exit 0".into()],
        );
        let error = RunnerComandoHttp::default()
            .correr(&externo, &serde_json::json!({}))
            .await
            .expect_err("el externo con powershell debe bloquearse");
        assert!(error.contains("K4"), "mensaje inesperado: {error}");
        let interno =
            comando_pwsh("$input | Out-Null; exit 0").con_timeout(Duration::from_secs(10));
        RunnerComandoHttp::default()
            .correr(&interno, &serde_json::json!({}))
            .await
            .expect("el interno de sistema debe correr");
    }
    /// `::1`, IPv4-mapeadas, hostnames reservados y esquemas no-http se
    /// deniegan SIN tocar la red (literales o nombre bloqueado).
    #[tokio::test]
    async fn k6_deniega_egreso_interno_y_esquemas_raros() {
        let extra: Vec<String> = vec![];
        for url in [
            "http://localhost:8080/hook",
            "http://LOCALHOST/hook",
            "http://127.0.0.1/hook",
            "http://10.0.0.5/hook",
            "http://192.168.1.1/hook",
            "http://172.16.0.1/hook",
            "http://169.254.169.254/latest/meta-data/",
            "http://[::1]/hook",
            "http://[::ffff:127.0.0.1]/hook",
            "http://metadata.google.internal/hook",
            "http://wpad/hook",
            "ftp://example.com/hook",
            "file:///etc/passwd",
        ] {
            assert!(
                validar_egreso_http(url, &extra).await.is_err(),
                "{url} debería denegarse"
            );
        }
    }

    /// [139A-8 F3n/K6] Una IP pública literal pasa (sin DNS); el allowlist
    /// del operador (`egreso_extra`) exime un host interno exacto.
    #[tokio::test]
    async fn k6_permite_ip_publica_y_extra_exime_exacta() {
        let extra: Vec<String> = vec![];
        assert!(
            validar_egreso_http("http://8.8.8.8/hook", &extra)
                .await
                .is_ok(),
            "IP pública literal debería pasar"
        );
        assert!(
            validar_egreso_http("https://1.1.1.1:8443/hook", &extra)
                .await
                .is_ok(),
            "IP pública con puerto debería pasar"
        );
        let local = vec!["localhost".to_string()];
        assert!(
            validar_egreso_http("http://localhost:8080/hook", &local)
                .await
                .is_ok(),
            "el extra exacto exime"
        );
        assert!(
            validar_egreso_http("http://127.0.0.1/hook", &local)
                .await
                .is_err(),
            "el extra es por nombre exacto, no abre el 127.0.0.1"
        );
    }
}
