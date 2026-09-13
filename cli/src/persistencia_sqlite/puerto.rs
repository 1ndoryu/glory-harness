//! Puerto `AgentPersistence` sobre `PersistenciaSqlite` ([069A-5 F5]):
//! turnos, mensajes, acciones, memoria, skills y cola del scheduler.
//! Un solo bloque `impl` (el trait no admite repartos por fichero); los
//! dominios inherentes viven en `conversaciones` / `memoria` / `tareas`.
//!
//! [139A-8 F2] (R1/R2) Cada método mueve sus argumentos a valores propios y
//! ejecuta el SQL en `con_conn` (`spawn_blocking`): rusqlite es síncrono y
//! estos `await` retenían el worker async.

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::params;
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::ports::{
    AccionAuditable, AmbitoMemoria, ColaTareas, MemoriaEntrada, MensajePersistido,
    PersistenciaAuditoria, PersistenciaMemoria, PersistenciaSkills, PersistenciaTurnos, SkillEntrada,
    SoportaAmbitos, SoportaSkills, TareaProgramadaPendiente, TurnoPersistido,
};
use glory_harness_core::{AgentPersistence, HarnessResult};

use super::{
    a_fecha, a_uuid, ahora_rfc3339, ambito_a_workspace_id, workspace_id_a_ambito,
    PersistenciaSqlite,
};

