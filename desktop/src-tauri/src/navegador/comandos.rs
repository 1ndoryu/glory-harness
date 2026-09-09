//! [079A-1 F5] Comandos IPC del navegador (partidos de navegador.rs).

use tauri::{AppHandle, Manager, Url};

use super::estado::{
    cerrar_webview, crear_webview_hija, esquema_permitido, validar_tamano, EstadoNavegador,
    MAX_CDP_METHOD, MAX_CDP_PARAMS, MAX_JAVASCRIPT, MAX_SELECTOR, MAX_VALUE,
};
use super::webview2;

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

/// Muestra u oculta la webview hija sin destruirla (para las tabs del panel
/// derecho). Ocultar la mueve fuera de la pantalla (1×1 en -10000); al
/// mostrar, el frontend la reposiciona con `navegador_posicionar`.
#[tauri::command]
pub async fn navegador_mostrar(app: AppHandle, visible: bool) -> Result<(), String> {
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

    if !visible {
        wv.set_position(tauri::Position::Logical(tauri::LogicalPosition::new(
            -10000.0, -10000.0,
        )))
        .map_err(|e| format!("ocultar falló: {e}"))?;
        wv.set_size(tauri::Size::Logical(tauri::LogicalSize::new(1.0, 1.0)))
            .map_err(|e| format!("ocultar falló: {e}"))?;
    }
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
