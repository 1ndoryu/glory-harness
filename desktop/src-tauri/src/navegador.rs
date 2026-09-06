//! Módulo de navegador interno: child webview nativo vía Tauri 2 `Window::add_child()`.
//!
//! ## Spike F1 (069A-1.1)
//!
//! Este módulo implementa el spike mínimo para verificar la API real:
//! - Crear una webview hija con `Window::add_child()` (feature `unstable`)
//! - Navegar a una URL
//! - Redimensionar
//! - Cerrar/destruir
//!
//! ## Referencias Tauri 2.11.5
//!
//! - `Window::add_child()` requiere `feature = "unstable"` y está disponible
//!   solo en desktop (Windows/Mac/Linux).
//! - `Webview::close()` libera la webview hija (no hay `destroy()` en child).
//! - `Webview::navigate()` recibe `Url` parseada.
//! - `Webview::with_webview()` da acceso a `PlatformWebview` (nativo COM).
//!
//! ## Seguridad
//!
//! - Solo se permiten esquemas `https:` y `http:` (para fixtures locales).
//! - Se rechazan `javascript:`, `data:`, `file:`.
//! - La URL se valida con `Url::parse()` y comprobación de esquema.
//! - No se expone el `PlatformWebview` fuera de este módulo.
//!
//! ## F2: Corrección de runtime (069A-1, 2026-09-06)
//!
//! `with_webview()` despacha una closure al event loop como
//! `Message::Webview(webview_id, WebviewMessage::WithWebview(...))`. El
//! handler busca el webview en `window.webviews.iter().find(|w| w.id ==
//! webview_id)`. Cuando una child webview se cierra y se vuelve a crear
//! (mismo label), el registro interno de la ventana se limpia y la nueva
//! child obtiene un `id` distinto al que `with_webview` capturó del
//! `DetachedWebview`. La closure nunca se ejecuta y el `oneshot` se cierra
//! silenciosamente → "callback... cerrado".
//!
//! Solución: extraer `ICoreWebView2` una sola vez durante `navegador_abrir`
//! (dentro de la llamada a `with_webview`, que funciona en la primera
//! creación porque el `id` está fresco). Almacenar la interfaz COM en
//! `EstadoNavegador.core`. Las operaciones COM posteriores usan
//! `run_on_main_thread` (que despacha `Message::Task`, sin pasar por el
//! registro de webviews) y toman `ICoreWebView2` directamente del estado.
//!
//! ## F5: NavegadorPort (069A-1, 2026-09-08)
//!
//! `NavegadorTauri` implementa el trait del núcleo y se inyecta al runtime
//! como puerto. Cada método usa `AppHandle` para despachar al hilo principal
//! via `run_on_main_thread` (COM) o acceder a `EstadoNavegador` (Tauri API).

use async_trait::async_trait;
use glory_harness_core::error::Error as HarnessError;
use glory_harness_core::error::Result as CoreResult;
use glory_harness_core::ports::NavegadorPort;
use std::sync::Arc;
use tauri::{AppHandle, Manager, Url, Window};

const MAX_CDP_METHOD: usize = 128;
const MAX_CDP_PARAMS: usize = 256 * 1024;
const MAX_JAVASCRIPT: usize = 128 * 1024;
const MAX_SELECTOR: usize = 4 * 1024;
const MAX_VALUE: usize = 64 * 1024;
const MAX_CAPTURE_BYTES: usize = 16 * 1024 * 1024;
const WEBVIEW_OPERATION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[cfg(windows)]
type CoreWebView = webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2;

#[cfg(not(windows))]
type CoreWebView = ();

