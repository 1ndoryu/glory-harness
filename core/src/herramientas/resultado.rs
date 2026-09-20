//! [139A-8 F4/S1] Resultado de ejecutar una tool (extraído de `tool.rs` sin
//! cambios de semántica): texto legible para el LLM + estado.

/// Resultado de ejecutar una tool: texto legible para el LLM + estado.
#[derive(Debug, Clone)]
pub struct AgentToolResult {
    pub ok: bool,
    pub contenido: String,
    /// Resumen corto para auditoría (sin secretos, sin contenido largo).
    pub resumen: String,
    /// [31-08-2026] Fase 4: diff de líneas del cambio (file_write/file_patch)
    /// para mostrarlo en el front; `None` si no aplica.
    pub diff: Option<String>,
    /// [069A-1 F6] Evento extra que el runtime emite junto a `ToolResult`
    /// (p. ej. `AgenteEvento::ToolNavegador` con captura base64). El front
    /// lo consume para mostrar la imagen.
    pub evento_extra: Option<crate::evento::AgenteEvento>,
    /// [209A-1 F1] Id de ejecución de consola (tool `comando`): el runtime
    /// lo copia al evento `ToolResult` para que la UI paree el resultado con
    /// los eventos `consola_*`. `None` en el resto de tools.
    pub consola_id: Option<String>,
}

impl AgentToolResult {
    #[must_use]
    pub fn ok(contenido: impl Into<String>, resumen: impl Into<String>) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff: None,
            evento_extra: None,
            consola_id: None,
        }
    }

    /// Resultado ok con diff de líneas (para tools que modifican archivos).
    #[must_use]
    pub fn ok_con_diff(
        contenido: impl Into<String>,
        resumen: impl Into<String>,
        diff: Option<String>,
    ) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff,
            evento_extra: None,
            consola_id: None,
        }
    }

    #[must_use]
    pub fn error(contenido: impl Into<String>) -> Self {
        Self {
            ok: false,
            contenido: contenido.into(),
            resumen: "error".to_string(),
            diff: None,
            evento_extra: None,
            consola_id: None,
        }
    }

    /// Constructor con evento_extra (p. ej. captura base64 del navegador).
    #[must_use]
    pub fn ok_con_evento(
        contenido: impl Into<String>,
        resumen: impl Into<String>,
        evento_extra: crate::evento::AgenteEvento,
    ) -> Self {
        Self {
            ok: true,
            contenido: contenido.into(),
            resumen: resumen.into(),
            diff: None,
            evento_extra: Some(evento_extra),
            consola_id: None,
        }
    }
}
