//! Notificación de escritorio del CLI ([069A-3], alcance mínimo): toast de
//! Windows al terminar un turno (`Stop`) o al pedir un permiso
//! (`PermissionRequest`), solo cuando el operador pasa `--notificar`.
//!
//! Diseño: el runtime ya emite ambos eventos por `DispatcherHooks`
//! (`turno/mod.rs` con `{"turno_id","resumen"}`, `permisos.rs` con
//! `{"tool","tool_input"}`); aquí solo se registra un dispatcher con dos
//! `Hook::comando` que invocan a `powershell.exe` con el script embebido
//! ([`SCRIPT_TOAST`]) en `-EncodedCommand` (base64 UTF-16LE, sin quoting
//! frágil) y el payload JSON por stdin. Sin dependencias nuevas (build
//! offline): el base64 se implementa a mano ([`base64_encode`]).
//!
//! Límites documentados: el toast necesita el shell de Windows (sin sesión
//! gráfica no se muestra, pero el hook nunca falla el turno); sin atajo en
//! el menú Inicio el aviso es transitorio (no queda en el Centro de
//! actividades). El script siempre sale con 0: jamás bloquea (ni siquiera
//! `PermissionRequest`, que sí admite veto por exit 2).

use std::sync::Arc;
use std::time::Duration;

use glory_harness_core::hooks::{DispatcherHooks, EventoHook, Hook};
use glory_harness_core::runtime::AgentRuntime;

/// [069A-3] Script PowerShell del toast: lee el payload JSON de stdin,
/// distingue el evento por forma (`tool` presente = permiso pendiente, si no
/// fin de turno con `resumen`) y muestra un toast WinRT. Todo dentro de
/// try/catch + `exit 0` final: un error nunca sale con 2 (no veta nada) ni
/// aborta el turno. Solo comillas simples (el `-EncodedCommand` evita el
/// quoting de la línea de comandos).
pub const SCRIPT_TOAST: &str = r#"$ErrorActionPreference = 'SilentlyContinue'
try {
  $raw = [Console]::In.ReadToEnd()
  if ([string]::IsNullOrWhiteSpace($raw)) { exit 0 }
  $p = $raw | ConvertFrom-Json
  if ($null -eq $p) { exit 0 }
  if ($null -ne $p.tool) {
    $titulo = 'Glory Harness: permiso pendiente'
    $texto = [string]$p.tool
  } else {
    $titulo = 'Glory Harness: turno terminado'
    $texto = [string]$p.resumen
    if ([string]::IsNullOrWhiteSpace($texto)) { $texto = 'El agente termino de responder.' }
  }
  if ($texto.Length -gt 140) { $texto = $texto.Substring(0, 140) + '...' }
  $t1 = [System.Security.SecurityElement]::Escape($titulo)
  $t2 = [System.Security.SecurityElement]::Escape($texto)
  $xml = '<toast><visual><binding template=''ToastGeneric''><text>' + $t1 + '</text><text>' + $t2 + '</text></binding></visual></toast>'
  $doc = New-Object Windows.Data.Xml.Dom.XmlDocument
  $doc.LoadXml($xml)
  $avisador = [Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime]::CreateToastNotifier('GloryHarness.CLI')
  $avisador.Show([Windows.UI.Notifications.ToastNotification, Windows.UI.Notifications, ContentType = WindowsRuntime]::new($doc))
} catch { }
exit 0"#;

/// [069A-3] Timeout propio de los hooks de aviso: el arranque frío de
/// `powershell.exe` ronda 1-2s; 20s deja margen sin retener el turno (el
/// `Stop` es informativo y el runner mata el proceso al expirar).
pub const TIMEOUT_AVISO: Duration = Duration::from_secs(20);

