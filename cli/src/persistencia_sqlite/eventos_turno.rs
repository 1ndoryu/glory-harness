//! Log de eventos por turno ([129A-4 F1]): cada evento del contrato
//! `AgenteEvento` que el desktop reenvía a la UI queda también en SQLite,
//! anclado al `turno_id`. Sin esto, un turno atascado
//! (`aprobada · ejecutando…`) no deja rastro consultable.
//!
//! Diseño:
//! - `eventos_turno`: una fila por evento (se omiten `token` y
//!   `razonamiento_delta`: volumen alto y el texto ya vive en `mensajes`).
//! - `peticion_turno`: vínculo petición→turno para atribuir la RESPUESTA de
//!   aprobación (que viaja por comando, fuera del canal de eventos) al turno
//!   que la esperaba.
//! - Todo fuera del trait `AgentPersistence` del núcleo (como `config_*`):
//!   el core no cambia y web/CLI pueden adoptar estos métodos después.

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::{a_uuid, ahora_rfc3339, bloquear, PersistenciaSqlite};

/// Evento de un turno recuperado para el visor (`log_turno`).
/// El `payload_json` es el JSON íntegro del `AgenteEvento` (tag `tipo`
/// snake_case); el front lo interpreta, el backend no necesita el tipo.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EventoTurnoRegistrado {
    pub id: i64,
    pub turno_id: Uuid,
    pub tipo: String,
    pub payload_json: String,
    pub creado_en: String,
}

impl PersistenciaSqlite {
    /// Registra un evento del turno (best-effort desde el reenvío: el
    /// llamante decide si un fallo tumba o solo avisa).
    pub fn evento_turno_registrar(
        &self,
        turno_id: Uuid,
        tipo: &str,
        payload_json: &str,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "INSERT INTO eventos_turno (turno_id, tipo, payload_json, creado_en)
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    turno_id.as_hyphenated().to_string(),
                    tipo,
                    payload_json,
                    ahora_rfc3339()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    /// Eventos del turno en orden de emisión (para `log_turno`).
    pub fn eventos_turno_listar(
        &self,
        turno_id: Uuid,
    ) -> HarnessResult<Vec<EventoTurnoRegistrado>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, turno_id, tipo, payload_json, creado_en FROM eventos_turno
                 WHERE turno_id = ?1 ORDER BY id",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(
                params![turno_id.as_hyphenated().to_string()],
                |f| {
                    Ok((
                        f.get::<_, i64>(0)?,
                        f.get::<_, String>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                        f.get::<_, String>(4)?,
                    ))
                },
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (id, turno, tipo, payload, creado) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(EventoTurnoRegistrado {
                id,
                turno_id: a_uuid(turno)?,
                tipo,
                payload_json: payload,
                creado_en: creado,
            });
        }
        Ok(out)
    }

    /// Vincula una petición de aprobación con el turno que la emitió
    /// (para atribuir la respuesta, que llega por otro canal).
    pub fn peticion_turno_vincular(
        &self,
        peticion_id: &str,
        turno_id: Uuid,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "INSERT INTO peticion_turno (peticion_id, turno_id)
                 VALUES (?1, ?2)
                 ON CONFLICT(peticion_id) DO UPDATE SET turno_id = excluded.turno_id",
                params![peticion_id, turno_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    /// Turno que emitió una petición (`None` = petición desconocida: la
    /// respuesta a ese id debe hacer ruido, no perderse en silencio).
    pub fn peticion_turno_de(&self, peticion_id: &str) -> HarnessResult<Option<Uuid>> {
        let turno: Option<String> = bloquear(&self.conn)
            .query_row(
                "SELECT turno_id FROM peticion_turno WHERE peticion_id = ?1",
                params![peticion_id],
                |f| f.get(0),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        turno.map(a_uuid).transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /* [129A-4 F1] El log por turno es la base de la observabilidad: si el
     * roundtrip (registrar → listar en orden) o el vínculo petición→turno
     * se rompe, `log_turno` miente y volvemos a ir a ciegas. */
    #[test]
    fn eventos_turno_roundtrip_en_orden() {
        let bd = PersistenciaSqlite::en_memoria().expect("bd");
        let turno = Uuid::new_v4();
        bd.evento_turno_registrar(turno, "tool_start", "{\"tipo\":\"tool_start\"}")
            .expect("registra 1");
        bd.evento_turno_registrar(turno, "tool_result", "{\"tipo\":\"tool_result\"}")
            .expect("registra 2");
        // Otro turno no contamina el listado.
        bd.evento_turno_registrar(Uuid::new_v4(), "token", "{}")
            .expect("registra otro");
        let eventos = bd.eventos_turno_listar(turno).expect("lista");
        assert_eq!(eventos.len(), 2);
        assert_eq!(eventos[0].tipo, "tool_start");
        assert_eq!(eventos[1].tipo, "tool_result");
        assert_eq!(eventos[0].turno_id, turno);
        assert!(eventos[0].id < eventos[1].id);
        assert!(!eventos[0].creado_en.is_empty());
    }

    #[test]
    fn peticion_turno_vinculo_y_desconocida() {
        let bd = PersistenciaSqlite::en_memoria().expect("bd");
        assert_eq!(bd.peticion_turno_de("inexistente").expect("lee"), None);
        let turno = Uuid::new_v4();
        bd.peticion_turno_vincular("pet-1", turno).expect("vincula");
        assert_eq!(
            bd.peticion_turno_de("pet-1").expect("lee"),
            Some(turno)
        );
    }
}