#[cfg(windows)]
thread_local! {
    /* WebView2/COM no es Send: la interfaz se conserva en el hilo de UI y
     * nunca cruza el estado global de Tauri. Las operaciones que la leen se
     * ejecutan mediante `run_on_main_thread`, manteniendo esta invariante. */
    static CORE_WEBVIEW: std::cell::RefCell<Option<CoreWebView>> =
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
fn esquema_permitido(url: &str) -> Result<(), String> {
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
pub async fn crear_webview_hija(
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
pub fn cerrar_webview(estado: &mut EstadoNavegador) {
    #[cfg(windows)]
    CORE_WEBVIEW.with(|slot| *slot.borrow_mut() = None);
    if let Some(wv) = estado.webview.take() {
        let _ = wv.close();
    }
}

fn validar_tamano(nombre: &str, valor: &str, maximo: usize) -> Result<(), String> {
    if valor.is_empty() {
        return Err(format!("{nombre} no puede estar vacío"));
    }
    if valor.len() > maximo {
        return Err(format!("{nombre} supera el límite de {maximo} bytes"));
    }
    Ok(())
}

// --- IPC commands ---

/// Abre el navegador interno: crea una webview hija y navega a la URL dada.
///
/// ## Parámetros
/// - `url`: URL a navegar (solo https/http).
/// - `ancho` (opcional): ancho en px. Default 800.
/// - `alto` (opcional): alto en px. Default 600.
/// - `pos_x` (opcional): posición X en px. Default 0.
/// - `pos_y` (opcional): posición Y en px. Default 0.
#[tauri::command]
pub async fn navegador_abrir(
    app: AppHandle,
    url: String,
    ancho: Option<u32>,
    alto: Option<u32>,
    pos_x: Option<i32>,
    pos_y: Option<i32>,
) -> Result<(), String> {
    let window = app
        .get_window("main")
        .ok_or_else(|| "ventana main no encontrada".to_string())?;

    let estado_lock = app
        .try_state::<std::sync::Mutex<EstadoNavegador>>()
        .ok_or_else(|| "navegador no disponible (estado no encontrado)".to_string())?;

    /* El label de una child webview es único. Cerrar la anterior antes de
     * crear la nueva evita que add_child falle por duplicidad y hace explícito
     * que el reemplazo libera el recurso anterior incluso si la creación falla. */
    {
        let mut estado = estado_lock
            .lock()
            .map_err(|_| "estado bloqueado".to_string())?;
        cerrar_webview(&mut estado);
    }

    let wv = crear_webview_hija(
        &app,
        &window,
        &url,
        ancho.unwrap_or(800),
        alto.unwrap_or(600),
        pos_x.unwrap_or(0),
        pos_y.unwrap_or(0),
    )
    .await?;

    let mut estado = estado_lock
        .lock()
        .map_err(|_| "estado bloqueado".to_string())?;
    estado.webview = Some(wv);

    Ok(())
}

/// Navega la webview hija a una URL.
#[tauri::command]
pub async fn navegador_navegar(app: AppHandle, url: String) -> Result<(), String> {
    esquema_permitido(&url)?;
    let parsed = Url::parse(&url).map_err(|e| format!("URL inválida: {e}"))?;

    let estado_lock = app
        .try_state::<std::sync::Mutex<EstadoNavegador>>()
        .ok_or_else(|| "navegador no disponible (estado no encontrado)".to_string())?;
    let estado = estado_lock
        .lock()
        .map_err(|_| "estado bloqueado".to_string())?;
    let wv = estado
        .webview
        .as_ref()
        .ok_or_else(|| "navegador no abierto".to_string())?;

    wv.navigate(parsed)
        .map_err(|e| format!("navegación falló: {e}"))?;

    Ok(())
}

/// Cierra la webview hija.
#[tauri::command]
pub async fn navegador_cerrar(app: AppHandle) -> Result<(), String> {
    let estado_lock = app
        .try_state::<std::sync::Mutex<EstadoNavegador>>()
        .ok_or_else(|| "navegador no disponible".to_string())?;
    let mut estado = estado_lock
        .lock()
        .map_err(|_| "estado bloqueado".to_string())?;
    cerrar_webview(&mut estado);
    Ok(())
}

/// Reposiciona la webview hija dentro de la ventana.
#[tauri::command]
pub async fn navegador_posicionar(
    app: AppHandle,
    x: i32,
    y: i32,
    ancho: u32,
    alto: u32,
) -> Result<(), String> {
    let estado_lock = app
        .try_state::<std::sync::Mutex<EstadoNavegador>>()
        .ok_or_else(|| "navegador no disponible".to_string())?;
    let estado = estado_lock
        .lock()
        .map_err(|_| "estado bloqueado".to_string())?;
    let wv = estado
        .webview
        .as_ref()
        .ok_or_else(|| "navegador no abierto".to_string())?;

    wv.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
        f64::from(x),
        f64::from(y),
    )))
    .map_err(|e| format!("posicionar falló: {e}"))?;

    wv.set_size(tauri::Size::Logical(tauri::LogicalSize::new(
        f64::from(ancho),
        f64::from(alto),
    )))
    .map_err(|e| format!("redimensionar falló: {e}"))
}

