//! Chats durables de `PersistenciaSqlite` ([069A-5 F5]): conversaciones,
//! turnos, mensajes, acciones y rewind. Partido de `persistencia_sqlite.rs`
//! (limite-lineas 1006 + nivel-2).

use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::ports::{AccionAuditable, MensajePersistido, TurnoPersistido};
use glory_harness_core::HarnessResult;

use super::{
    a_fecha, a_uuid, ahora_rfc3339, bloquear, AccionRecuperada, InfoConversacion,
    PersistenciaSqlite,
};

impl PersistenciaSqlite {
    // --- CRUD de conversaciones (inherente: no forma parte del trait) ---

    /// Crea una conversaci├│n y devuelve su id.
    pub fn conversacion_crear(&self, user_id: Uuid, titulo: &str) -> HarnessResult<Uuid> {
        let id = Uuid::new_v4();
        let ahora = ahora_rfc3339();
        bloquear(&self.conn)
            .execute(
                "INSERT INTO conversaciones (id, user_id, titulo, archivada, creada_en, actualizada_en)
                 VALUES (?1, ?2, ?3, 0, ?4, ?4)",
                params![
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string(),
                    titulo,
                    ahora
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(id)
    }

    /// Lista las conversaciones del usuario (recientes primero).
    pub fn conversaciones_listar(&self, user_id: Uuid) -> HarnessResult<Vec<InfoConversacion>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, titulo, archivada, actualizada_en FROM conversaciones
                 WHERE user_id = ?1 ORDER BY actualizada_en DESC",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                Ok((
                    f.get::<_, String>(0)?,
                    f.get::<_, String>(1)?,
                    f.get::<_, i64>(2)?,
                    f.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (id, titulo, archivada, actualizada) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(InfoConversacion {
                id: a_uuid(id)?,
                titulo,
                archivada: archivada != 0,
                actualizada_en: a_fecha(actualizada)?,
            });
        }
        Ok(out)
    }

