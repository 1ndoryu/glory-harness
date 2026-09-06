//! Memoria y skills de `PersistenciaSqlite` ([069A-5 F5], dominio de
//! [069A-4]): recuerdos con metadatos de curaduría + registro de skills.
//! Partido de `persistencia_sqlite.rs` (limite-lineas 1006 + nivel-2).

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::{bloquear, PersistenciaSqlite};

impl PersistenciaSqlite {
    /// Skills base (paridad con `PersistenciaMemoria::con_skills_base`).
    pub fn con_skills_base(&self, user_id: Uuid) -> &Self {
        let conn = bloquear(&self.conn);
        let existe: HarnessResult<bool> = conn
            .query_row(
                "SELECT 1 FROM skills WHERE user_id = ?1 AND nombre = 'resumen'",
                params![user_id.as_hyphenated().to_string()],
                |_| Ok(true),
            )
            .optional()
            .map(|o| o.unwrap_or(false))
            .map_err(|e| Error::Persistencia(e.to_string()));
        if !existe.unwrap_or(true) {
            let _ = conn.execute(
                "INSERT INTO skills (id, user_id, nombre, descripcion, instrucciones, activa)
                 VALUES (?1, ?2, 'resumen', 'Resume en 3 vi├▒etas', 'Al terminar, resume tu respuesta en 3 vi├▒etas concisas.', 1)",
                params![Uuid::new_v4().as_hyphenated().to_string(), user_id.as_hyphenated().to_string()],
            );
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glory_harness_core::AgentPersistence;

    #[tokio::test]
    async fn skills_base_idempotente() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        p.con_skills_base(user);
        p.con_skills_base(user);
        let skills = p.skills_listar(user).await.expect("skills");
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].nombre, "resumen");
    }
}
