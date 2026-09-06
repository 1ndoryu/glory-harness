/* [05-09-2026] Checkpoint/undo de cambios de archivo (plan 318A-17, Fase 6):
 * pila acotada de snapshots con la imagen previa de cada archivo tocado,
 * estilo claurst `file_history.rs` y grok `verify/checkpoint.ts` — SIN tocar
 * el estado git real del repo (una copia de archivos tocados es suficiente y
 * determinista; un commit git local sería destructivo si falla a mitad).
 *
 * Ciclo de vida: la store (`HistorialCompartido`) es de sesión, igual que
 * `PlanCompartida` (efímera, en memoria). Antes de aplicar un lote de
 * escrituras (`aplicar_plan`) el consumidor captura `tomar_checkpoint` con
 * las rutas objetivo; `revertir_ultimo` restaura la imagen previa (reescribe
 * el contenido anterior o borra el archivo si no existía).
 *
 * Captura fail-closed: si una ruta escapa del sandbox o una lectura previa
 * falla, el checkpoint NO se toma (mejor no registrar un snapshot incompleto
 * que uno que mienta). Restaurar reutiliza `SandboxArchivos::escribir` (pasa
 * la misma validación de secretos/contención que cualquier escritura). */

use std::sync::{Arc, RwLock};

use crate::error::{Error, Result};
use crate::sandbox::SandboxArchivos;

/// Imagen previa de un archivo tocado por un checkpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImagenArchivo {
    /// Ruta relativa al workspace (misma semántica que `file_write`).
    pub relativa: String,
    /// Contenido previo completo. `None` = el archivo no existía antes del
    /// cambio (revertir lo eliminará).
    pub previo: Option<String>,
}

/// Un punto de restauración: el estado previo de un lote de archivos.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    /// Identificador secuencial estable (aparece en los mensajes de undo).
    pub id: u64,
    /// Por qué se tomó (p. ej. "plan aprobado").
    pub momento: String,
    /// Imágenes previas de los archivos que el cambio iba a tocar.
    pub imagenes: Vec<ImagenArchivo>,
}

/// Pila de checkpoints con límite acotado (los más viejos se descartan para
/// no crecer sin fin en una sesión larga).
#[derive(Debug)]
pub struct HistorialCambios {
    pila: Vec<Checkpoint>,
    limite: usize,
    siguiente_id: u64,
}

/// Store compartida del historial: una por sesión de consumidor (REPL), igual
/// que `PlanCompartida` vive por runtime. Función libre porque un alias de
/// `Arc<RwLock<…>>` no expone métodos asociados del tipo interior.
pub type HistorialCompartido = Arc<RwLock<HistorialCambios>>;

/// Crea una store compartida lista para inyectar en el consumidor.
#[must_use]
pub fn historial_compartido() -> HistorialCompartido {
    Arc::new(RwLock::new(HistorialCambios::nuevo()))
}

impl HistorialCambios {
    /// Límite por defecto de checkpoints retenidos por sesión.
    pub const LIMITE_DEFECTO: usize = 20;

    #[must_use]
    pub fn nuevo() -> Self {
        Self {
            pila: Vec::new(),
            limite: Self::LIMITE_DEFECTO,
            siguiente_id: 1,
        }
    }

    /// Cambia el límite de retención (los excedentes más viejos se descartan).
    pub fn con_limite(mut self, limite: usize) -> Self {
        self.limite = limite.max(1);
        self
    }

    /// Checkpoints retenidos (los ids de más reciente a más antiguo).
    #[must_use]
    pub fn ids(&self) -> Vec<u64> {
        self.pila.iter().rev().map(|c| c.id).collect()
    }

    #[must_use]
    pub fn esta_vacio(&self) -> bool {
        self.pila.is_empty()
    }
}

/// Toma la imagen previa de `relativas` y la apila como checkpoint nuevo.
/// Devuelve el id del checkpoint. Fail-closed: una ruta que escape del
/// sandbox o una lectura que falle aborta sin registrar nada.
pub fn tomar_checkpoint(
    historial: &HistorialCompartido,
    sandbox: &SandboxArchivos,
    momento: &str,
    relativas: &[String],
) -> Result<u64> {
    let mut imagenes = Vec::with_capacity(relativas.len());
    for relativa in relativas {
        imagenes.push(ImagenArchivo {
            relativa: relativa.clone(),
            previo: leer_previa(sandbox, relativa)?,
        });
    }
    let mut guardia = historial.write().unwrap_or_else(|p| p.into_inner());
    let id = guardia.siguiente_id;
    guardia.siguiente_id += 1;
    guardia.pila.push(Checkpoint {
        id,
        momento: momento.to_string(),
        imagenes,
    });
    /* Límite acotado: descarta los más viejos, no el recién tomado. */
    while guardia.pila.len() > guardia.limite {
        guardia.pila.remove(0);
    }
    Ok(id)
}