    /// Renombra (solo si es del usuario); `false` si no existe o no es suya.
    pub fn conversacion_renombrar(
        &self,
        id: Uuid,
        user_id: Uuid,
        titulo: &str,
    ) -> HarnessResult<bool> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE conversaciones SET titulo = ?1, actualizada_en = ?2 WHERE id = ?3 AND user_id = ?4",
                params![
                    titulo,
                    ahora_rfc3339(),
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n == 1)
    }

    /// Archiva/desarchiva (solo si es del usuario).
    pub fn conversacion_archivar(
        &self,
        id: Uuid,
        user_id: Uuid,
        archivada: bool,
    ) -> HarnessResult<bool> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE conversaciones SET archivada = ?1, actualizada_en = ?2 WHERE id = ?3 AND user_id = ?4",
                params![
                    i64::from(archivada),
                    ahora_rfc3339(),
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n == 1)
    }

    /// Elimina la conversaci├│n con sus mensajes y turnos (transacci├│n).
    pub fn conversacion_eliminar(&self, id: Uuid, user_id: Uuid) -> HarnessResult<bool> {
        let mut conn = bloquear(&self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let id_s = id.as_hyphenated().to_string();
        tx.execute(
            "DELETE FROM mensajes WHERE conversacion_id = ?1",
            params![id_s],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        tx.execute(
            "DELETE FROM acciones WHERE turno_id IN (SELECT id FROM turnos WHERE conversacion_id = ?1)",
            params![id_s],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        tx.execute(
            "DELETE FROM turnos WHERE conversacion_id = ?1",
            params![id_s],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        let n = tx
            .execute(
                "DELETE FROM conversaciones WHERE id = ?1 AND user_id = ?2",
                params![id_s, user_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        tx.commit()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n == 1)
    }

    /// Acciones (tools ejecutadas) de una conversaci├│n en orden de ejecuci├│n.
    /// El orden se ancla en el `creado_en` del TURNO al que pertenece cada
    /// acci├│n (las acciones no tienen timestamp fiable de UI; el JOIN da el
    /// orden con una sola consulta). [039A-1 04-09 H6]
    pub fn acciones_por_conversacion(
        &self,
        conversacion_id: Uuid,
    ) -> HarnessResult<Vec<AccionRecuperada>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT a.tool, a.ok, a.resumen, a.argumentos_json, a.diff, t.creado_en
                 FROM acciones a
                 JOIN turnos t ON t.id = a.turno_id
                 WHERE t.conversacion_id = ?1
                 ORDER BY t.creado_en, a.id",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![conversacion_id.as_hyphenated().to_string()], |f| {
                Ok(AccionRecuperada {
                    tool: f.get::<_, String>(0)?,
                    ok: f.get::<_, i64>(1)? != 0,
                    resumen: f.get::<_, String>(2)?,
                    argumentos_json: f.get::<_, Option<String>>(3)?,
                    diff: f.get::<_, Option<String>>(4)?,
                    turno_en: f.get::<_, String>(5)?,
                })
            })
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            out.push(fila.map_err(|e| Error::Persistencia(e.to_string()))?);
        }
        Ok(out)
    }

    /// [039A-3 P1] M├®tricas reales del ├ÜLTIMO turno de una conversaci├│n, para
    /// repintar el pie de turno al cargar. `None` si no hay turnos o si el
    /// turno no registr├│ uso real (los tokens quedan 0 y el modelo el
    /// solicitado). El turno m├ís reciente es el de `creado_en` mayor; los
    /// `id` son UUID (orden aleatorio), as├¡ que el orden se ancla en el
    /// timestamp del turno.
    #[allow(clippy::type_complexity)]
    pub fn turno_ultimo_uso_por_conversacion(
        &self,
        conversacion_id: Uuid,
    ) -> HarnessResult<Option<(String, String, u32, u32)>> {
        let conn = bloquear(&self.conn);
        let fila = conn
            .query_row(
                "SELECT provider, modelo, tokens_prompt, tokens_complecion
                 FROM turnos WHERE conversacion_id = ?1
                 ORDER BY creado_en DESC LIMIT 1",
                params![conversacion_id.as_hyphenated().to_string()],
                |f| {
                    Ok((
                        f.get::<_, Option<String>>(0)?,
                        f.get::<_, Option<String>>(1)?,
                        f.get::<_, i64>(2)?,
                        f.get::<_, i64>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(fila.map(|(provider, modelo, tp, tc)| {
            (
                provider.unwrap_or_default(),
                modelo.unwrap_or_default(),
                tp.max(0) as u32,
                tc.max(0) as u32,
            )
        }))
    }
    /// [039A-3 P1] Persiste el uso/modelo REAL de un turno terminado.
    ///
    /// El runtime guarda el turno con `tokens_prompt/complecion = 0` y el
    /// provider/modelo SOLICITADO (no el que respondi├│ tras fallback); el
    /// `AgenteEvento::Usage` real viaja transitorio por el canal del turno.
    /// El backend de Tauri acumula esos Usage parciales (un turno con N
    /// tools emite N Usage) y, al `turno-fin` ok, llama a este m├®todo para
    /// rellenar las columnas reales. Solo se actualizan campos SIEMPRE
    /// acumulados: los tokens se SUMAN; provider/modelo se conservan los del
    /// ├║ltimo Usage (el que respondi├│ de verdad).
    pub fn turno_actualizar_uso(
        &self,
        turno_id: Uuid,
        tokens_prompt: u32,
        tokens_complecion: u32,
        provider: Option<&str>,
        modelo: Option<&str>,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "UPDATE turnos SET tokens_prompt = ?1, tokens_complecion = ?2,
                 provider = ?3, modelo = ?4 WHERE id = ?5",
                params![
                    i64::from(tokens_prompt),
                    i64::from(tokens_complecion),
                    provider,
                    modelo,
                    turno_id.as_hyphenated().to_string(),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use glory_harness_core::AgentPersistence;
    use std::path::PathBuf;

    /// Ruta temporal ├║nica para la BD de un test (se borra al terminar).
    fn ruta_temp(nombre: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "gh-sqlite-test-{}-{}-{nombre}.db",
            std::process::id(),
            Utc::now().timestamp_millis()
        ))
    }

    #[tokio::test]
    async fn mensajes_ordenados_y_reapertura_conserva() {
        let ruta = ruta_temp("mensajes");
        let user = Uuid::new_v4();
        let conv = {
            let p = PersistenciaSqlite::abrir(&ruta).expect("abrir BD");
            let conv = p
                .conversacion_crear(user, "prueba")
                .expect("crear conversaci├│n");
            for (rol, texto) in [("user", "hola"), ("assistant", "buenas")] {
                p.guardar_mensaje(&MensajePersistido {
                    id: Uuid::new_v4(),
                    conversacion_id: conv,
                    rol: rol.into(),
                    contenido: texto.into(),
                    creado_en: Utc::now(),
                })
                .await
                .expect("guardar mensaje");
            }
            conv
        };
        // Reabrir: el historial sobrevive al proceso.
        let p2 = PersistenciaSqlite::abrir(&ruta).expect("reabrir BD");
        let mensajes = p2.listar_mensajes(conv).await.expect("listar");
        assert_eq!(mensajes.len(), 2);
        assert_eq!(mensajes[0].rol, "user");
        assert_eq!(mensajes[1].rol, "assistant");
        let _ = std::fs::remove_file(&ruta);
    }

    #[tokio::test]
    async fn conversaciones_crud_y_config() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let id = p.conversacion_crear(user, "una").expect("crear");
        assert!(p
            .conversacion_renombrar(id, user, "una-dos")
            .expect("renombrar"));
        assert!(!p
            .conversacion_renombrar(id, Uuid::new_v4(), "ajena")
            .expect("renombrar ajena"));
        assert!(p.conversacion_archivar(id, user, true).expect("archivar"));
        let lista = p.conversaciones_listar(user).expect("listar");
        assert_eq!(lista.len(), 1);
        assert_eq!(lista[0].titulo, "una-dos");
        assert!(lista[0].archivada);
        assert!(p.config_leer("modo").expect("leer").is_none());
        p.config_guardar("modo", "autonomo").expect("guardar");
        assert_eq!(
            p.config_leer("modo").expect("releer").as_deref(),
            Some("autonomo")
        );
        assert!(p.conversacion_eliminar(id, user).expect("eliminar"));
        assert!(p.conversaciones_listar(user).expect("listar2").is_empty());
    }

    #[tokio::test]
    async fn turno_actualizar_uso_rellena_tokens_y_modelo_real() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let conv = p.conversacion_crear(user, "uso").expect("crear");
        let turno = Uuid::new_v4();
        // El runtime guarda el turno con tokens 0 y el modelo SOLICITADO.
        p.guardar_turno(&TurnoPersistido {
            id: turno,
            conversacion_id: conv,
            user_id: user,
            estado: "ok".into(),
            resumen: None,
            creado_en: Utc::now(),
            provider: Some("commandcode".into()),
            modelo: Some("command-r-plus".into()),
            tokens_prompt: 0,
            tokens_complecion: 0,
            tools_ejecutadas: 2,
            duracion_ms: 1200,
            error: None,
        })
        .await
        .expect("guardar turno");
        // El backend acumula el uso REAL tras fallback y lo persiste.
        p.turno_actualizar_uso(turno, 5120, 640, Some("glory"), Some("gpt-4.1"))
            .expect("actualizar uso");
        // Releer por SQL directo: el m├®todo es inherente y no expone lector.
        let conn = bloquear(&p.conn);
        let (tokens_p, tokens_c, provider, modelo): (i64, i64, Option<String>, Option<String>) =
            conn.query_row(
                "SELECT tokens_prompt, tokens_complecion, provider, modelo FROM turnos WHERE id = ?1",
                params![turno.as_hyphenated().to_string()],
                |f| Ok((f.get(0)?, f.get(1)?, f.get(2)?, f.get(3)?)),
            )
            .expect("leer turno");
        assert_eq!(tokens_p, 5120);
        assert_eq!(tokens_c, 640);
        assert_eq!(provider.as_deref(), Some("glory"));
        assert_eq!(modelo.as_deref(), Some("gpt-4.1"));
    }

    #[tokio::test]
    async fn rewind_conserva_y_edita_tramo_posterior() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let conv = p.conversacion_crear(user, "rewind").expect("crear");
        // Timestamps crecientes (precisi├│n de BD = 1 s): el turno que responde
        // a un user SIEMPRE se persiste despu├®s de que ese user lleg├│ y antes
        // de su assistant (flujo real del runtime).
        let base = Utc::now();
        let t = |s: i64| base + chrono::Duration::seconds(s);
        let u1 = Uuid::new_v4();
        let t1 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: u1,
            conversacion_id: conv,
            rol: "user".into(),
            contenido: "pregunta 1".into(),
            creado_en: t(0),
        })
        .await
        .expect("user 1");
        p.guardar_turno(&TurnoPersistido {
            id: t1,
            conversacion_id: conv,
            user_id: user,
            estado: "ok".into(),
            resumen: Some("resumen 1".into()),
            creado_en: t(10),
            provider: Some("glory".into()),
            modelo: Some("gpt-4.1".into()),
            tokens_prompt: 100,
            tokens_complecion: 50,
            tools_ejecutadas: 1,
            duracion_ms: 1000,
            error: None,
        })
        .await
        .expect("turno 1");
        p.registrar_accion(&AccionAuditable {
            turno_id: t1,
            tool: "leer".into(),
            ok: true,
            resumen: "ley├│".into(),
            argumentos_json: None,
            diff: None,
        })
        .await
        .expect("acci├│n 1");
        let a1 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: a1,
            conversacion_id: conv,
            rol: "assistant".into(),
            contenido: "respuesta 1".into(),
            creado_en: t(11),
        })
        .await
        .expect("assistant 1");

        // Turno 2: otro ciclo completo.
        let u2 = Uuid::new_v4();
        let t2 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: u2,
            conversacion_id: conv,
            rol: "user".into(),
            contenido: "pregunta 2".into(),
            creado_en: t(20),
        })
        .await
        .expect("user 2");
        p.guardar_turno(&TurnoPersistido {
            id: t2,
            conversacion_id: conv,
            user_id: user,
            estado: "ok".into(),
            resumen: Some("resumen 2".into()),
            creado_en: t(30),
            provider: Some("glory".into()),
            modelo: Some("gpt-4.1".into()),
            tokens_prompt: 200,
            tokens_complecion: 100,
            tools_ejecutadas: 1,
            duracion_ms: 2000,
            error: None,
        })
        .await
        .expect("turno 2");
        p.registrar_accion(&AccionAuditable {
            turno_id: t2,
            tool: "editar".into(),
            ok: true,
            resumen: "edit├│".into(),
            argumentos_json: None,
            diff: None,
        })
        .await
        .expect("acci├│n 2");
        let a2 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: a2,
            conversacion_id: conv,
            rol: "assistant".into(),
            contenido: "respuesta 2".into(),
            creado_en: t(31),
        })
        .await
        .expect("assistant 2");

        // volver a punto = conservar el user objetivo (u1) y borrar su
        // assistant + el turno 2 completo.
        p.rewind_conversacion(conv, u1, user, false)
            .expect("volver a punto");
        let mensajes = p.listar_mensajes(conv).await.expect("listar");
        assert_eq!(mensajes.len(), 1);
        assert_eq!(mensajes[0].id, u1);
        assert_eq!(mensajes[0].rol, "user");
        // Turnos: solo queda el anterior al mensaje objetivo (ninguno aqu├¡).
        // El guard se suelta al salir del bloque: nunca cruzar un await con un
        // MutexGuard de `p.conn` retenido (deadlock en runtime current_thread).
        {
            let conn = bloquear(&p.conn);
            let turnos: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM turnos WHERE conversacion_id = ?1",
                    params![conv.as_hyphenated().to_string()],
                    |f| f.get(0),
                )
                .expect("contar turnos");
            assert_eq!(turnos, 0);
            let acciones: i64 = conn
                .query_row("SELECT COUNT(*) FROM acciones", params![], |f| f.get(0))
                .expect("contar acciones");
            assert_eq!(acciones, 0);
        }

        // editar = borrar el propio user objetivo (u3) para reescribirlo,
        // conservando los mensajes ANTERIORES al punto (u1 sigue ah├¡: volver
        // a un punto conserv├│ su mensaje y editar u3 solo recorta desde u3).
        let u3 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: u3,
            conversacion_id: conv,
            rol: "user".into(),
            contenido: "pregunta 3".into(),
            creado_en: t(40),
        })
        .await
        .expect("user 3");
        p.rewind_conversacion(conv, u3, user, true).expect("editar");
        let mensajes2 = p.listar_mensajes(conv).await.expect("listar 2");
        assert_eq!(mensajes2.len(), 1);
        assert_eq!(mensajes2[0].id, u1, "editar conserva lo anterior al punto");
        assert_eq!(mensajes2[0].rol, "user");
    }

    #[tokio::test]
    async fn rewind_rechaza_ajeno_o_no_usuario() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let otro = Uuid::new_v4();
        let conv = p.conversacion_crear(user, "rewind").expect("crear");
        let u1 = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: u1,
            conversacion_id: conv,
            rol: "user".into(),
            contenido: "p".into(),
            creado_en: Utc::now(),
        })
        .await
        .expect("user");

        // Mensaje de conversaci├│n ajena.
        assert!(p
            .rewind_conversacion(conv, Uuid::new_v4(), user, false)
            .is_err());
        // Conversaci├│n de otro usuario.
        assert!(p.rewind_conversacion(conv, u1, otro, false).is_err());
        // Mensaje objetivo que no es de rol user: se inserta un assistant.
        let asis = Uuid::new_v4();
        p.guardar_mensaje(&MensajePersistido {
            id: asis,
            conversacion_id: conv,
            rol: "assistant".into(),
            contenido: "r".into(),
            creado_en: Utc::now(),
        })
        .await
        .expect("assistant");
        assert!(p.rewind_conversacion(conv, asis, user, false).is_err());
        // Nada se borr├│ en ning├║n caso (transacciones fallidas).
        let mensajes = p.listar_mensajes(conv).await.expect("listar");
        assert_eq!(mensajes.len(), 2);
    }
}
