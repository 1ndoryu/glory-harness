//! Tareas programadas de `PersistenciaSqlite` ([069A-5 F5], dominio de
//! [B3-F8a]): cola del scheduler (`AgentPersistence`) + CRUD de
//! [`ProgramadorTareas`]. Partido de `persistencia_sqlite.rs`
//! (limite-lineas 1006 + nivel-2).

use async_trait::async_trait;
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::ports::{
    LogTareaEjecucion, NuevaTareaProgramada, ProgramadorTareas, TareaProgramada,
};
use glory_harness_core::HarnessResult;

use super::{a_fecha, a_uuid, ahora_rfc3339, bloquear, PersistenciaSqlite};

#[async_trait]
impl ProgramadorTareas for PersistenciaSqlite {
    async fn tarea_crear(&self, nueva: &NuevaTareaProgramada) -> HarnessResult<Uuid> {
        let id = Uuid::new_v4();
        bloquear(&self.conn)
            .execute(
                "INSERT INTO tareas (id, user_id, nombre, prompt, tipo, cron_expr, proxima_ejecucion, estado, creado_en)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pendiente', ?8)",
                params![
                    id.as_hyphenated().to_string(),
                    nueva.user_id.as_hyphenated().to_string(),
                    nueva.nombre,
                    nueva.prompt,
                    nueva.tipo,
                    nueva.cron_expr,
                    nueva.proxima_ejecucion.to_rfc3339_opts(SecondsFormat::Secs, true),
                    ahora_rfc3339(),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(id)
    }

    async fn tareas_listar(&self, user_id: Uuid) -> HarnessResult<Vec<TareaProgramada>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, nombre, prompt, tipo, cron_expr, proxima_ejecucion, estado, creado_en
                 FROM tareas WHERE user_id = ?1 ORDER BY creado_en ASC",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                Ok((
                    f.get::<_, String>(0)?,
                    f.get::<_, String>(1)?,
                    f.get::<_, String>(2)?,
                    f.get::<_, String>(3)?,
                    f.get::<_, Option<String>>(4)?,
                    f.get::<_, Option<String>>(5)?,
                    f.get::<_, String>(6)?,
                    f.get::<_, String>(7)?,
                ))
            })
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (id, nombre, prompt, tipo, cron_expr, proxima, estado, creado) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(TareaProgramada {
                id: a_uuid(id)?,
                user_id,
                nombre,
                prompt,
                tipo,
                cron_expr,
                proxima_ejecucion: match proxima {
                    Some(s) => Some(a_fecha(s)?),
                    None => None,
                },
                estado,
                creado_en: a_fecha(creado)?,
            });
        }
        Ok(out)
    }

    async fn tarea_cancelar(&self, id: Uuid, user_id: Uuid) -> HarnessResult<bool> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE tareas SET estado = 'cancelada' WHERE id = ?1 AND user_id = ?2",
                params![
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n == 1)
    }

    async fn tarea_logs(
        &self,
        id: Uuid,
        user_id: Uuid,
        limite: u32,
    ) -> HarnessResult<Vec<LogTareaEjecucion>> {
        let conn = bloquear(&self.conn);
        let es_suya: bool = conn
            .query_row(
                "SELECT 1 FROM tareas WHERE id = ?1 AND user_id = ?2",
                params![
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .unwrap_or(false);
        if !es_suya {
            return Ok(Vec::new());
        }
        let mut stmt = conn
            .prepare(
                "SELECT id, ok, resumen, ejecutada_en FROM tarea_logs
                 WHERE tarea_id = ?1 ORDER BY ejecutada_en DESC LIMIT ?2",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(
                params![id.as_hyphenated().to_string(), i64::from(limite)],
                |f| {
                    Ok((
                        f.get::<_, String>(0)?,
                        f.get::<_, i64>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                    ))
                },
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (lid, ok, resumen, ejecutada) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(LogTareaEjecucion {
                id: a_uuid(lid)?,
                tarea_id: id,
                ok: ok != 0,
                resumen,
                ejecutada_en: a_fecha(ejecutada)?,
            });
        }
        out.reverse();
        Ok(out)
    }

    async fn tarea_registrar_log(
        &self,
        id: Uuid,
        user_id: Uuid,
        ok: bool,
        resumen: &str,
    ) -> HarnessResult<()> {
        // [B3-F8a] Entrega durable del cron: solo la tarea propia recibe log
        // (misma guarda que `tarea_logs`; la ajena se ignora sin error para
        // no abortar la pasada del ejecutor por una carrera de ownership).
        let conn = bloquear(&self.conn);
        let es_suya: bool = conn
            .query_row(
                "SELECT 1 FROM tareas WHERE id = ?1 AND user_id = ?2",
                params![
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .unwrap_or(false);
        if !es_suya {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO tarea_logs (id, tarea_id, ok, resumen, ejecutada_en)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                Uuid::new_v4().as_hyphenated().to_string(),
                id.as_hyphenated().to_string(),
                if ok { 1 } else { 0 },
                resumen,
                Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
            ],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use glory_harness_core::AgentPersistence;

    #[tokio::test]
    async fn tareas_claim_atomico_y_finalizar() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let id = p
            .tarea_crear(&NuevaTareaProgramada {
                user_id: user,
                nombre: "t".into(),
                prompt: "p".into(),
                tipo: "una_vez".into(),
                cron_expr: "@once".into(),
                proxima_ejecucion: Utc::now(),
            })
            .await
            .expect("crear tarea");
        assert!(p.tarea_tomar(id).await.expect("tomar"));
        assert!(!p.tarea_tomar(id).await.expect("retomar"));
        p.tarea_finalizar(id, true, None).await.expect("finalizar");
        assert!(p
            .tareas_pendientes(10)
            .await
            .expect("pendientes")
            .is_empty());
    }

    #[tokio::test]
    async fn tarea_log_durable_solo_dueno() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let id = p
            .tarea_crear(&NuevaTareaProgramada {
                user_id: user,
                nombre: "t".into(),
                prompt: "p".into(),
                tipo: "una_vez".into(),
                cron_expr: "@once".into(),
                proxima_ejecucion: Utc::now(),
            })
            .await
            .expect("crear tarea");
        p.tarea_registrar_log(id, user, true, "resumen uno")
            .await
            .expect("registrar");
        // La ajena se ignora sin error (no aborta la pasada del ejecutor).
        p.tarea_registrar_log(id, Uuid::new_v4(), true, "ajeno")
            .await
            .expect("ajena no falla");
        let logs = p.tarea_logs(id, user, 10).await.expect("leer logs");
        assert_eq!(logs.len(), 1);
        assert!(logs[0].ok);
        assert_eq!(logs[0].resumen, "resumen uno");
        assert!(p
            .tarea_logs(id, Uuid::new_v4(), 10)
            .await
            .expect("leer ajeno")
            .is_empty());
    }
}
