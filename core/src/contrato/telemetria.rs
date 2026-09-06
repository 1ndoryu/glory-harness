//! [318A-15 F0] Telemetría del turno (no invasiva).
//!
//! Acumula, durante la ejecución de un turno, lo que el runtime ya observa
//! (usos/fallos/duración por tool, denegaciones de permiso, subagentes
//! parciales) y lo expone como evento `Telemetria` justo antes de `Done`.
//! No cambia el comportamiento del turno: solo observa y agrega.
//!
//! Los turnos fallidos no emiten `Telemetria` (el contrato ya los cubre con
//! `Error { mensaje, retryable }`); este evento acompaña solo a `Done`.
//! Diseñado para que F2/F6 (umbrales de compactación/reglas) y la decisión
//! F4 item-8 (manager-executor vs proceso hijo) consuman sus agregados.

use std::collections::HashMap;

use uuid::Uuid;

use crate::evento::{AgenteEvento, TelemetriaTool};

/// Contadores por tool dentro de un turno.
#[derive(Debug, Clone, Default)]
pub struct ContadoresTool {
    pub usos: u32,
    pub fallos: u32,
    pub duracion_ms_total: u64,
}

/// Acumulador del turno: interior-mutable en el runtime (`Mutex`), reseteado
/// al emitir `Telemetria`. Las duraciones se miden en el punto de ejecución
/// (incluyen el timeout de tool: un timeout cuenta como fallo).
#[derive(Debug, Clone, Default)]
pub struct TelemetriaTurno {
    por_tool: HashMap<String, ContadoresTool>,
    pub denegaciones: u32,
    pub subagentes_parciales: u32,
}

impl TelemetriaTurno {
    #[must_use]
    pub fn nuevo() -> Self {
        Self::default()
    }

    /// Registra una ejecución de tool: `ok=false` cuenta como fallo (resultado
    /// de tool con error o timeout).
    pub fn registrar_uso(&mut self, tool: &str, ok: bool, duracion_ms: u64) {
        let c = self.por_tool.entry(tool.to_string()).or_default();
        c.usos += 1;
        if !ok {
            c.fallos += 1;
        }
        c.duracion_ms_total += duracion_ms;
    }

    /// Registra una denegación de permiso emitida (política o usuario).
    pub fn registrar_denegacion(&mut self) {
        self.denegaciones += 1;
    }

    /// Registra un subagente cerrado como parcial (presupuesto agotado).
    pub fn registrar_subagente_parcial(&mut self) {
        self.subagentes_parciales += 1;
    }

    /// Agregados por tool, ordenados por usos desc (desempate alfabético),
    /// para un reporte/evento determinista.
    #[must_use]
    pub fn herramientas(&self) -> Vec<TelemetriaTool> {
        let mut v: Vec<TelemetriaTool> = self
            .por_tool
            .iter()
            .map(|(tool, c)| TelemetriaTool {
                tool: tool.clone(),
                usos: c.usos,
                fallos: c.fallos,
                duracion_ms_total: c.duracion_ms_total,
            })
            .collect();
        v.sort_by(|a, b| b.usos.cmp(&a.usos).then_with(|| a.tool.cmp(&b.tool)));
        v
    }
}

/// Motivo de cierre del turno para la telemetría.
///
/// Prioridad: SSE cortado > respuesta final > límite de pasos con wrap-up
/// (F5) > sin respuesta (proveedor caído; el consumidor reintenta).
#[must_use]
pub fn motivo_cierre(tiene_respuesta: bool, limite_pasos: bool, sse_cortado: bool) -> &'static str {
    if sse_cortado {
        "sse_cortado"
    } else if tiene_respuesta {
        "respuesta_final"
    } else if limite_pasos {
        "limite_pasos"
    } else {
        "sin_respuesta"
    }
}

/// Construye el evento `Telemetria` a partir del acumulador (función pura,
/// testeable sin runtime).
#[must_use]
pub fn construir_evento(
    conversacion_id: Uuid,
    motivo_cierre: &str,
    compactaciones: u32,
    turno: &TelemetriaTurno,
) -> AgenteEvento {
    AgenteEvento::Telemetria {
        conversacion_id,
        motivo_cierre: motivo_cierre.to_string(),
        compactaciones,
        denegaciones: turno.denegaciones,
        subagentes_parciales: turno.subagentes_parciales,
        herramientas: turno.herramientas(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f0_agrega_usos_fallos_y_duracion_por_tool() {
        let mut t = TelemetriaTurno::nuevo();
        t.registrar_uso("file_read", true, 12);
        t.registrar_uso("file_read", true, 8);
        t.registrar_uso("file_read", false, 100);
        t.registrar_uso("web_search", true, 500);
        t.registrar_denegacion();
        t.registrar_denegacion();
        t.registrar_subagente_parcial();

        let h = t.herramientas();
        assert_eq!(h.len(), 2);
        let fr = h.iter().find(|x| x.tool == "file_read").unwrap();
        assert_eq!(fr.usos, 3);
        assert_eq!(fr.fallos, 1);
        assert_eq!(fr.duracion_ms_total, 120);
        assert_eq!(t.denegaciones, 2);
        assert_eq!(t.subagentes_parciales, 1);
    }

    #[test]
    fn f0_herramientas_ordenadas_por_usos_desc_y_nombre() {
        let mut t = TelemetriaTurno::nuevo();
        t.registrar_uso("b", true, 0);
        t.registrar_uso("a", true, 0);
        t.registrar_uso("c", true, 0);
        t.registrar_uso("b", true, 0);
        let h = t.herramientas();
        assert_eq!(h[0].tool, "b");
        assert_eq!(h[1].tool, "a");
        assert_eq!(h[2].tool, "c");
    }

    #[test]
    fn f0_motivo_cierre_prioriza_sse_y_respuesta() {
        assert_eq!(motivo_cierre(true, false, false), "respuesta_final");
        assert_eq!(motivo_cierre(false, true, false), "limite_pasos");
        assert_eq!(motivo_cierre(false, false, false), "sin_respuesta");
        assert_eq!(motivo_cierre(true, false, true), "sse_cortado");
        assert_eq!(motivo_cierre(false, true, true), "sse_cortado");
    }

    #[test]
    fn f0_contrato_serializa_campos_snake_case() {
        /* El contrato SSE se preserva: el evento serializa con `tipo:
         * "telemetria"` y los nombres de campo exactos que el consumidor
         * puede ignorar de forma segura (serde deny_unknown_fields no se
         * aplica a la recepción del front). */
        let mut t = TelemetriaTurno::nuevo();
        t.registrar_uso("todo", true, 3);
        t.registrar_denegacion();
        let ev = construir_evento(Uuid::nil(), "respuesta_final", 2, &t);
        let json = serde_json::to_value(&ev).unwrap();
        assert_eq!(json["tipo"], "telemetria");
        assert_eq!(json["motivo_cierre"], "respuesta_final");
        assert_eq!(json["compactaciones"], 2);
        assert_eq!(json["denegaciones"], 1);
        assert_eq!(json["subagentes_parciales"], 0);
        assert_eq!(json["herramientas"][0]["tool"], "todo");
        assert_eq!(json["herramientas"][0]["usos"], 1);
        assert_eq!(json["herramientas"][0]["fallos"], 0);
        assert_eq!(json["herramientas"][0]["duracion_ms_total"], 3);
    }
}