#[async_trait]
impl PersistenciaTurnos for PersistenciaSqlite {
    async fn guardar_turno(&self, turno: &TurnoPersistido) -> HarnessResult<()> {
        let turno = turno.clone();
        self.con_conn(move |conn| {
            conn.execute(
                "INSERT INTO turnos (id, conversacion_id, user_id, estado, resumen, creado_en,
                 provider, modelo, tokens_prompt, tokens_complecion, tools_ejecutadas, duracion_ms, error)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    turno.id.as_hyphenated().to_string(),
                    turno.conversacion_id.as_hyphenated().to_string(),
                    turno.user_id.as_hyphenated().to_string(),
                    turno.estado,
                    turno.resumen,
                    turno.creado_en.to_rfc3339_opts(SecondsFormat::Secs, true),
                    turno.provider,
                    turno.modelo,
                    turno.tokens_prompt as i64,
                    turno.tokens_complecion as i64,
                    turno.tools_ejecutadas as i64,
                    turno.duracion_ms as i64,
                    turno.error,
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn finalizar_turno(
        &self,
        turno_id: Uuid,
        estado_final: &str,
        resumen: Option<&str>,
    ) -> HarnessResult<()> {
        let estado_final = estado_final.to_owned();
        let resumen = resumen.map(str::to_owned);
        self.con_conn(move |conn| {
            conn.execute(
                "UPDATE turnos SET estado = ?1, resumen = ?2 WHERE id = ?3",
                params![estado_final, resumen, turno_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> HarnessResult<()> {
        let mensaje = mensaje.clone();
        self.con_conn(move |conn| {
            conn.execute(
                "INSERT INTO mensajes (id, conversacion_id, rol, contenido, creado_en)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    mensaje.id.as_hyphenated().to_string(),
                    mensaje.conversacion_id.as_hyphenated().to_string(),
                    mensaje.rol,
                    mensaje.contenido,
                    mensaje.creado_en.to_rfc3339_opts(SecondsFormat::Secs, true),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn listar_mensajes(
        &self,
        conversacion_id: Uuid,
    ) -> HarnessResult<Vec<MensajePersistido>> {
        // Sin filtros: el núcleo (`leer_mensajes`) vive en `conversaciones`.
        self.listar_mensajes_desde(conversacion_id, None, None).await
    }

    async fn conversacion_tocar(&self, conversacion_id: Uuid) -> HarnessResult<()> {
        self.con_conn(move |conn| {
            conn.execute(
                "UPDATE conversaciones SET actualizada_en = ?1 WHERE id = ?2",
                params![ahora_rfc3339(), conversacion_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl PersistenciaAuditoria for PersistenciaSqlite {
    async fn registrar_accion(&self, accion: &AccionAuditable) -> HarnessResult<()> {
        let accion = accion.clone();
        self.con_conn(move |conn| {
            conn.execute(
                "INSERT INTO acciones (turno_id, tool, ok, resumen, argumentos_json, diff, creado_en)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    accion.turno_id.as_hyphenated().to_string(),
                    accion.tool,
                    i64::from(accion.ok),
                    accion.resumen,
                    accion.argumentos_json,
                    accion.diff,
                    ahora_rfc3339(),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl PersistenciaMemoria for PersistenciaSqlite {
    async fn memoria_listar(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
    ) -> HarnessResult<Vec<MemoriaEntrada>> {
        self.con_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT clave, contenido, actualizada_en, origen, usos, ultimo_uso
                     FROM memoria WHERE user_id = ?1 AND workspace_id = ?2",
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(
                    params![
                        user_id.as_hyphenated().to_string(),
                        ambito_a_workspace_id(ambito)
                    ],
                    |f| {
                        Ok((
                            f.get::<_, String>(0)?,
                            f.get::<_, String>(1)?,
                            f.get::<_, Option<String>>(2)?,
                            f.get::<_, Option<String>>(3)?,
                            f.get::<_, i64>(4)?,
                            f.get::<_, Option<String>>(5)?,
                        ))
                    },
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut out = Vec::new();
            for fila in filas {
                let (clave, contenido, actualizada_en, origen, usos, ultimo_uso) =
                    fila.map_err(|e| Error::Persistencia(e.to_string()))?;
                // [069A-4] Filas de BDs antiguas (NULL): se tratan como nuevas
                // (fecha actual), nunca como obsoletas — el curador no poda lo
                // que no sabe fechar.
                let leida = actualizada_en
                    .filter(|s| !s.is_empty())
                    .map(a_fecha)
                    .transpose()?
                    .unwrap_or_else(Utc::now);
                let usado = ultimo_uso
                    .filter(|s| !s.is_empty())
                    .map(a_fecha)
                    .transpose()?;
                out.push(MemoriaEntrada {
                    clave,
                    contenido,
                    actualizada_en: leida,
                    origen: origen.unwrap_or_default(),
                    usos: usos.max(0) as u32,
                    ultimo_uso: usado,
                });
            }
            Ok(out)
        })
        .await
    }

    async fn memoria_upsert(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
        entrada: &MemoriaEntrada,
    ) -> HarnessResult<()> {
        let entrada = entrada.clone();
        self.con_conn(move |conn| {
            conn.execute(
                "INSERT INTO memoria
                     (user_id, workspace_id, clave, contenido, actualizada_en, origen, usos, ultimo_uso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                 ON CONFLICT(user_id, workspace_id, clave) DO UPDATE SET
                    contenido = excluded.contenido,
                    actualizada_en = excluded.actualizada_en,
                    origen = excluded.origen,
                    usos = excluded.usos,
                    ultimo_uso = excluded.ultimo_uso",
                params![
                    user_id.as_hyphenated().to_string(),
                    ambito_a_workspace_id(ambito),
                    entrada.clave,
                    entrada.contenido,
                    entrada.actualizada_en.to_rfc3339_opts(SecondsFormat::Secs, true),
                    entrada.origen,
                    entrada.usos as i64,
                    entrada
                        .ultimo_uso
                        .map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true)),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn memoria_borrar(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
        clave: &str,
    ) -> HarnessResult<()> {
        let clave = clave.to_owned();
        self.con_conn(move |conn| {
            conn.execute(
                "DELETE FROM memoria WHERE user_id = ?1 AND workspace_id = ?2 AND clave = ?3",
                params![
                    user_id.as_hyphenated().to_string(),
                    ambito_a_workspace_id(ambito),
                    clave
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl SoportaAmbitos for PersistenciaSqlite {
    /// Ámbitos con recuerdos del usuario ([109A-2]). El curador los recorre
    /// todos, así que el orden debe ser estable entre pasadas: global primero
    /// y después los proyectos por UUID. El global siempre está presente
    /// aunque no tenga recuerdos, para que el curador pueda avisar de él.
    async fn memoria_ambitos(&self, user_id: Uuid) -> HarnessResult<Vec<AmbitoMemoria>> {
        self.con_conn(move |conn| {
            let mut stmt = conn
                .prepare("SELECT DISTINCT workspace_id FROM memoria WHERE user_id = ?1")
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                    f.get::<_, String>(0)
                })
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut ambitos = Vec::new();
            for fila in filas {
                let valor = fila.map_err(|e| Error::Persistencia(e.to_string()))?;
                let ambito = workspace_id_a_ambito(&valor)?;
                if !ambitos.contains(&ambito) {
                    ambitos.push(ambito);
                }
            }
            if !ambitos.contains(&AmbitoMemoria::Global) {
                ambitos.push(AmbitoMemoria::Global);
            }
            ambitos.sort_by_key(|a| (a.proyecto_id().is_some(), a.proyecto_id()));
            Ok(ambitos)
        })
        .await
    }
}

#[async_trait]
impl PersistenciaSkills for PersistenciaSqlite {
    async fn skills_listar(&self, user_id: Uuid) -> HarnessResult<Vec<SkillEntrada>> {
        self.con_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, nombre, descripcion, instrucciones, activa FROM skills WHERE user_id = ?1",
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                    Ok((
                        f.get::<_, String>(0)?,
                        f.get::<_, String>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                        f.get::<_, i64>(4)?,
                    ))
                })
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut out = Vec::new();
            for fila in filas {
                let (id, nombre, descripcion, instrucciones, activa) =
                    fila.map_err(|e| Error::Persistencia(e.to_string()))?;
                out.push(SkillEntrada {
                    id: a_uuid(id)?,
                    nombre,
                    descripcion,
                    instrucciones,
                    activa: activa != 0,
                });
            }
            Ok(out)
        })
        .await
    }
}

#[async_trait]
impl SoportaSkills for PersistenciaSqlite {
    async fn skills_registrar(&self, user_id: Uuid, skill: &SkillEntrada) -> HarnessResult<()> {
        // [069A-4] Alta o sustitución por (user_id, nombre): el curador
        // promueve recuerdos sin duplicar skills.
        let skill = skill.clone();
        self.con_conn(move |conn| {
            conn.execute(
                "INSERT INTO skills (id, user_id, nombre, descripcion, instrucciones, activa)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(id) DO UPDATE SET
                    nombre = excluded.nombre,
                    descripcion = excluded.descripcion,
                    instrucciones = excluded.instrucciones,
                    activa = excluded.activa",
                params![
                    skill.id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string(),
                    skill.nombre,
                    skill.descripcion,
                    skill.instrucciones,
                    i64::from(skill.activa),
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }
}

#[async_trait]
impl ColaTareas for PersistenciaSqlite {
    async fn tareas_recuperar_interrumpidas(&self) -> HarnessResult<u64> {
        self.con_conn(move |conn| {
            let n = conn
                .execute(
                    "UPDATE tareas SET estado = 'pendiente' WHERE estado = 'ejecutando'",
                    [],
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(n as u64)
        })
        .await
    }

    async fn tareas_pendientes(&self, limite: u32) -> HarnessResult<Vec<TareaProgramadaPendiente>> {
        self.con_conn(move |conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT id, user_id, nombre, prompt, tipo, cron_expr, programacion FROM tareas
                     WHERE estado = 'pendiente' ORDER BY creado_en ASC LIMIT ?1",
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let filas = stmt
                .query_map(params![i64::from(limite)], |f| {
                    Ok((
                        f.get::<_, String>(0)?,
                        f.get::<_, String>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                        f.get::<_, String>(4)?,
                        f.get::<_, Option<String>>(5)?,
                        f.get::<_, String>(6)?,
                    ))
                })
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            let mut out = Vec::new();
            for fila in filas {
                let (id, user_id, nombre, prompt, tipo, cron_expr, programacion) =
                    fila.map_err(|e| Error::Persistencia(e.to_string()))?;
                out.push(TareaProgramadaPendiente {
                    id: a_uuid(id)?,
                    user_id: a_uuid(user_id)?,
                    nombre,
                    prompt,
                    tipo,
                    cron_expr,
                    /* Fila legacy (programacion '') → None: el scheduler cae al
                     * espejo `tipo`+`cron_expr` heredado. */
                    programacion: if programacion.is_empty() {
                        None
                    } else {
                        Some(programacion)
                    },
                });
            }
            Ok(out)
        })
        .await
    }

    async fn tarea_tomar(&self, id: Uuid) -> HarnessResult<bool> {
        self.con_conn(move |conn| {
            let n = conn
                .execute(
                    "UPDATE tareas SET estado = 'ejecutando' WHERE id = ?1 AND estado = 'pendiente'",
                    params![id.as_hyphenated().to_string()],
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(n == 1)
        })
        .await
    }

    async fn tarea_finalizar(
        &self,
        id: Uuid,
        ok: bool,
        _resumen: Option<&str>,
    ) -> HarnessResult<()> {
        self.con_conn(move |conn| {
            conn.execute(
                "UPDATE tareas SET estado = ?1 WHERE id = ?2",
                params![
                    if ok { "completada" } else { "pendiente" },
                    id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }

    async fn tarea_reprogramar(
        &self,
        id: Uuid,
        _user_id: Uuid,
        proxima: Option<DateTime<Utc>>,
    ) -> HarnessResult<()> {
        self.con_conn(move |conn| {
            conn.execute(
                "UPDATE tareas SET proxima_ejecucion = ?1, estado = 'pendiente' WHERE id = ?2",
                params![
                    proxima.map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true)),
                    id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(())
        })
        .await
    }
}

/// [139A-8 F4/S3-S4] Compuesto: la tienda sqlite declara ambas capacidades
/// (ámbitos por `workspace_id` + alta/sustitución de skills por id).
impl AgentPersistence for PersistenciaSqlite {
    fn como_soporta_ambitos(&self) -> Option<&dyn SoportaAmbitos> {
        Some(self)
    }
    fn como_soporta_skills(&self) -> Option<&dyn SoportaSkills> {
        Some(self)
    }
}