/// Base64 estándar (alfabeto `+/`, relleno `=`), puro y testeable. Existe
/// solo porque el build es offline y ninguna dependencia del CLI lo expone;
/// se usa para el `-EncodedCommand` de PowerShell (que exige UTF-16LE).
fn base64_encode(datos: &[u8]) -> String {
    const TABLA: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut salida = String::with_capacity(datos.len().div_ceil(3) * 4);
    for trozo in datos.chunks(3) {
        let mut n: u32 = 0;
        for (i, &b) in trozo.iter().enumerate() {
            n |= (b as u32) << (16 - 8 * i);
        }
        let relleno = 3 - trozo.len();
        for i in 0..4 - relleno {
            let indice = ((n >> (18 - 6 * i)) & 0x3F) as usize;
            salida.push(TABLA[indice] as char);
        }
        for _ in 0..relleno {
            salida.push('=');
        }
    }
    salida
}

/// Codifica el script a UTF-16LE + base64 para `-EncodedCommand` (formato
/// documentado de `powershell.exe`: sin esto, las comillas del XML romperían
/// la línea de comandos).
fn script_codificado() -> String {
    let utf16: Vec<u8> = SCRIPT_TOAST
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    base64_encode(&utf16)
}

/// [069A-3] Dispatcher con los dos avisos (fin de turno + permiso
/// pendiente) vía `powershell.exe`. El runner por defecto
/// (`RunnerComandoHttp`) ejecuta el comando con el payload en stdin.
#[must_use]
pub fn dispatcher_notificacion() -> DispatcherHooks {
    let args = vec![
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-EncodedCommand".to_string(),
        script_codificado(),
    ];
    let mut dispatcher = DispatcherHooks::vacia();
    dispatcher.registrar(
        Hook::comando(
            "toast-stop",
            EventoHook::Stop,
            "powershell.exe",
            args.clone(),
        )
        .con_timeout(TIMEOUT_AVISO),
    );
    dispatcher.registrar(
        Hook::comando(
            "toast-permiso",
            EventoHook::PermissionRequest,
            "powershell.exe",
            args,
        )
        .con_timeout(TIMEOUT_AVISO),
    );
    dispatcher
}

/// [069A-3] Activa los avisos en el runtime si el operador pasó
/// `--notificar`. Sin flag no toca nada (emisión no-op como antes). Solo el
/// CLI interactivo (`run`/`chat`/`tui`/`session resume`) la llama: daemon,
/// `schedule run` y desktop quedan fuera (sin usuario ante la consola).
pub fn aplicar_notificacion(runtime: &Arc<AgentRuntime>, notificar: bool) {
    if notificar {
        runtime.set_hooks(dispatcher_notificacion());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_vector_conocido() {
        assert_eq!(base64_encode(b"hola"), "aG9sYQ==");
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"AB"), "QUI=");
    }

    #[test]
    fn base64_utf16le_vector_powershell() {
        // "AB" en UTF-16LE = 41 00 42 00 → QQBCAA== (formato -EncodedCommand).
        let utf16: Vec<u8> = "AB".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
        assert_eq!(base64_encode(&utf16), "QQBCAA==");
    }

    #[test]
    fn dispatcher_registra_stop_y_permiso_en_powershell() {
        let d = dispatcher_notificacion();
        assert_eq!(d.hooks().len(), 2);
        let eventos: Vec<EventoHook> = d.hooks().iter().map(|h| h.evento).collect();
        assert!(eventos.contains(&EventoHook::Stop));
        assert!(eventos.contains(&EventoHook::PermissionRequest));
        for h in d.hooks() {
            match &h.tipo {
                glory_harness_core::hooks::TipoHook::Comando { comando, args } => {
                    assert_eq!(comando, "powershell.exe");
                    assert!(args.contains(&"-EncodedCommand".to_string()));
                }
                otro => panic!("hook {otro:?} debería ser comando"),
            }
            assert_eq!(h.timeout, TIMEOUT_AVISO);
            assert!(h.tool_patron.is_none());
        }
    }

    #[test]
    fn script_cubre_ambos_payloads_y_nunca_veta() {
        // Contrato de forma con los emisores (turno/mod.rs, permisos.rs).
        assert!(SCRIPT_TOAST.contains("tool"));
        assert!(SCRIPT_TOAST.contains("resumen"));
        assert!(SCRIPT_TOAST.contains("ToastNotificationManager"));
        assert!(SCRIPT_TOAST.contains("exit 0"));
        assert!(!SCRIPT_TOAST.contains("exit 2"));
    }
}
