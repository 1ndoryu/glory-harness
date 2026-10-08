//! Comandos CRUD de conversaciones del desktop.
//!
//! [Partición limite-lineas/god-object] Los comandos viven por dominio:
//! Comandos por dominio: `crud.rs` (crear/listar/cargar/renombrar/archivar/
//! eliminar, con `CargaConversacion` y `panel_poner_conversacion`),
//! `consolas.rs` (`consola_matar/listar/salida/escribir/crear`) y `rewind.rs`
//! (`rewind_conversacion`, `restaurar_archivos_tramo`, `cambios_archivo`,
//! `rechazar_cambio`). Este fichero conserva los helpers de panel (los usan
//! `turno.rs`, `sesion.rs`, `crud.rs`, `rewind.rs` y `main.rs`).
//!
//! [08AA-2] Sin re-exports de comandos: `generate_handler!` resuelve
//! `__cmd__*` en el módulo donde vive `#[tauri::command]` (`crud`, `rewind`,
//! `consolas`); re-exportar la fn no arrastra el wrapper (E0603). El handler
//! de `main.rs` usa las rutas de definición; los nombres de comando del
//! front no cambian.

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

/// [069A-7] Conversación actual de un panel. `Ok(None)` = el panel está en
/// borrador (sin conversación creada todavía); `Err` = panel inexistente.
/* [109A-6] `pub(crate)`: antes el módulo colgaba de la raíz y `pub(super)` ya
alcanzaba todo el crate; sigue usándose desde `main.rs` y `sesion.rs`. */
pub(crate) fn conv_id_de_panel(sesion: &Sesion, panel_id: &str) -> Result<Option<Uuid>, String> {
    sesion
        .paneles
        .lock()
        .map(|p| {
            p.get(panel_id)
                .map(|d| d.conversacion_id)
                .ok_or_else(|| format!("panel no encontrado: {panel_id}"))
        })
        .map_err(|_| "sesión bloqueada por otro turno".to_string())?
}

/// [069A-7] Id de conversación de un panel que DEBE tener una (los turnos
/// requieren conversación: el front crea antes de enviar). Devuelve error
/// claro si el panel está en borrador.
// [109A-6] `pub(crate)`: se usa desde `chat/turno.rs` y `sesion.rs`.
pub(crate) fn conv_id_de_panel_obligatoria(
    sesion: &Sesion,
    panel_id: &str,
) -> Result<Uuid, String> {
    conv_id_de_panel(sesion, panel_id)?.ok_or_else(|| {
        "no hay conversación en este panel: escribe el primer mensaje para crearla".to_string()
    })
}
