//! [079A-1 F5] Estado y ciclo de vida de la webview hija (partido de navegador.rs).

use tauri::{AppHandle, Url, Window};

pub(super) const MAX_CDP_METHOD: usize = 128;
pub(super) const MAX_CDP_PARAMS: usize = 256 * 1024;
pub(super) const MAX_JAVASCRIPT: usize = 128 * 1024;
pub(super) const MAX_SELECTOR: usize = 4 * 1024;
pub(super) const MAX_VALUE: usize = 64 * 1024;
pub(super) const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
pub(super) const WEBVIEW_OPERATION_TIMEOUT: std::time::Duration =
    std::time::Duration::from_secs(15);

#[cfg(windows)]
type CoreWebView = webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

#[cfg(not(windows))]
type CoreWebView = ();

#[cfg(windows)]
thread_local! {
    /* WebView2/COM no es Send: la interfaz se conserva en el hilo de UI y
     * nunca cruza el estado global de Tauri. Las operaciones que la leen se
     * ejecutan mediante `run_on_main_thread`, manteniendo esta invariante. */
    pub(super) static CORE_WEBVIEW: std::cell::RefCell<Option<CoreWebView>> =
        const { std::cell::RefCell::new(None) };
}

/// Estado mutable de la webview hija.
///
/// El estado global contiene únicamente el handle Tauri, que sí puede ser
/// gestionado por Tauri. La interfaz COM vive en `CORE_WEBVIEW` para respetar
/// su afinidad de hilo.
pub struct EstadoNavegador {
    pub webview: Option<tauri::Webview>,
}

impl EstadoNavegador {
    pub fn new() -> Self {
        Self { webview: None }
    }
}

/// Esquemas URL permitidos para navegación.
pub(super) fn esquema_permitido(url: &str) -> Result<(), String> {
    let parsed = Url::parse(url).map_err(|e| format!("URL inválida: {e}"))?;
    match parsed.scheme() {
        "https" => Ok(()),
        "http" => Ok(()),
        scheme => Err(format!("esquema no permitido: {scheme} (solo https/http)")),
    }
}

/// Crea una webview hija dentro de la ventana principal.
///
/// - Llama `Window::add_child()` (feature `unstable`).
/// - Tamaño por defecto: 800×600, posición (0, 0) relativa a la ventana padre.
/// - Si ya hay una webview abierta, la cierra primero (reemplazo).
///
/// ## Deadlock conocido
///
/// En Windows, `add_child` puede deadlock si se llama desde un comando síncrono
/// (wry#583). Por eso este comando es `async` y la webview se crea en un
/// `tokio::task::spawn_blocking` o directamente en el comando Tauri asíncrono
/// (Tauri maneja el main thread internamente). El comando `navegador_abrir` debe
/// ser `async` (Tauri 2 admite comandos asíncronos con `#[tauri::command]`).
pub(super) async fn crear_webview_hija(
    _app: &AppHandle,
    window: &Window,
    url: &str,
    ancho: u32,
    alto: u32,
    pos_x: i32,
    pos_y: i32,
) -> Result<tauri::Webview, String> {
    esquema_permitido(url)?;

    let parsed_url = Url::parse(url).map_err(|e| format!("URL inválida: {e}"))?;
    let webview_url = tauri::WebviewUrl::External(parsed_url);

    let builder = tauri::webview::WebviewBuilder::new("navegador-interno", webview_url)
        .on_navigation(move |url| {
            /* Solo permitir navegación a https/http (no salir de la política) */
            let scheme = url.scheme();
            scheme == "https" || scheme == "http"
        });

    /* add_child requiere el hilo principal de la ventana; Tauri acepta comandos
     * async porque el dispatch interno se encarga del main thread. */
    let webview = window
        .add_child(
            builder,
            tauri::LogicalPosition::new(f64::from(pos_x), f64::from(pos_y)),
            tauri::LogicalSize::new(ancho as f64, alto as f64),
        )
        .map_err(|e| format!("add_child falló: {e}"))?;

    #[cfg(windows)]
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        webview
            .with_webview(move |platform| {
                let resultado = unsafe { platform.controller().CoreWebView2() }
                    .map(|core| {
                        CORE_WEBVIEW.with(|slot| {
                            *slot.borrow_mut() = Some(core);
                        });
                    })
                    .map_err(|error| format!("CoreWebView2 no disponible: {error}"));
                let _ = tx.send(resultado);
            })
            .map_err(|error| format!("extracción de CoreWebView2 falló: {error}"))?;
        tokio::time::timeout(WEBVIEW_OPERATION_TIMEOUT, rx)
            .await
            .map_err(|_| "extracción de CoreWebView2 agotó el tiempo".to_string())?
            .map_err(|_| "callback de CoreWebView2 cerrado".to_string())??;
    }

    Ok(webview)
}

/// Cierra la webview hija. Si no hay ninguna, es un no-op (no error).
pub(super) fn cerrar_webview(estado: &mut EstadoNavegador) {
    #[cfg(windows)]
    CORE_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
    if let Some(wv) = estado.webview.take() {
        let _ = wv.close();
    }
}

pub(super) fn validar_tamano(nombre: &str, valor: &str, maximo: usize) -> Result<(), String> {
    if valor.is_empty() {
        return Err(format!("{nombre} no puede estar vacío"));
    }
    if valor.len() > maximo {
        return Err(format!("{nombre} supera el límite de {maximo} bytes"));
    }
    Ok(())
}
