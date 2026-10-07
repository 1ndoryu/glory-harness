//! Comandos CRUD de conversaciones del desktop.
//!
//! [Partición limite-lineas/god-object] Los comandos viven por dominio:
//! - `crud.rs`: crear/listar/cargar/renombrar/archivar/eliminar (+ proyecto)
//!   con `CargaConversacion` y `panel_poner_conversacion`.
//! - `consolas.rs`: `consola_matar/listar/salida/escribir/crear`.
//! - `rewind.rs`: `rewind_conversacion`, `restaurar_archivos_tramo`,
//!   `cambios_archivo`, `rechazar_cambio`.
//! Este fichero conserva los helpers de panel (los usan `turno.rs`,
//! `sesion.rs` y `main.rs`) y re-exporta los comandos para no cambiar las
//! rutas `conversaciones::…` registradas en `main.rs`.

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

pub(crate) use super::consolas::{
    consola_crear, consola_escribir, consola_matar, consola_salida, consolas_listar,
};
pub(crate) use super::crud::{
    archivar_conversacion, archivar_conversaciones_proyecto, cargar_conversacion,
    conversacion_nueva, eliminar_conversacion, eliminar_conversaciones_proyecto,
    listar_conversaciones, renombrar_conversacion,
};
pub(crate) use super::rewind::{
    cambios_archivo, rechazar_cambio, restaurar_archivos_tramo, rewind_conversacion,
};

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