#[cfg(windows)]
mod webview2 {
    //! Operaciones COM sincronizadas dentro de `run_on_main_thread`.
    //!
    //! `run_on_main_thread` despacha una closure al event loop de Tauri vía
    //! `Message::Task`, que no depende del registro interno de la webview hija
    //! en la ventana. Esto evita el fallo determinista de `with_webview` en
    //! child WebViews tras cerrar y reabrir (la lista `window.webviews` se
    //! limpia al cerrar y `with_webview` busca por webview_id en esa lista).
    //!
    //! Dentro de la closure se obtiene `ICoreWebView2` desde el estado del
    //! módulo (extraído una vez al crear la child), no desde el
    //! `PlatformWebview` de `with_webview`.

use super::{
        EstadoNavegador, CORE_WEBVIEW, MAX_CAPTURE_BYTES, WEBVIEW_OPERATION_TIMEOUT,
    };
    use base64::Engine;
    use std::sync::Arc;
    use tauri::{AppHandle, Manager};
    use tokio::sync::oneshot;
    use webview2_com::{
        CallDevToolsProtocolMethodCompletedHandler, CapturePreviewCompletedHandler, CoTaskMemPWSTR,
        ExecuteScriptCompletedHandler,
        Microsoft::Web::WebView2::Win32::{
            ICoreWebView2, COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
        },
    };
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::Com::{
        IStream, StructuredStorage::CreateStreamOnHGlobal, STREAM_SEEK_END, STREAM_SEEK_SET,
    };

    type Resultado<T> = Result<T, String>;
    type ResultadoCompartido<T> = Arc<std::sync::Mutex<Option<Resultado<T>>>>;

    fn guardar<T>(slot: &ResultadoCompartido<T>, resultado: Resultado<T>) {
        match slot.lock() {
            Ok(mut guard) => *guard = Some(resultado),
            Err(poisoned) => *poisoned.into_inner() = Some(resultado),
        }
    }

    fn tomar<T>(slot: ResultadoCompartido<T>) -> Resultado<T> {
        match slot.lock() {
            Ok(mut guard) => guard
                .take()
                .unwrap_or_else(|| Err("operación WebView2 no devolvió resultado".to_string())),
            Err(poisoned) => poisoned
                .into_inner()
                .take()
                .unwrap_or_else(|| Err("operación WebView2 no devolvió resultado".to_string())),
        }
    }

    fn leer_stream(stream: &IStream) -> Resultado<Vec<u8>> {
        let mut longitud = 0_u64;
        unsafe {
            stream
                .Seek(0, STREAM_SEEK_END, Some(&mut longitud))
                .map_err(|error| format!("no se pudo medir la captura: {error}"))?;
            stream
                .Seek(0, STREAM_SEEK_SET, None)
                .map_err(|error| format!("no se pudo rebobinar la captura: {error}"))?;
        }
        let longitud: usize = longitud
            .try_into()
            .map_err(|_| "la captura supera el límite de memoria".to_string())?;
        if longitud > MAX_CAPTURE_BYTES {
            return Err(format!("la captura supera {MAX_CAPTURE_BYTES} bytes"));
        }
        let mut resultado = Vec::with_capacity(longitud);
        let mut buffer = [0_u8; 8192];
        loop {
            let mut leidos = 0_u32;
            unsafe {
                stream
                    .Read(
                        buffer.as_mut_ptr().cast(),
                        buffer.len() as u32,
                        Some(&mut leidos),
                    )
                    .ok()
                    .map_err(|error| format!("no se pudo leer la captura: {error}"))?;
            }
            if leidos == 0 {
                break;
            }
            resultado.extend_from_slice(&buffer[..leidos as usize]);
            if resultado.len() > MAX_CAPTURE_BYTES {
                return Err(format!("la captura supera {MAX_CAPTURE_BYTES} bytes"));
            }
        }
        Ok(resultado)
    }

    fn core_desde_hilo() -> Resultado<ICoreWebView2> {
        CORE_WEBVIEW.with(|slot| {
            slot.borrow()
                .clone()
                .ok_or_else(|| "navegador no abierto (sin core WebView2)".to_string())
        })
    }