/// Restaura el checkpoint más reciente y lo retira de la pila. Devuelve un
/// resumen legible de lo restaurado. `None` = no hay nada que deshacer.
pub fn revertir_ultimo(
    historial: &HistorialCompartido,
    sandbox: &SandboxArchivos,
) -> Result<Option<String>> {
    let checkpoint = {
        let mut guardia = historial.write().unwrap_or_else(|p| p.into_inner());
        guardia.pila.pop()
    };
    let Some(checkpoint) = checkpoint else {
        return Ok(None);
    };
    let mut restaurados = 0usize;
    let mut eliminados = 0usize;
    for imagen in &checkpoint.imagenes {
        match &imagen.previo {
            Some(previo) => {
                sandbox.escribir(&imagen.relativa, previo)?;
                restaurados += 1;
            }
            None => {
                /* El archivo no existía antes del cambio: revertir = borrarlo. */
                match sandbox.resolver(&imagen.relativa) {
                    Ok(ruta) => {
                        if let Err(error) = std::fs::remove_file(&ruta) {
                            if error.kind() != std::io::ErrorKind::NotFound {
                                return Err(Error::Validacion(format!(
                                    "No se pudo eliminar {} al revertir: {error}",
                                    imagen.relativa
                                )));
                            }
                        }
                        eliminados += 1;
                    }
                    /* Ya no existe: nada que borrar (el cambio no llegó a crear). */
                    Err(Error::NoEncontrado(_)) => {}
                    Err(error) => return Err(error),
                }
            }
        }
    }
    let mut partes = vec![format!(
        "Checkpoint #{} ({}) revertido",
        checkpoint.id, checkpoint.momento
    )];
    if restaurados > 0 {
        partes.push(format!("{restaurados} archivo(s) restaurado(s)"));
    }
    if eliminados > 0 {
        partes.push(format!("{eliminados} archivo(s) nuevo(s) eliminado(s)"));
    }
    Ok(Some(partes.join(" · ")))
}

/// Resumen del historial (ids + momentos) para mostrarlo en el REPL.
#[must_use]
pub fn resumen_historial(historial: &HistorialCompartido) -> String {
    let guardia = historial.read().unwrap_or_else(|p| p.into_inner());
    if guardia.pila.is_empty() {
        return "historial de cambios vacío".to_string();
    }
    let linea: Vec<String> = guardia
        .pila
        .iter()
        .rev()
        .map(|c| format!("#{} ({})", c.id, c.momento))
        .collect();
    format!("{} checkpoint(s): {}", guardia.pila.len(), linea.join(", "))
}

