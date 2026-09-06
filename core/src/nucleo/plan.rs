/* [03-09-2026] Modo plan explícito (plan 318A-16, F5): el agente NO aplica
 * cambios; registra propuestas (archivo → contenido nuevo con su diff) en una
 * store compartida del runtime. Al cerrar el turno la UI muestra el diff
 * acumulado y ofrece "Aprobar y aplicar" (una sola aplicación) o descartar.
 *
 * Diseño (referencia claurst `PermissionMode::Plan`): en modo plan las tools
 * de escritura de archivos quedan `allow` a nivel de permiso (para que lleguen
 * a esta capa) pero aquí se desvían a la propuesta; el resto de tools con
 * efecto siguen `deny` (semántica de `meta`). La store vive en el runtime
 * (efímera, nunca en BD): si el runtime se descarta, la propuesta se pierde
 * con él (mismo ciclo de vida que `todo`).
 *
 * Regla de una sola aplicación: `aplicar` marca la propuesta como aplicada;
 * un segundo `aplicar` falla con error claro (no se puede aplicar dos veces
 * el mismo diff sobre un archivo que ya cambió). */

use std::sync::{Arc, RwLock};

use crate::diff::diff_lineas;
use crate::error::{Error, Result};
use crate::historial::{tomar_checkpoint, HistorialCompartido};
use crate::sandbox::SandboxArchivos;

/// Un cambio propuesto por el agente en modo plan (aún NO aplicado).
#[derive(Debug, Clone)]
pub struct CambioPropuesto {
    /// Ruta relativa al workspace (misma semántica que `file_write`).
    pub ruta: String,
    /// Contenido previo del archivo (vacío si no existía).
    pub antes: String,
    /// Contenido nuevo propuesto.
    pub despues: String,
    /// Diff por hunks (`diff_lineas`) entre antes y después.
    pub diff: String,
}

/// Propuesta acumulada del turno en modo plan.
#[derive(Debug, Default)]
pub struct PlanPropuesto {
    cambios: Vec<CambioPropuesto>,
    aplicado: bool,
}

/// Store compartida del runtime (mismo patrón que `TodoCompartida`).
pub type PlanCompartida = Arc<RwLock<PlanPropuesto>>;

/// Registra un cambio en la propuesta y devuelve su diff. Devuelve `None`
/// si el contenido no cambia (antes == después).
pub fn registrar_cambio(
    plan: &PlanCompartida,
    ruta: impl Into<String>,
    antes: impl Into<String>,
    despues: impl Into<String>,
) -> Option<String> {
    let ruta = ruta.into();
    let antes = antes.into();
    let despues = despues.into();
    let diff = diff_lineas(&antes, &despues);
    /* Sin cambio real (idéntico): `diff_lineas` devuelve `Some("")`; no se
     * registra propuesta alguna (misma regla que file_write: no tocar). */
    if diff.as_deref().is_none_or(str::is_empty) {
        return None;
    }
    let mut guardia = plan.write().unwrap_or_else(|p| p.into_inner());
    /* Un archivo puede proponerse varias veces en el mismo turno: el último
     * cambio sustituye al anterior (el diff mostrado siempre es contra el
     * estado real del disco, no contra propuestas intermedias). */
    if let Some(existente) = guardia.cambios.iter_mut().find(|c| c.ruta == ruta) {
        existente.antes = antes;
        existente.despues = despues;
        existente.diff = diff.clone().unwrap_or_default();
    } else {
        guardia.cambios.push(CambioPropuesto {
            ruta,
            antes,
            despues,
            diff: diff.clone().unwrap_or_default(),
        });
    }
    diff
}

/// ¿Hay cambios pendientes de aplicar?
#[must_use]
pub fn tiene_cambios(plan: &PlanCompartida) -> bool {
    let guardia = plan.read().unwrap_or_else(|p| p.into_inner());
    !guardia.cambios.is_empty() && !guardia.aplicado
}

/// [318A-17 F6] Rutas relativas de los cambios propuestos: exactamente los
/// archivos que `aplicar_plan` va a tocar (lo que necesita un checkpoint
/// previo). Orden estable: el del registro.
#[must_use]
pub fn rutas_plan(plan: &PlanCompartida) -> Vec<String> {
    let guardia = plan.read().unwrap_or_else(|p| p.into_inner());
    guardia
        .cambios
        .iter()
        .map(|cambio| cambio.ruta.clone())
        .collect()
}

/// Número de cambios pendientes.
#[must_use]
pub fn cuenta_cambios(plan: &PlanCompartida) -> usize {
    let guardia = plan.read().unwrap_or_else(|p| p.into_inner());
    guardia.cambios.len()
}

/// Resumen legible de la propuesta (difícil de confundir con la salida de una
/// tool: es el artefacto que ve el humano antes de aprobar).
#[must_use]
pub fn resumen_plan(plan: &PlanCompartida) -> String {
    let guardia = plan.read().unwrap_or_else(|p| p.into_inner());
    if guardia.aplicado {
        return "La propuesta ya fue aplicada.".to_string();
    }
    if guardia.cambios.is_empty() {
        return "No hay cambios propuestos.".to_string();
    }
    let mut salida = format!(
        "PROPUESTA EN MODO PLAN ({} cambio(s), aún NO aplicados):\n",
        guardia.cambios.len()
    );
    for cambio in &guardia.cambios {
        salida.push_str(&format!("\n=== {}\n", cambio.ruta));
        salida.push_str(&cambio.diff);
        salida.push('\n');
    }
    salida
}