    fn webview_handle(app: &AppHandle) -> Resultado<tauri::Webview> {
        let estado_lock = app
            .try_state::<std::sync::Mutex<EstadoNavegador>>()
            .ok_or_else(|| "navegador no disponible (estado no encontrado)".to_string())?;
        let estado = estado_lock
            .lock()
            .map_err(|_| "estado bloqueado".to_string())?;
        estado
            .webview
            .as_ref()
            .cloned()
            .ok_or_else(|| "navegador no abierto".to_string())
    }

    pub async fn capturar(app: AppHandle) -> Resultado<String> {
        let wv = webview_handle(&app)?;
        let (tx, rx) = oneshot::channel();
        wv.run_on_main_thread(move || {
            let core = match core_desde_hilo() {
                Ok(c) => c,
                Err(e) => { let _ = tx.send(Err(e)); return; }
            };
            let resultado = (|| {
                let stream = unsafe { CreateStreamOnHGlobal(HGLOBAL::default(), true) }
                    .map_err(|error| format!("no se pudo crear el stream: {error}"))?;
                let slot: ResultadoCompartido<String> = Arc::new(std::sync::Mutex::new(None));
                let slot_cb = slot.clone();
                let stream_cb = stream.clone();
                let espera = CapturePreviewCompletedHandler::wait_for_async_operation(
                    Box::new(move |handler| unsafe {
                        core.CapturePreview(
                            COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                            &stream,
                            &handler,
                        )
                        .map_err(webview2_com::Error::WindowsError)
                    }),
                    Box::new(move |status| {
                        let resultado = match status {
                            Ok(()) => leer_stream(&stream_cb).map(|bytes| {
                                base64::engine::general_purpose::STANDARD.encode(bytes)
                            }),
                            Err(error) => Err(format!("CapturePreview falló: {error}")),
                        };
                        guardar(&slot_cb, resultado);
                        Ok(())
                    }),
                );
                if let Err(error) = espera {
                    guardar(&slot, Err(format!("CapturePreview no inició: {error}")));
                }
                Ok(tomar(slot))
            })();
            let _ = tx.send(resultado.flatten());
        })
        .map_err(|error| format!("run_on_main_thread falló: {error}"))?;
        tokio::time::timeout(WEBVIEW_OPERATION_TIMEOUT, rx)
            .await
            .map_err(|_| "captura agotó el tiempo".to_string())?
            .map_err(|_| "callback de captura cerrado".to_string())?
    }

    pub async fn ejecutar_script(app: AppHandle, codigo: String) -> Resultado<String> {
        let wv = webview_handle(&app)?;
        let (tx, rx) = oneshot::channel();
        wv.run_on_main_thread(move || {
            let core = match core_desde_hilo() {
                Ok(c) => c,
                Err(e) => { let _ = tx.send(Err(e)); return; }
            };
            let resultado = (|| {
                let slot: ResultadoCompartido<String> = Arc::new(std::sync::Mutex::new(None));
                let slot_cb = slot.clone();
                let espera = ExecuteScriptCompletedHandler::wait_for_async_operation(
                    Box::new(move |handler| unsafe {
                        let js = CoTaskMemPWSTR::from(codigo.as_str());
                        core.ExecuteScript(*js.as_ref().as_pcwstr(), &handler)
                            .map_err(webview2_com::Error::WindowsError)
                    }),
                    Box::new(move |status, resultado| {
                        guardar(
                            &slot_cb,
                            status
                                .map(|_| resultado)
                                .map_err(|error| format!("ExecuteScript falló: {error}")),
                        );
                        Ok(())
                    }),
                );
                if let Err(error) = espera {
                    guardar(&slot, Err(format!("ExecuteScript no inició: {error}")));
                }
                Ok(tomar(slot))
            })();
            let _ = tx.send(resultado.flatten());
        })
        .map_err(|error| format!("run_on_main_thread falló: {error}"))?;
        tokio::time::timeout(WEBVIEW_OPERATION_TIMEOUT, rx)
            .await
            .map_err(|_| "JavaScript agotó el tiempo".to_string())?
            .map_err(|_| "callback de JavaScript cerrado".to_string())?
    }