/// Lee el contenido previo completo de `relativa` en el sandbox.
/// `Ok(None)` = no existe (ni el archivo ni su directorio padre).
/// Fail-closed: cualquier otro fallo de resolución o lectura aborta.
fn leer_previa(sandbox: &SandboxArchivos, relativa: &str) -> Result<Option<String>> {
    let ruta = match sandbox.resolver(relativa) {
        Ok(ruta) => ruta,
        Err(Error::NoEncontrado(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    match std::fs::read_to_string(&ruta) {
        Ok(contenido) => Ok(Some(contenido)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::Validacion(format!(
            "No se pudo leer el estado previo de {relativa} para el checkpoint: {error}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    fn sandbox_tmp() -> SandboxArchivos {
        let dir = std::env::temp_dir().join(format!("gh-historial-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("crear tmp");
        SandboxArchivos::nuevo(dir).expect("sandbox")
    }

    fn historial() -> HistorialCompartido {
        historial_compartido()
    }

    fn escribir(sandbox: &SandboxArchivos, relativa: &str, contenido: &str) {
        sandbox.escribir(relativa, contenido).expect("escribir");
    }

    fn leer(sandbox: &SandboxArchivos, relativa: &str) -> String {
        sandbox.leer(relativa, usize::MAX).expect("leer").0
    }

    #[test]
    fn checkpoint_captura_previa_y_undo_restaura_contenido() {
        let sandbox = sandbox_tmp();
        escribir(&sandbox, "a.txt", "v1\n");
        let historial = historial();
        /* El checkpoint se toma ANTES del cambio (con el estado v1). */
        let id =
            tomar_checkpoint(&historial, &sandbox, "plan", &["a.txt".into()]).expect("checkpoint");
        assert_eq!(id, 1);
        escribir(&sandbox, "a.txt", "v2\n");
        assert_eq!(leer(&sandbox, "a.txt"), "v2\n");
        let msg = revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("hay checkpoint");
        assert!(msg.contains("#1"), "el mensaje nombra el checkpoint");
        assert!(msg.contains("restaurado"));
        assert_eq!(leer(&sandbox, "a.txt"), "v1\n", "restaura la imagen previa");
        assert!(historial
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .esta_vacio());
    }

    #[test]
    fn undo_elimina_archivo_creado_por_el_cambio() {
        let sandbox = sandbox_tmp();
        escribir(&sandbox, "base.txt", "x\n");
        let historial = historial();
        /* El archivo no existe: la imagen previa es None. */
        tomar_checkpoint(&historial, &sandbox, "crear", &["nuevo.txt".into()]).expect("ckpt");
        escribir(&sandbox, "nuevo.txt", "contenido\n");
        assert_eq!(leer(&sandbox, "nuevo.txt"), "contenido\n");
        let msg = revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("hay checkpoint");
        assert!(msg.contains("eliminado"));
        let err = sandbox
            .leer("nuevo.txt", usize::MAX)
            .expect_err("ya no existe");
        assert!(
            matches!(err, Error::NoEncontrado(_)),
            "error de no encontrado: {err}"
        );
        assert_eq!(leer(&sandbox, "base.txt"), "x\n", "el resto queda intacto");
    }

    #[test]
    fn dos_checkpoints_revierten_en_lifo() {
        let sandbox = sandbox_tmp();
        escribir(&sandbox, "a.txt", "v0\n");
        let historial = historial();
        tomar_checkpoint(&historial, &sandbox, "c1", &["a.txt".into()]).expect("ckpt 1");
        escribir(&sandbox, "a.txt", "v1\n");
        tomar_checkpoint(&historial, &sandbox, "c2", &["a.txt".into()]).expect("ckpt 2");
        escribir(&sandbox, "a.txt", "v2\n");
        revertir_ultimo(&historial, &sandbox)
            .expect("undo 2")
            .expect("ckpt 2");
        assert_eq!(leer(&sandbox, "a.txt"), "v1\n", "undo 2 → estado tras c1");
        revertir_ultimo(&historial, &sandbox)
            .expect("undo 1")
            .expect("ckpt 1");
        assert_eq!(leer(&sandbox, "a.txt"), "v0\n", "undo 1 → estado original");
        assert!(
            revertir_ultimo(&historial, &sandbox)
                .expect("undo vacío")
                .is_none(),
            "sin más checkpoints no hay nada que deshacer"
        );
    }

    #[test]
    fn limite_acotado_descarta_los_mas_viejos() {
        let sandbox = sandbox_tmp();
        let historial = Arc::new(RwLock::new(HistorialCambios::nuevo().con_limite(2)));
        for i in 0..3 {
            let relativa = format!("a{i}.txt");
            tomar_checkpoint(
                &historial,
                &sandbox,
                &format!("c{i}"),
                std::slice::from_ref(&relativa),
            )
            .expect("ckpt");
            escribir(&sandbox, &relativa, "contenido\n");
        }
        let ids = historial.read().unwrap_or_else(|p| p.into_inner()).ids();
        assert_eq!(ids, vec![3, 2], "el checkpoint 1 se descartó por el límite");
        revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("ckpt 3");
        revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("ckpt 2");
        assert!(
            revertir_ultimo(&historial, &sandbox)
                .expect("undo")
                .is_none(),
            "el checkpoint descartado ya no es recuperable"
        );
    }

    #[test]
    fn checkpoint_falla_si_una_ruta_escapa_del_sandbox() {
        let sandbox = sandbox_tmp();
        escribir(&sandbox, "a.txt", "v1\n");
        let historial = historial();
        let ruta_fuera = std::path::Path::new("..").join("fuera.txt");
        let err = tomar_checkpoint(
            &historial,
            &sandbox,
            "malo",
            &[ruta_fuera.to_string_lossy().into_owned()],
        )
        .expect_err("la ruta con '..' debe fallar");
        assert!(
            err.to_string().contains("..") || err.to_string().contains("no se permiten"),
            "error claro de contención: {err}"
        );
        /* Fail-closed: nada quedó registrado. */
        assert!(historial
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .esta_vacio());
    }

    #[test]
    fn checkpoint_con_archivo_vacio_y_previo_vacio_se_distingue_de_inexistente() {
        let sandbox = sandbox_tmp();
        let historial = historial();
        /* Archivo vacío: previo Some("") → undo restaura vacío (no borra). */
        escribir(&sandbox, "vacio.txt", "");
        tomar_checkpoint(&historial, &sandbox, "editar vacío", &["vacio.txt".into()])
            .expect("ckpt");
        escribir(&sandbox, "vacio.txt", "ahora tiene contenido\n");
        let msg = revertir_ultimo(&historial, &sandbox)
            .expect("undo")
            .expect("hay checkpoint");
        assert!(msg.contains("restaurado") && !msg.contains("eliminado"));
        assert_eq!(
            leer(&sandbox, "vacio.txt"),
            "",
            "se restauró el archivo vacío, no se borró"
        );
    }
}
