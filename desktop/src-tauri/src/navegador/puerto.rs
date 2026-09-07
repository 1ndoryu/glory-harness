//! [079A-1 F5] Adaptador NavegadorPort sobre Tauri (partido de navegador.rs).

use async_trait::async_trait;
use glory_harness_core::error::Error as HarnessError;
use glory_harness_core::error::Result as CoreResult;
use glory_harness_core::ports::NavegadorPort;
use tauri::AppHandle;

use super::comandos::{
    navegador_abrir, navegador_capturar, navegador_cdp, navegador_cerrar, navegador_click,
    navegador_js, navegador_navegar, navegador_rellenar, navegador_snapshot,
};

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
        navegador_cdp(self.app.clone(), metodo.to_string(), parametros.to_string())
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