    pub async fn cdp(app: AppHandle, metodo: String, params: String) -> Resultado<String> {
        let wv = webview_handle(&app)?;
        let (tx, rx) = oneshot::channel();
        wv.run_on_main_thread(move || {
            let core = match core_desde_hilo() {
                Ok(c) => c,
                Err(e) => { let _ = tx.send(Err(e)); return; }
            };
            let resultado = (|| {
                let slot: ResultadoCompartido<String> = Arc::new(std::sync::Mutex::new(None));
                let slot_cb = slot.clone();
                let espera =
                    CallDevToolsProtocolMethodCompletedHandler::wait_for_async_operation(
                        Box::new(move |handler| unsafe {
                            let m = CoTaskMemPWSTR::from(metodo.as_str());
                            let p = CoTaskMemPWSTR::from(params.as_str());
                            core.CallDevToolsProtocolMethod(
                                *m.as_ref().as_pcwstr(),
                                *p.as_ref().as_pcwstr(),
                                &handler,
                            )
                            .map_err(webview2_com::Error::WindowsError)
                        }),
                        Box::new(move |status, resultado| {
                            guardar(
                                &slot_cb,
                                status
                                    .map(|_| resultado)
                                    .map_err(|error| format!("CDP falló: {error}")),
                            );
                            Ok(())
                        }),
                    );
                if let Err(error) = espera {
                    guardar(&slot, Err(format!("CDP no inició: {error}")));
                }
                Ok(tomar(slot))
            })();
            let _ = tx.send(resultado.flatten());
        })
        .map_err(|error| format!("run_on_main_thread falló: {error}"))?;
        tokio::time::timeout(WEBVIEW_OPERATION_TIMEOUT, rx)
            .await
            .map_err(|_| "CDP agotó el tiempo".to_string())?
            .map_err(|_| "callback CDP cerrado".to_string())?
    }
}

#[cfg(not(windows))]
mod webview2 {
    use tauri::AppHandle;

    pub async fn capturar(_app: AppHandle) -> Result<String, String> {
        Err("captura solo está implementada en Windows".to_string())
    }

    pub async fn ejecutar_script(_app: AppHandle, _codigo: String) -> Result<String, String> {
        Err("JavaScript solo está implementado en Windows".to_string())
    }

    pub async fn cdp(
        _app: AppHandle,
        _metodo: String,
        _params: String,
    ) -> Result<String, String> {
        Err("CDP solo está implementado en Windows".to_string())
    }
}

/// Captura PNG nativa de la webview hija y devuelve Base64.
#[tauri::command]
pub async fn navegador_capturar(app: AppHandle) -> Result<String, String> {
    webview2::capturar(app).await
}

/// Ejecuta JavaScript en la webview hija.
#[tauri::command]
pub async fn navegador_js(app: AppHandle, codigo: String) -> Result<String, String> {
    validar_tamano("código JavaScript", &codigo, MAX_JAVASCRIPT)?;
    webview2::ejecutar_script(app, codigo).await
}

/// Invoca un método CDP con parámetros JSON.
#[tauri::command]
pub async fn navegador_cdp(
    app: AppHandle,
    metodo: String,
    parametros: String,
) -> Result<String, String> {
    validar_tamano("método CDP", &metodo, MAX_CDP_METHOD)?;
    validar_tamano("parámetros CDP", &parametros, MAX_CDP_PARAMS)?;
    serde_json::from_str::<serde_json::Value>(&parametros)
        .map_err(|error| format!("parámetros CDP inválidos: {error}"))?;
    webview2::cdp(app, metodo, parametros).await
}

/// Hace click sobre el primer elemento que coincida con el selector CSS.
#[tauri::command]
pub async fn navegador_click(app: AppHandle, selector: String) -> Result<String, String> {
    validar_tamano("selector", &selector, MAX_SELECTOR)?;
    let sel_esc = serde_json::to_string(&selector).map_err(|error| error.to_string())?;
    let codigo = format!(
        "(()=>{{const e=document.querySelector({sel_esc});if(!e)return {{ok:false,error:'elemento no encontrado'}};e.click();return{{ok:true}};}})()"
    );
    navegador_js(app, codigo).await
}

