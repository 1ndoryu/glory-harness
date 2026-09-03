/* [03-09-2026] Modelo de permisos por tool (plan 318A-15, F3):
 * `Permiso { ask | allow | deny }` con herencia de un default por perfil/modo
 * y overrides por conversación (patrón opencode permissions — reglas con
 * acción allow/deny/ask evaluadas en orden, última coincidencia gana — y
 * claurst `PermissionLevel`).
 *
 * Semántica (contrato F3):
 * - `ask`    → la tool se ofrece al modelo pero su ejecución emite
 *              `RequiereAprobacion` y se omite hasta confirmación del usuario.
 * - `allow`  → se ejecuta sin preguntar.
 * - `deny`   → se QUITA del schema (el modelo no la ve; no solo policy) y, si
 *              por cualquier vía llega a proponerse, se deniega con el evento
 *              `PermisoDenegado` (fail-closed).
 *
 * Mapeo desde los modos actuales (default, override por conversación
 * explícito; configuración existente sin romper — un modo desconocido cae a
 * predeterminado):
 * - `predeterminado` → ask para tools con efecto, allow para las demás.
 * - `meta`           → deny para tools con efecto, allow para las demás.
 * - `autonomo`       → allow para todo.
 *
 * F4 consumirá este modelo tal cual: el runtime del subagente resuelve la
 * policy de su perfil con los mismos `Permiso` y hereda los overrides.
 */

/// Decisión de permiso por tool: ask | allow | deny.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Permiso {
    /// Preguntar al usuario antes de ejecutar (evento `RequiereAprobacion`).
    Ask,
    /// Ejecutar sin preguntar.
    Allow,
    /// Bloquear: la tool se retira del schema y, si se propone, se deniega.
    Deny,
}

/// Default del modo actual para una tool según tenga efecto o no
/// (invariante: un modo desconocido se trata como `predeterminado`, nunca se
/// abre un permiso por un typo).
#[must_use]
pub fn permiso_por_modo(modo: &str, efecto: bool) -> Permiso {
    match modo {
        "autonomo" => Permiso::Allow,
        "meta" => {
            if efecto {
                Permiso::Deny
            } else {
                Permiso::Allow
            }
        }
        /* predeterminado y cualquier valor desconocido. */
        _ => {
            if efecto {
                Permiso::Ask
            } else {
                Permiso::Allow
            }
        }
    }
}

/// El permiso efectivo de una tool: el override por conversación (si existe)
/// gana al default del modo.
#[must_use]
pub fn permiso_efectivo(default: Permiso, override_conv: Option<Permiso>) -> Permiso {
    override_conv.unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modo_predeterminado_pregunta_por_efectos_y_permite_lectura() {
        assert_eq!(permiso_por_modo("predeterminado", true), Permiso::Ask);
        assert_eq!(permiso_por_modo("predeterminado", false), Permiso::Allow);
    }

    #[test]
    fn modo_meta_deniega_todo_efecto() {
        assert_eq!(permiso_por_modo("meta", true), Permiso::Deny);
        assert_eq!(permiso_por_modo("meta", false), Permiso::Allow);
    }

    #[test]
    fn modo_autonomo_permite_sin_preguntar() {
        assert_eq!(permiso_por_modo("autonomo", true), Permiso::Allow);
        assert_eq!(permiso_por_modo("autonomo", false), Permiso::Allow);
    }

    #[test]
    fn modo_desconocido_cae_a_predeterminado_fail_closed() {
        assert_eq!(permiso_por_modo("modo-inexistente", true), Permiso::Ask);
        assert_eq!(permiso_por_modo("modo-inexistente", false), Permiso::Allow);
    }

    #[test]
    fn override_de_conversacion_gana_al_default() {
        assert_eq!(
            permiso_efectivo(Permiso::Ask, Some(Permiso::Allow)),
            Permiso::Allow
        );
        assert_eq!(
            permiso_efectivo(Permiso::Allow, Some(Permiso::Deny)),
            Permiso::Deny
        );
        assert_eq!(permiso_efectivo(Permiso::Deny, None), Permiso::Deny);
    }
}
