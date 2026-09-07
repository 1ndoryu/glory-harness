//! [079A-1 F5] Backend WebView2/COM (partido de navegador.rs).

#[cfg(windows)]
use super::estado::{EstadoNavegador, CORE_WEBVIEW, MAX_CAPTURE_BYTES, WEBVIEW_OPERATION_TIMEOUT};

#[cfg(windows)]
mod win {
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

    use super::{EstadoNavegador, CORE_WEBVIEW, MAX_CAPTURE_BYTES, WEBVIEW_OPERATION_TIMEOUT};
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
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
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
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
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
                Err(e) => {
                    let _ = tx.send(Err(e));
                    return;
                }
            };
            let resultado = (|| {
                let slot: ResultadoCompartido<String> = Arc::new(std::sync::Mutex::new(None));
                let slot_cb = slot.clone();
                let espera = CallDevToolsProtocolMethodCompletedHandler::wait_for_async_operation(
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

#[cfg(windows)]
pub(super) use win::{capturar, cdp, ejecutar_script};

#[cfg(not(windows))]
mod win {
    use tauri::AppHandle;

    pub async fn capturar(_app: AppHandle) -> Result<String, String> {
        Err("captura solo está implementada en Windows".to_string())
    }

    pub async fn ejecutar_script(_app: AppHandle, _codigo: String) -> Result<String, String> {
        Err("JavaScript solo está implementado en Windows".to_string())
    }

    pub async fn cdp(_app: AppHandle, _metodo: String, _params: String) -> Result<String, String> {
        Err("CDP solo está implementado en Windows".to_string())
    }
}

#[cfg(not(windows))]
pub(super) use win::{capturar, cdp, ejecutar_script};
