//! Batch de hilos por área de trabajo ([119A-2 F4]): archivar/desarchivar y
//! eliminar todas las conversaciones de un proyecto. Partido de `mod.rs`
//! (limite-lineas: el archivo superó 500 líneas efectivas para servicio).

use rusqlite::params;
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::{a_uuid, ahora_rfc3339, bloquear, PersistenciaSqlite};

impl PersistenciaSqlite {
    /// [119A-2 F4] Archiva/desarchiva TODAS las conversaciones de un área
    /// (solo las del usuario). Devuelve cuántas filas cambió. Reversible
    /// hilo a hilo con `conversacion_archivar`.
    pub fn conversaciones_archivar_por_workspace(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        archivada: bool,
    ) -> HarnessResult<usize> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE conversaciones SET archivada = ?1, actualizada_en = ?2
                  WHERE user_id = ?3 AND workspace_id = ?4",
                params![
                    i64::from(archivada),
                    ahora_rfc3339(),
                    user_id.as_hyphenated().to_string(),
                    workspace_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n)
    }

    /// [119A-2 F4] Elimina TODAS las conversaciones de un área con sus
    /// mensajes y turnos (transacción espejo de `conversacion_eliminar`).
    /// Devuelve los ids borrados (el desktop limpia el vault por hilo).
    pub fn conversaciones_eliminar_por_workspace(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> HarnessResult<Vec<Uuid>> {
        let user_s = user_id.as_hyphenated().to_string();
        let ws_s = workspace_id.as_hyphenated().to_string();
        // SELECT previo sobre `conn` (no sobre `tx`): `prepare`+`query_map`
        // con `stmt` de vida corta dentro de la transacción no compila
        // (E0597). Los borrados sí van en `tx` (todo o nada).
        let ids: Vec<String> = {
            let conn = bloquear(&self.conn);
            let mut stmt = conn
                .prepare("SELECT id FROM conversaciones WHERE user_id = ?1 AND workspace_id = ?2")
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(params![user_s, ws_s], |f| f.get::<_, String>(0))
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut ids = Vec::new();
            for fila in filas {
                ids.push(fila.map_err(|e| Error::Persistencia(e.to_string()))?);
            }
            ids
        };
        let mut conn = bloquear(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        for id_s in &ids {
            tx.execute("DELETE FROM mensajes WHERE conversacion_id = ?1", params![id_s])
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            tx.execute(
                "DELETE FROM acciones WHERE turno_id IN (SELECT id FROM turnos WHERE conversacion_id = ?1)",
                params![id_s],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            tx.execute("DELETE FROM turnos WHERE conversacion_id = ?1", params![id_s])
                .map_err(|e| Error::Persistencia(e.to_string()))?;
        }
        tx.execute(
            "DELETE FROM conversaciones WHERE user_id = ?1 AND workspace_id = ?2",
            params![user_s, ws_s],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        tx.commit()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::with_capacity(ids.len());
        for s in ids {
            out.push(a_uuid(s)?);
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// [119A-2 F4] El batch por área archiva/elimina solo sus hilos; el
    /// resto de áreas queda intacto.
    #[tokio::test]
    async fn conversaciones_batch_por_workspace() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let ws_a = p
            .workspace_crear(user, "Área A", "C:\\tmp\\proyecto-a-batch")
            .expect("crear A")
            .id;
        let ws_b = p
            .workspace_crear(user, "Área B", "C:\\tmp\\proyecto-b-batch")
            .expect("crear B")
            .id;
        let a1 = p
            .conversacion_crear_en(user, "a-uno", Some(ws_a))
            .expect("crear a1");
        let a2 = p
            .conversacion_crear_en(user, "a-dos", Some(ws_a))
            .expect("crear a2");
        let b1 = p
            .conversacion_crear_en(user, "b-uno", Some(ws_b))
            .expect("crear b1");
        // Archivar el área A oculta sus dos hilos y no toca B.
        assert_eq!(
            p.conversaciones_archivar_por_workspace(user, ws_a, true)
                .expect("archivar A"),
            2
        );
        let lista = p.conversaciones_listar(user).expect("listar");
        assert!(lista.iter().find(|c| c.id == a1).expect("a1").archivada);
        assert!(lista.iter().find(|c| c.id == a2).expect("a2").archivada);
        assert!(!lista.iter().find(|c| c.id == b1).expect("b1").archivada);
        // Desarchivar revierte.
        assert_eq!(
            p.conversaciones_archivar_por_workspace(user, ws_a, false)
                .expect("desarchivar A"),
            2
        );
        // Eliminar el área A borra sus hilos; B queda intacta.
        let borradas = p
            .conversaciones_eliminar_por_workspace(user, ws_a)
            .expect("eliminar A");
        assert_eq!(borradas.len(), 2);
        assert!(borradas.contains(&a1) && borradas.contains(&a2));
        let resto = p.conversaciones_listar(user).expect("listar resto");
        assert_eq!(resto.len(), 1);
        assert_eq!(resto[0].id, b1);
        // Repetir sobre un área vacía no falla ni toca nada.
        assert!(
            p.conversaciones_eliminar_por_workspace(user, ws_a)
                .expect("repetir")
                .is_empty()
        );
    }
}
