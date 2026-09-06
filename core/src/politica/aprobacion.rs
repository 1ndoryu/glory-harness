/* [03-09-2026] Canal de aprobación explícito (plan 318A-16, F2): hasta F2 la
 * aprobación era solo conversacional (insignia + "escribe sí"). Aquí cada
 * petición `ask` lleva un `id` y la UI responde por canal con tres vías
 * (opencode `permission.shared.ts`: `permission → [Allow once / Always /
 * Reject]`):
 *
 * - `Aprobar`   → permiso de UNA vez para la clase derivada (token consumido
 *                 en la siguiente llamada igual); no persiste.
 * - `Siempre`   → regla F1 `Allow` para la clase (categoría + patrón): ya no
 *                 vuelve a preguntar para esa *clase* de acción.
 * - `Rechazar`  → regla F1 `Deny` para la clase: la UI no reintenta y la clase
 *                 queda denegada para la conversación (no solo el turno).
 *
 * El estado vive en el registro (Arc compartido con los clones del runtime y
 * los subagentes), mismo patrón que overrides/reglas de F1/F3: la conversación
 * puede responder entre turnos sin reconstruir el runtime.
 */

use crate::permiso::Permiso;
use crate::regla::ReglaPermiso;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Petición de aprobación pendiente (emitida como evento `PeticionAprobacion`
/// junto a la `RequiereAprobacion` de compatibilidad).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeticionAprobacion {
    /// Identificador único de la petición (lo responde `responder_peticion`).
    pub id: String,
    /// Tool propuesta (p. ej. `file_write`).
    pub tool: String,
    /// Argumentos propuestos (para el detalle en la UI).
    pub argumentos: Value,
    /// Clase derivada F1 ("categoria:patrón" o "tool:*"): lo que recordará
    /// "Permitir siempre / Rechazar" sin abrir la tool entera.
    pub clasificacion: String,
}

impl PeticionAprobacion {
    #[must_use]
    pub fn nueva(
        id: impl Into<String>,
        tool: impl Into<String>,
        argumentos: Value,
        clasificacion: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            tool: tool.into(),
            argumentos,
            clasificacion: clasificacion.into(),
        }
    }
}

/// Decisión del usuario sobre una petición pendiente.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RespuestaAprobacion {
    /// Permitir esta llamada una sola vez (sin regla persistente).
    Aprobar,
    /// Denegar esta clase de acción (regla `Deny` por categoría+patrón).
    Rechazar,
    /// Permitir siempre esta clase de acción (regla `Allow` por categoría+patrón).
    Siempre,
}

impl RespuestaAprobacion {
    /// La regla F1 que materializa la respuesta sobre una clase derivada
    /// (categoría + patrón). `Aprobar` no crea regla: usa un token de una vez.
    #[must_use]
    pub fn regla_para(self, categoria: &str, patron: &str) -> Option<ReglaPermiso> {
        match self {
            RespuestaAprobacion::Siempre => {
                Some(ReglaPermiso::nueva(categoria, patron, Permiso::Allow))
            }
            RespuestaAprobacion::Rechazar => {
                Some(ReglaPermiso::nueva(categoria, patron, Permiso::Deny))
            }
            RespuestaAprobacion::Aprobar => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn respuesta_aprobacion_siempre_crea_regla_allow() {
        let regla = RespuestaAprobacion::Siempre.regla_para("escritura", "**");
        let regla = regla.expect("Siempre debe crear regla");
        assert_eq!(regla.categoria, "escritura");
        assert_eq!(regla.accion, Permiso::Allow);
    }

    #[test]
    fn respuesta_aprobacion_rechazar_crea_regla_deny() {
        let regla = RespuestaAprobacion::Rechazar.regla_para("comando", "git *");
        let regla = regla.expect("Rechazar debe crear regla");
        assert_eq!(regla.accion, Permiso::Deny);
    }

    #[test]
    fn respuesta_aprobacion_aprobar_no_crea_regla() {
        assert!(RespuestaAprobacion::Aprobar
            .regla_para("escritura", "**")
            .is_none());
    }
}
