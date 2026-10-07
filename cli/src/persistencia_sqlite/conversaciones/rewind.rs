//! Rewind de conversaciones (parte de `super`): rebobina hasta un mensaje de
//! usuario en una transacción (mensajes, turnos y acciones del tramo).
//!
//! Sin cambio de comportamiento: el bloque `impl` vivía en `mod.rs`
//! (partición 309A-3; el archivo superaba el límite + god-object).

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::super::{bloquear, PersistenciaSqlite};

impl PersistenciaSqlite {
    /// [039A-3 P2] Rebobina una conversaci├│n hasta un mensaje de usuario.
    ///
    /// Borra, en una transacci├│n, el tramo posterior a `hasta_mensaje_id`
    /// (mensajes, turnos y las acciones de esos turnos). Con `editar=true`
    /// borra tambi├®n el propio mensaje objetivo para reescribirlo; con
    /// `editar=false` (volver a punto) lo conserva como ├║ltimo mensaje.
    ///
    /// [039A-3 P3] Devuelve los `turno_id` borrados (los del tramo) para que
    /// el consumidor (vault del desktop) pueda ofrecer "restaurar archivos de
    /// este tramo" sin depender de la BD ya borrada. El hook de respaldo del
    /// sandbox registra cada escritura con su `turno_id` en el log del vault;
    /// con estos ids el vault sabe qu├® entradas corresponden al tramo.
    ///
    /// Anclaje del borrado:
    /// - Mensajes: `rowid` impl├¡cito (orden de inserci├│n estricto), a prueba
    ///   de timestamps con precisi├│n de 1 s. El mensaje objetivo debe ser de
    ///   rol `user`.
    /// - Turnos y sus acciones: `creado_en` del turno >= al del mensaje
    ///   objetivo. El turno que responde a un mensaje se persiste SIEMPRE
    ///   despu├®s (o en el mismo segundo) de que ese mensaje lleg├│, y el turno
    ///   anterior termin├│ antes de que el usuario escribiera el siguiente
    ///   mensaje: el `>=` borra el turno del propio mensaje objetivo (el
    ///   "hilo" de ese punto) sin alcanzar al turno previo.
    ///
    /// Falla (sin borrado parcial) si la conversaci├│n no es del `user_id` o
    /// el mensaje objetivo no existe en ella o no es de rol `user`.
    pub fn rewind_conversacion(
        &self,
        conversacion_id: Uuid,
        hasta_mensaje_id: Uuid,
        user_id: Uuid,
        editar: bool,
    ) -> HarnessResult<Vec<Uuid>> {
        let mut conn = bloquear(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let conv_s = conversacion_id.as_hyphenated().to_string();
        let msg_s = hasta_mensaje_id.as_hyphenated().to_string();
        let user_s = user_id.as_hyphenated().to_string();

        // Propiedad de la conversaci├│n + existencia del mensaje objetivo
        // (rol user). Si falta, error expl├¡cito: nunca borrado parcial mudo.
        let punto: Option<(i64, String)> = tx
            .query_row(
                "SELECT m.rowid, m.creado_en FROM mensajes m
                 JOIN conversaciones c ON c.id = m.conversacion_id
                 WHERE m.id = ?1 AND m.conversacion_id = ?2 AND m.rol = 'user'
                   AND c.user_id = ?3",
                params![msg_s, conv_s, user_s],
                |f| Ok((f.get(0)?, f.get(1)?)),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let (rowid_punto, creado_punto) = punto.ok_or_else(|| {
            Error::Persistencia(
                "mensaje objetivo no encontrado, no es de usuario o conversaci├│n ajena".into(),
            )
        })?;

        /* [039A-3 P3] Turnos del tramo ANTES de borrarlos: los ids que el
         * vault usar├í para localizar los respaldos de este tramo. */
        let turnos_tramo: Vec<Uuid> = {
            let mut stmt = tx
                .prepare("SELECT id FROM turnos WHERE conversacion_id = ?1 AND creado_en >= ?2")
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(params![conv_s, creado_punto], |f| f.get::<_, String>(0))
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut ids = Vec::new();
            for fila in filas {
                let s = fila.map_err(|e| Error::Persistencia(e.to_string()))?;
                if let Ok(id) = Uuid::parse_str(&s) {
                    ids.push(id);
                }
            }
            ids
        };

        // Acciones de los turnos del tramo (turnos posteriores al mensaje).
        tx.execute(
            "DELETE FROM acciones WHERE turno_id IN (
                 SELECT id FROM turnos WHERE conversacion_id = ?1 AND creado_en >= ?2
             )",
            params![conv_s, creado_punto],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        // Turnos del tramo (>= borra tambi├®n el turno que responde al propio
        // mensaje objetivo: su `creado_en` es posterior o igual al del user).
        tx.execute(
            "DELETE FROM turnos WHERE conversacion_id = ?1 AND creado_en >= ?2",
            params![conv_s, creado_punto],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        // Mensajes del tramo (rowid estricto; `>=` borra el objetivo al editar).
        let operador = if editar { ">=" } else { ">" };
        let sql =
            format!("DELETE FROM mensajes WHERE conversacion_id = ?1 AND rowid {operador} ?2");
        tx.execute(&sql, params![conv_s, rowid_punto])
            .map_err(|e| Error::Persistencia(e.to_string()))?;

        tx.commit()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(turnos_tramo)
    }
}
