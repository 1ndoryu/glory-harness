//! Lib compartida del CLI `glory-harness` (plan 039A-1, Fase 1).
//!
//! La lógica de sesión vivía en el crate bin (`main.rs` + módulos privados):
//! la app de escritorio necesita la MISMA construcción (runtime, persistencia,
//! historial entre turnos) sin duplicarla. Este lib expone esa superficie;
//! `main.rs` queda como despachador fino de subcomandos.
//!
//! Sin cambio de comportamiento: los símbolos son los mismos, solo cambia la
//! visibilidad (`pub(crate)` → `pub`) y el punto de declaración.

/* [059A-S4] Organización por dominio (glory-sentinel directorio-abarrotado).
 * Los módulos viven en `comandos/`, `ui/` e `infra/`; el glob los re-exporta
 * en la raíz del crate para que `glory_harness::{chat, daemon, run, tui, …}`
 * de `main.rs` y del escritorio sigan resolviendo sin cambios. `persistencia_sqlite`
 * queda en la raíz hasta el cierre del ajeno 039A-3. */
mod comandos;
mod ui;
mod infra;

pub use comandos::*;
pub use ui::*;
pub use infra::*;

pub mod persistencia_sqlite;

pub use ui::turno::{TurnoResultado, historial_desde_persistencia, procesar_turno};
pub use infra::ejecutor::EjecutorCliente;
pub use infra::persistencia::{PersistenciaMemoria, ProgramadorMemoria};
pub use persistencia_sqlite::{AccionRecuperada, InfoConversacion, PersistenciaSqlite};
pub use infra::reglas::cargar_reglas;
pub use comandos::run::{OpcionesRun, SalidaTurno, construir_harness, construir_harness_con, quitar_prefijo_verbatim, turno_config_default};

/// Carga `~/.glory-harness.env` si existe (formato `CLAVE=valor`,
/// `#` = comentario). Solo define variables aún ausentes, así el entorno real
/// del proceso siempre tiene prioridad. Nunca imprime valores.
///
/// Vive en el lib (antes en `main.rs`) para que el CLI y la app de escritorio
/// compartan la misma carga de claves LLM.
pub fn cargar_env_usuario() {
    let Some(home) = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(std::path::PathBuf::from)
    else {
        return;
    };
    let ruta = home.join(".glory-harness.env");
    let contenido = match std::fs::read_to_string(&ruta) {
        Ok(c) => c,
        Err(_) => return, // no existe o no legible: sin claves extra, no es error
    };
    for linea in contenido.lines() {
        let linea = linea.trim();
        if linea.is_empty() || linea.starts_with('#') {
            continue;
        }
        let Some((clave, valor)) = linea.split_once('=') else {
            continue;
        };
        let clave = clave.trim();
        let valor = valor.trim();
        if clave.is_empty() || valor.is_empty() {
            continue;
        }
        // Solo si no está ya definida (edition 2021: set_var es seguro).
        if std::env::var_os(clave).is_none() {
            std::env::set_var(clave, valor);
        }
    }
}