/// Aplica TODA la propuesta una sola vez (regla de una sola aplicación):
/// escribe cada cambio en el sandbox y marca la propuesta como aplicada.
/// Un segundo intento falla con error claro; una propuesta vacía también.
pub fn aplicar_plan(plan: &PlanCompartida, sandbox: &SandboxArchivos) -> Result<String> {
    let mut guardia = plan.write().unwrap_or_else(|p| p.into_inner());
    if guardia.aplicado {
        return Err(Error::Validacion(
            "La propuesta ya fue aplicada; no se puede aplicar dos veces el mismo diff".into(),
        ));
    }
    if guardia.cambios.is_empty() {
        return Err(Error::Validacion(
            "No hay cambios propuestos para aplicar".into(),
        ));
    }
    let mut aplicados = Vec::with_capacity(guardia.cambios.len());
    for cambio in &guardia.cambios {
        sandbox.escribir(&cambio.ruta, &cambio.despues)?;
        aplicados.push(format!(
            "- {} ({} líneas cambiadas)",
            cambio.ruta,
            cambio.diff.lines().count()
        ));
    }
    guardia.aplicado = true;
    Ok(format!(
        "Propuesta aplicada ({} cambio(s)):\n{}",
        aplicados.len(),
        aplicados.join("\n")
    ))
}

/// [318A-17 F6] Aplica la propuesta dejando un checkpoint recuperable: captura
/// la imagen previa de los archivos objetivo ANTES de escribir (fail-closed:
/// si el snapshot falla no se aplica nada) y luego delega en `aplicar_plan`.
/// El consumidor ofrece `/undo` sobre el mismo historial de sesión.
pub fn aplicar_plan_con_checkpoint(
    plan: &PlanCompartida,
    sandbox: &SandboxArchivos,
    historial: &HistorialCompartido,
) -> Result<String> {
    let rutas = rutas_plan(plan);
    tomar_checkpoint(historial, sandbox, "plan aprobado", &rutas)?;
    aplicar_plan(plan, sandbox)
}

