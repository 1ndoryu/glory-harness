//! Punto de compactación de una conversación ([109A-4 F3]). Partido de
//! `conversaciones.rs` (limite-lineas 500 al añadir la compactación por
//! demanda): el asunto es independiente —dónde quedó resumido el historial—
//! y no comparte estado con el CRUD de conversaciones.

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::{ahora_rfc3339, bloquear, CompactacionPersistida, PersistenciaSqlite};

impl PersistenciaSqlite {
    /// [109A-4 F3] Punto de compactación de una conversación propia.
    ///
    /// `None` si la conversación no existe, es de otro usuario o nunca se
    /// compactó. Un resumen vacío se trata como ausencia: sin texto que
    /// sustituya al historial no hay punto de compactación que aplicar.
    pub fn conversacion_compactacion(
        &self,
        user_id: Uuid,
        conversacion_id: Uuid,
    ) -> HarnessResult<Option<CompactacionPersistida>> {
        bloquear(&self.conn)
            .query_row(
                "SELECT compactado_en, resumen_compactado
                 FROM conversaciones WHERE id = ?1 AND user_id = ?2",
                params![
                    conversacion_id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
                |f| {
                    Ok((
                        f.get::<_, Option<String>>(0)?,
                        f.get::<_, Option<String>>(1)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))
            .map(|fila| {
                fila.and_then(|(cuando, resumen)| match (cuando, resumen) {
                    (Some(compactado_en), Some(resumen)) if !resumen.trim().is_empty() => {
                        Some(CompactacionPersistida {
                            compactado_en,
                            resumen,
                        })
                    }
                    _ => None,
                })
            })
    }

    /// [109A-4 F3] Guarda el punto de compactación (`resumen` + marca de
    /// tiempo). Los mensajes anteriores NO se tocan: solo dejan de enviarse al
    /// modelo. Devuelve `false` si no existe o no es del usuario.
    pub fn conversacion_compactar(
        &self,
        user_id: Uuid,
        conversacion_id: Uuid,
        compactado_en: &str,
        resumen: &str,
    ) -> HarnessResult<bool> {
        let filas = bloquear(&self.conn)
            .execute(
                "UPDATE conversaciones SET
                    compactado_en = ?1,
                    resumen_compactado = ?2,
                    actualizada_en = ?3
                 WHERE id = ?4 AND user_id = ?5",
                params![
                    compactado_en,
                    resumen,
                    ahora_rfc3339(),
                    conversacion_id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(filas == 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use std::path::PathBuf;

    /// Ruta temporal única para la BD de un test (se borra al terminar).
    fn ruta_temp(nombre: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gh-sqlite-test-{}-{}-{nombre}.db",
            std::process::id(),
            Utc::now().timestamp_millis()
        ))
    }

    /// [109A-4 F3] Punto de compactación: `None` sin compactar, round-trip con
    /// resumen, resumen en blanco = ausencia y conversación ajena intacta.
    #[test]
    fn compactacion_round_trip_y_ownership() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let otro = Uuid::new_v4();
        let conv = p.conversacion_crear(user, "compactar").expect("crear");
        assert!(p
            .conversacion_compactacion(user, conv)
            .expect("leer")
            .is_none());

        // Resumen en blanco: la fila se escribe pero se lee como ausencia (sin
        // texto que sustituya al historial no hay punto que aplicar).
        assert!(p
            .conversacion_compactar(user, conv, "2026-09-10T10:00:00Z", "   ")
            .expect("guardar en blanco"));
        assert!(p
            .conversacion_compactacion(user, conv)
            .expect("leer")
            .is_none());

        // Otro usuario: no escribe ni lee.
        assert!(!p
            .conversacion_compactar(otro, conv, "2026-09-10T10:00:00Z", "ajeno")
            .expect("guardar ajeno"));
        assert!(p
            .conversacion_compactacion(otro, conv)
            .expect("leer ajeno")
            .is_none());

        assert!(p
            .conversacion_compactar(user, conv, "2026-09-10T10:00:00Z", "resumen del tramo")
            .expect("guardar"));
        let punto = p
            .conversacion_compactacion(user, conv)
            .expect("leer")
            .expect("punto");
        assert_eq!(punto.compactado_en, "2026-09-10T10:00:00Z");
        assert_eq!(punto.resumen, "resumen del tramo");
    }

    /// [109A-4 F3] Una BD creada con el esquema ANTERIOR (sin las columnas de
    /// compactación) se migra al abrir: las columnas vuelven, las filas previas
    /// sobreviven y el punto queda utilizable.
    #[test]
    fn migracion_anade_columnas_de_compactacion() {
        let ruta = ruta_temp("migracion-compactar");
        let user = Uuid::new_v4();
        let conv = {
            let p = PersistenciaSqlite::abrir(&ruta).expect("abrir BD");
            p.conversacion_crear(user, "previa").expect("crear")
        };
        // Simula la versión anterior: fuera las columnas nuevas.
        {
            let conn = rusqlite::Connection::open(&ruta).expect("abrir cruda");
            conn.execute_batch(
                "ALTER TABLE conversaciones DROP COLUMN compactado_en;
                 ALTER TABLE conversaciones DROP COLUMN resumen_compactado;",
            )
            .expect("soltar columnas");
        }
        let p = PersistenciaSqlite::abrir(&ruta).expect("reabrir BD");
        assert!(
            p.conversacion_compactar(user, conv, "2026-09-10T10:00:00Z", "resumen migrado")
                .expect("guardar tras migrar"),
            "la conversación previa debe sobrevivir a la migración"
        );
        let punto = p
            .conversacion_compactacion(user, conv)
            .expect("leer")
            .expect("punto");
        assert_eq!(punto.resumen, "resumen migrado");
        let _ = std::fs::remove_file(&ruta);
    }
}
