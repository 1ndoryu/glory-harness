/* [04-09-2026] Guardas baratas de turno (Bloque 3, Fase 1): respuesta vacía y
 * repetición. Evidencia: hermes-agent `empty_response_guard.py` /
 * `repetition_guard.py`. Módulo PURO y determinista (sin I/O, sin SQL): el
 * runtime las aplica en la finalización del turno y los tests no necesitan
 * proveedor ni red.
 *
 * - Respuesta vacía → reintento único con aviso (el aviso se inyecta como
 *   mensaje de sistema antes de la llamada de reintento; quien decide el
 *   reintento es el runtime, aquí solo se define el texto).
 * - Repetición → detector por ventana: compara la respuesta nueva contra las
 *   últimas respuestas del asistente normalizadas; si supera el umbral, el
 *   runtime debe avisar (y opcionalmente abortar la respuesta repetida). */

use serde::{Deserialize, Serialize};

/// Configuración de guardas de un turno.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GuardasTurno {
    /// Guardas activas. Falso desactiva todo (compatibilidad total).
    pub habilitadas: bool,
    /// Respuesta vacía → permitir un reintento con aviso.
    pub reintento_vacio: bool,
    /// Máximo de respuestas casi idénticas consecutivas toleradas (0 = off).
    pub umbral_repeticion: usize,
}

impl Default for GuardasTurno {
    fn default() -> Self {
        Self {
            habilitadas: true,
            reintento_vacio: true,
            umbral_repeticion: 2,
        }
    }
}

/// ¿El texto del asistente quedó vacío (solo blancos)?
#[must_use]
pub fn texto_vacio(texto: &str) -> bool {
    texto.trim().is_empty()
}

/// Normaliza un texto para comparar repeticiones: minúsculas y blancos
/// colapsados (ignora diferencias de formato/acento de mayúsculas, no de
/// contenido).
#[must_use]
pub fn normalizar(texto: &str) -> String {
    texto
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Cuenta cuántas de las últimas respuestas del asistente son casi idénticas
/// a la nueva (comparación normalizada). Solo mira la cola — una respuesta
/// antigua igual no cuenta como bucle activo.
#[must_use]
pub fn veces_repetida(nueva: &str, anteriores: &[String]) -> usize {
    let nueva = normalizar(nueva);
    if nueva.is_empty() {
        return 0;
    }
    anteriores
        .iter()
        .rev()
        .take_while(|anterior| normalizar(anterior) == nueva)
        .count()
}

/// Aviso de sistema inyectado antes del reintento tras una respuesta vacía.
#[must_use]
pub fn aviso_vacio() -> &'static str {
    "Tu respuesta anterior quedó vacía. Responde concretamente a la petición \
     del usuario: si no tienes certeza, dilo y propón el siguiente paso. No \
     repitas herramientas que ya ejecutaste."
}

/// Aviso cuando la respuesta repite casi verbatim respuestas anteriores.
#[must_use]
pub fn aviso_repeticion(n: usize) -> String {
    format!(
        "Estás repitiendo casi verbatim la respuesta anterior ({n} veces \
         seguidas). El usuario ya la vio. Reformula con contenido nuevo: \
         estado real, siguiente acción concreta o pregunta de decisión. Si no \
         hay nada nuevo que decir, cierra con el siguiente paso."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vacio_por_blancos() {
        assert!(texto_vacio(""));
        assert!(texto_vacio("   \n\t "));
        assert!(!texto_vacio("ok"));
    }

    #[test]
    fn normalizacion_colapsa_y_minusculiza() {
        assert_eq!(normalizar("  Hola   MUNDO\n"), "hola mundo");
        assert_eq!(normalizar("Hola mundo"), normalizar("  hola  MUNDO "));
    }

    #[test]
    fn repeticion_cuenta_solo_la_cola() {
        let viejas = vec![
            "respuesta vieja".to_string(),
            "igual que nueva".to_string(),
            "igual que nueva".to_string(),
        ];
        // Dos seguidas al final → 2 (la vieja intermedia corta el take_while
        // porque recorre desde el final hacia atrás: ["igual","igual","vieja"]).
        assert_eq!(veces_repetida("igual que nueva", &viejas), 2);
    }

    #[test]
    fn repeticion_ignora_distintas_y_vacias() {
        let viejas = vec!["una cosa".to_string(), "otra cosa".to_string()];
        assert_eq!(veces_repetida("tercera cosa", &viejas), 0);
        assert_eq!(veces_repetida("   ", &viejas), 0);
        assert_eq!(veces_repetida("", &[]), 0);
    }

    #[test]
    fn repeticion_una_sola() {
        let viejas = vec!["mismo texto".to_string()];
        assert_eq!(veces_repetida("Mismo texto", &viejas), 1);
    }

    #[test]
    fn default_activo_con_umbral_2() {
        let g = GuardasTurno::default();
        assert!(g.habilitadas && g.reintento_vacio);
        assert_eq!(g.umbral_repeticion, 2);
    }
}
