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
        /* [318A-16 F5] `plan` es el modo de propuesta: mismo comportamiento
         * que `meta` (deny de efectos) EXCEPTO las tools de escritura de
         * archivos, que el runtime deja `allow` para que registren su
         * propuesta en la store del plan en vez de aplicarla. */
        "meta" | "plan" => {
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

/// [318A-16 F5] ¿La tool es de PROPUESTA (modo plan)? En modo plan estas
/// tools quedan `allow` para que registren su diff en la store del plan en
/// vez de aplicarlo; el resto de tools con efecto siguen `deny`.
#[must_use]
pub fn es_tool_propuesta(tool_id: &str) -> bool {
    matches!(tool_id, "file_write" | "file_patch")
}

/// El permiso efectivo de una tool: el override por conversación (si existe)
/// gana al default del modo.
#[must_use]
pub fn permiso_efectivo(default: Permiso, override_conv: Option<Permiso>) -> Permiso {
    override_conv.unwrap_or(default)
}

/* [318A-16 F1] Resolución extendida con reglas v2 (ver `regla.rs`). Orden
 * decidido y testeado (plan: "la regla gana al override explícito solo si es
 * más específica"):
 * 1. `override_conv == Deny` → Deny SIEMPRE (fail-closed): una denegación
 *    explícita de conversación (F3) no se abre con reglas posteriores.
 * 2. Reglas coincidentes (categoría + patrón): la ÚLTIMA decide, deny o
 *    allow (findLast de opencode). La llamada llega ya con las reglas de la
 *    clave MÁS ESPECÍFICA (derivada del argumento), que por diseño gana al
 *    override de tool-entera (menos específico).
 * 3. Si no hay regla: override de conversación; si no, default del modo. */
#[must_use]
pub fn resolver_permiso(
    default: Permiso,
    override_conv: Option<Permiso>,
    reglas: &[crate::regla::ReglaPermiso],
) -> Permiso {
    if override_conv == Some(Permiso::Deny) {
        return Permiso::Deny;
    }
    if let Some(ultima) = reglas.last() {
        return ultima.accion;
    }
    override_conv.unwrap_or(default)
}

/* [059A-21] Veredicto de turno F3 — etapa 2 de la misma decisión de política:
 * `resolver_permiso` (arriba) produce el `Permiso` efectivo y aquí se mapea
 * `Permiso` + "¿ya se preguntó/denegó en este turno?" al veredicto de
 * ejecución. Pura y sin I/O; la emisión de eventos (RequiereAprobacion /
 * PermisoDenegado / resultado de tool) vive en el runtime, que solo consume.
 * Ask y deny no se reintentan en el mismo turno: el repetido vuelve sin
 * re-emitir para que el modelo ya informado no reciba el evento dos veces. */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerdictoPermiso {
    /// `allow`: ejecutar normal.
    Ejecutar,
    /// `ask` (primera vez en el turno): emitir `RequiereAprobacion`.
    Preguntar,
    /// `ask` repetido en el mismo turno: el modelo ya fue informado;
    /// emitir ToolResult sin re-preguntar.
    RepetidoPregunta,
    /// `deny` (primera vez en el turno): emitir `PermisoDenegado`.
    Denegar,
    /// `deny` repetido: la tool ya fue denegada; no re-emitir, solo informar.
    RepetidoDenegado,
}

#[must_use]
pub fn decidir_permiso(permiso: Permiso, ya_denegada: bool) -> VerdictoPermiso {
    match permiso {
        Permiso::Allow => VerdictoPermiso::Ejecutar,
        Permiso::Ask => {
            if ya_denegada {
                VerdictoPermiso::RepetidoPregunta
            } else {
                VerdictoPermiso::Preguntar
            }
        }
        Permiso::Deny => {
            if ya_denegada {
                VerdictoPermiso::RepetidoDenegado
            } else {
                VerdictoPermiso::Denegar
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn veredicto_ask_deny_no_se_reintentan_en_el_mismo_turno() {
        assert_eq!(decidir_permiso(Permiso::Allow, false), VerdictoPermiso::Ejecutar);
        assert_eq!(decidir_permiso(Permiso::Allow, true), VerdictoPermiso::Ejecutar);
        assert_eq!(decidir_permiso(Permiso::Ask, false), VerdictoPermiso::Preguntar);
        assert_eq!(decidir_permiso(Permiso::Ask, true), VerdictoPermiso::RepetidoPregunta);
        assert_eq!(decidir_permiso(Permiso::Deny, false), VerdictoPermiso::Denegar);
        assert_eq!(decidir_permiso(Permiso::Deny, true), VerdictoPermiso::RepetidoDenegado);
    }

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

    /* [318A-16 F5] Modo plan: solo las tools de propuesta (escritura de
     * archivos) quedan allow — para que registren su diff — y el resto de
     * efectos siguen deny (semántica de meta). */
    #[test]
    fn es_tool_propuesta_solo_escritura_de_archivos() {
        assert!(es_tool_propuesta("file_write"));
        assert!(es_tool_propuesta("file_patch"));
        assert!(!es_tool_propuesta("comando"));
        assert!(!es_tool_propuesta("file_read"));
        assert!(!es_tool_propuesta("web_search"));
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

    /* [318A-16 F1] Orden de resolución con reglas v2. */
    use crate::regla::ReglaPermiso;

    #[test]
    fn f1_regla_allow_gana_al_ask_del_modo() {
        /* default ask (efecto en predeterminado) + regla allow de la
         * categoría derivada → la regla (específica) gana. */
        let reglas = vec![ReglaPermiso::nueva("escritura", "src/**", Permiso::Allow)];
        assert_eq!(
            resolver_permiso(Permiso::Ask, None, &reglas),
            Permiso::Allow
        );
    }

    #[test]
    fn f1_regla_deny_mas_especifica_gana_al_override_allow() {
        /* El plan: la regla gana al override explícito solo si es más
         * específica (categoría derivada + patrón > tool entera). */
        let reglas = vec![ReglaPermiso::nueva("escritura_fuera_repo", "*", Permiso::Deny)];
        assert_eq!(
            resolver_permiso(Permiso::Ask, Some(Permiso::Allow), &reglas),
            Permiso::Deny
        );
    }

    #[test]
    fn f1_override_deny_fail_closed_gana_a_toda_regla() {
        /* Una denegación explícita de conversación no se abre con reglas. */
        let reglas = vec![ReglaPermiso::nueva("escritura", "*", Permiso::Allow)];
        assert_eq!(
            resolver_permiso(Permiso::Ask, Some(Permiso::Deny), &reglas),
            Permiso::Deny
        );
    }

    #[test]
    fn f1_sin_reglas_vuelve_override_y_default() {
        assert_eq!(
            resolver_permiso(Permiso::Ask, Some(Permiso::Allow), &[]),
            Permiso::Allow
        );
        assert_eq!(resolver_permiso(Permiso::Ask, None, &[]), Permiso::Ask);
    }
}
