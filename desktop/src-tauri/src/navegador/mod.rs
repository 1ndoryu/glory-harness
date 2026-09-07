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

pub(crate) mod comandos;
mod estado;
mod puerto;
mod webview2;

pub use estado::EstadoNavegador;
pub use puerto::NavegadorTauri;