/// Rellena un control y emite eventos de input/change.
#[tauri::command]
pub async fn navegador_rellenar(
    app: AppHandle,
    selector: String,
    valor: String,
) -> Result<String, String> {
    validar_tamano("selector", &selector, MAX_SELECTOR)?;
    validar_tamano("valor", &valor, MAX_VALUE)?;
    let sel_esc = serde_json::to_string(&selector).map_err(|error| error.to_string())?;
    let val_esc = serde_json::to_string(&valor).map_err(|error| error.to_string())?;
    let codigo = format!(
        "(()=>{{const e=document.querySelector({sel_esc});if(!e)return {{ok:false,error:'elemento no encontrado'}};e.focus();e.value={val_esc};e.dispatchEvent(new Event('input',{{bubbles:true}}));e.dispatchEvent(new Event('change',{{bubbles:true}}));return{{ok:true}};}})()"
    );
    navegador_js(app, codigo).await
}

/// Devuelve el texto visible de la página (snapshot de accesibilidad).
#[tauri::command]
pub async fn navegador_snapshot(app: AppHandle) -> Result<String, String> {
    navegador_js(
        app,
        "(()=>{const t=document.body?.innerText??'';return t.slice(0,262144);})()".to_string(),
    )
    .await
}

// ---------------------------------------------------------------------------
// NavegadorPort (F5): implementación del trait del núcleo
// ---------------------------------------------------------------------------

/// Adaptador Tauri del puerto `NavegadorPort`.
///
/// Cada método delega al `AppHandle` para despachar al hilo principal y
/// consulta el estado global del navegador (webview + COM). Como Tauri exige
/// que todo acceso a la webview viva en el hilo de UI, los métodos COM usan
/// `run_on_main_thread` vía las funciones del submódulo `webview2`.
///
/// ## Afinidad de hilo
///
/// `Abir` y `cerrar` usan `crear_webview_hija` / `EstadoNavegador.webview`
/// directamente (async Tauri). Las operaciones COM delegadas
/// (`capturar`, `js`, `cdp`) se resuelven internamente en `webview2::*`,
/// que ya toman `AppHandle`. `click`, `rellenar`, `snapshot` y `navegar`
/// se implementan sobre `navegador_js` / `wv.navigate`, ambos seguros para
/// cross-thread porque usan `AppHandle` + `run_on_main_thread` o el comando
/// async directo.
///
/// El `AppHandle` se clona al crear el struct (es un Arc interno) y todas
/// las operaciones reciben `&self`: el handle se conserva inmutablemente.
#[derive(Clone)]
pub struct NavegadorTauri {
    app: AppHandle,
}

impl NavegadorTauri {
    pub fn nuevo(app: &AppHandle) -> Self {
        Self { app: app.clone() }
    }
}

#[async_trait]
impl NavegadorPort for NavegadorTauri {
    async fn abrir(&self, url: &str) -> CoreResult<()> {
        navegador_abrir(self.app.clone(), url.to_string(), None, None, None, None)
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_abrir: {e}")))
    }

    async fn navegar(&self, url: &str) -> CoreResult<()> {
        navegador_navegar(self.app.clone(), url.to_string())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_navegar: {e}")))
    }

    async fn capturar(&self) -> CoreResult<String> {
        navegador_capturar(self.app.clone())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_capturar: {e}")))
    }

    async fn js(&self, codigo: &str) -> CoreResult<String> {
        navegador_js(self.app.clone(), codigo.to_string())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_js: {e}")))
    }

    async fn cdp(&self, metodo: &str, parametros: &str) -> CoreResult<String> {
        navegador_cdp(
            self.app.clone(),
            metodo.to_string(),
            parametros.to_string(),
        )
        .await
        .map_err(|e| HarnessError::Interno(format!("navegador_cdp: {e}")))
    }

    async fn click(&self, selector: &str) -> CoreResult<()> {
        navegador_click(self.app.clone(), selector.to_string())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_click: {e}")))?;
        Ok(())
    }

    async fn rellenar(&self, selector: &str, valor: &str) -> CoreResult<()> {
        navegador_rellenar(self.app.clone(), selector.to_string(), valor.to_string())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_rellenar: {e}")))?;
        Ok(())
    }

    async fn snapshot(&self, _selector: &str) -> CoreResult<String> {
        /* [069A-1 F5] navegador_snapshot captura texto visible de toda la
         * página. El parámetro `selector` se ignora en esta implementación
         * de escritorio. */
        navegador_snapshot(self.app.clone())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_snapshot: {e}")))
    }

    async fn cerrar(&self) -> CoreResult<()> {
        navegador_cerrar(self.app.clone())
            .await
            .map_err(|e| HarnessError::Interno(format!("navegador_cerrar: {e}")))
    }
}