/// Descarta la propuesta sin aplicar nada.
pub fn descartar_plan(plan: &PlanCompartida) -> String {
    let mut guardia = plan.write().unwrap_or_else(|p| p.into_inner());
    let n = guardia.cambios.len();
    guardia.cambios.clear();
    guardia.aplicado = false;
    format!("Propuesta descartada ({} cambio(s) sin aplicar).", n)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan_nuevo() -> PlanCompartida {
        Arc::new(RwLock::new(PlanPropuesto::default()))
    }

    fn sandbox_tmp() -> SandboxArchivos {
        let dir = std::env::temp_dir().join(format!("gh-plan-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("crear tmp");
        SandboxArchivos::nuevo(dir).expect("sandbox")
    }

    #[test]
    fn registrar_cambio_guarda_y_devuelve_diff() {
        let plan = plan_nuevo();
        let diff = registrar_cambio(&plan, "a.txt", "hola\n", "hola mundo\n");
        assert!(diff.is_some(), "el diff existe cuando cambia el contenido");
        let resumen = resumen_plan(&plan);
        assert!(resumen.contains("PROPUESTA EN MODO PLAN"));
        assert!(resumen.contains("+hola mundo"));
        assert!(tiene_cambios(&plan));
    }

    #[test]
    fn sin_cambios_no_genera_diff() {
        let plan = plan_nuevo();
        let diff = registrar_cambio(&plan, "a.txt", "igual\n", "igual\n");
        assert_eq!(diff, None);
        assert!(!tiene_cambios(&plan), "antes == después → sin propuesta");
    }

    #[test]
    fn mismo_archivo_sustituye_la_propuesta_anterior() {
        let plan = plan_nuevo();
        registrar_cambio(&plan, "a.txt", "v1\n", "v2\n");
        registrar_cambio(&plan, "a.txt", "v2\n", "v3\n");
        assert_eq!(cuenta_cambios(&plan), 1);
        let resumen = resumen_plan(&plan);
        assert!(
            resumen.contains("-v2"),
            "el diff es contra el disco (antes real)"
        );
        assert!(resumen.contains("+v3"));
        assert!(
            !resumen.contains("-v1"),
            "sin propuestas intermedias: v1 no aparece"
        );
    }

    #[test]
    fn aplicar_escribe_una_sola_vez_y_la_segunda_falla() {
        let plan = plan_nuevo();
        let sandbox = sandbox_tmp();
        registrar_cambio(&plan, "nuevo.txt", "", "contenido aplicado\n");
        let ok = aplicar_plan(&plan, &sandbox).expect("aplicar");
        assert!(ok.contains("Propuesta aplicada"));
        assert_eq!(
            sandbox.leer("nuevo.txt", 1024).expect("leer").0,
            "contenido aplicado\n"
        );
        /* Segunda aplicación: error claro, no reescribe. */
        let err = aplicar_plan(&plan, &sandbox).expect_err("segunda aplicación falla");
        assert!(err.to_string().contains("ya fue aplicada"));
    }

    #[test]
    fn aplicar_vacio_falla_sin_tocar_disco() {
        let plan = plan_nuevo();
        let sandbox = sandbox_tmp();
        let err = aplicar_plan(&plan, &sandbox).expect_err("vacío falla");
        assert!(err.to_string().contains("No hay cambios propuestos"));
    }

    #[test]
    fn descartar_limpia_y_permite_reproponer() {
        let plan = plan_nuevo();
        registrar_cambio(&plan, "a.txt", "", "x\n");
        let msg = descartar_plan(&plan);
        assert!(msg.contains("descartada"));
        assert!(!tiene_cambios(&plan));
        assert_eq!(resumen_plan(&plan), "No hay cambios propuestos.");
    }

    /* E2E determinista (plan 318A-16 F5, ítem 5): fixture plan → propuesta →
     * aprobar → verificar el archivo; sin proveedor LLM. */
    #[test]
    fn e2e_plan_propuesta_aprobar_verifica_archivo() {
        let plan = plan_nuevo();
        let sandbox = sandbox_tmp();
        sandbox
            .escribir("doc.txt", "linea1\nlinea2\nlinea3\n")
            .expect("fixture");

        /* El agente "propone" dos cambios (como harían file_write/file_patch
         * en modo plan). */
        let (antes, _) = sandbox.leer("doc.txt", 1024).expect("leer");
        registrar_cambio(
            &plan,
            "doc.txt",
            &antes,
            "linea1\nlinea2 CAMBIADA\nlinea3\n",
        );
        registrar_cambio(&plan, "nota.txt", "", "nota nueva\n");

        /* El humano ve el diff acumulado... */
        let resumen = resumen_plan(&plan);
        assert!(resumen.contains("doc.txt"));
        assert!(resumen.contains("-linea2"));
        assert!(resumen.contains("+linea2 CAMBIADA"));
        assert!(resumen.contains("nota.txt"));

        /* ...y aprueba: se aplica una sola vez, exactamente ese diff. */
        let ok = aplicar_plan(&plan, &sandbox).expect("aplicar");
        assert!(ok.contains("2 cambio(s)"));
        let (contenido, _) = sandbox.leer("doc.txt", 1024).expect("leer doc");
        assert_eq!(contenido, "linea1\nlinea2 CAMBIADA\nlinea3\n");
        let (nota, _) = sandbox.leer("nota.txt", 1024).expect("leer nota");
        assert_eq!(nota, "nota nueva\n");
        assert!(!tiene_cambios(&plan), "aplicado → ya no hay pendientes");
    }

    /* E2E determinista (plan 318A-17 F6): aprobar con `aplicar_plan_con_`
     * `checkpoint` deja imagen previa recuperable; `revertir_ultimo` restaura
     * el estado original (archivo modificado y archivo creado) sin tocar git.
     * Fixture plan → aprobar → undo, sin proveedor LLM. */
    #[test]
    fn e2e_aprobar_con_checkpoint_y_undo_restaura() {
        use crate::historial::{historial_compartido, revertir_ultimo};

        let plan = plan_nuevo();
        let sandbox = sandbox_tmp();
        sandbox.escribir("doc.txt", "v1\n").expect("fixture");
        let (antes, _) = sandbox.leer("doc.txt", 1024).expect("leer");
        registrar_cambio(&plan, "doc.txt", &antes, "v2\n");
        registrar_cambio(&plan, "nuevo.txt", "", "creado\n");
        let historial = historial_compartido();

        /* Aprobar: captura previa ANTES de escribir y aplica una sola vez. */
        let msg = aplicar_plan_con_checkpoint(&plan, &sandbox, &historial).expect("aplicar");
        assert!(msg.contains("2 cambio(s)"));
        assert_eq!(sandbox.leer("doc.txt", 1024).expect("leer").0, "v2\n");
        assert_eq!(sandbox.leer("nuevo.txt", 1024).expect("leer").0, "creado\n");

        /* Un segundo aplicar falla (regla de una sola aplicación). */
        assert!(aplicar_plan(&plan, &sandbox).is_err());

        /* Undo: doc.txt vuelve a v1 y nuevo.txt desaparece (no existía). */
        let undo = revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("hay checkpoint");
        assert!(undo.contains("#1") && undo.contains("plan aprobado"));
        assert!(undo.contains("restaurado") && undo.contains("eliminado"));
        assert_eq!(sandbox.leer("doc.txt", 1024).expect("leer").0, "v1\n");
        assert!(
            sandbox.leer("nuevo.txt", 1024).is_err(),
            "el archivo creado se eliminó"
        );
        assert!(
            revertir_ultimo(&historial, &sandbox)
                .expect("undo vacío")
                .is_none(),
            "la pila quedó vacía tras el undo"
        );
    }
}
