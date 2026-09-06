//! Áreas de trabajo (workspaces) de `PersistenciaSqlite` ([069A-Proyectos]):
//! nombre visible asignado por el usuario + carpeta raíz única (ruta
//! absoluta). Cada conversación pertenece a UNA área vía `workspace_id`
//! (columna en `conversaciones`; NULL = conversación sin área, el
//! comportamiento previo a la feature). Partido de
//! `persistencia_sqlite.rs` (limite-lineas + nivel-2, patrón memoria.rs).

use chrono::{DateTime, Utc};
use rusqlite::{params, OptionalExtension};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

use super::{a_fecha, a_uuid, ahora_rfc3339, bloquear, PersistenciaSqlite};

/// Vista de un área de trabajo para la sidebar (Tauri/web la serializan).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Workspace {
    pub id: Uuid,
    pub nombre: String,
    pub ruta: String,
    pub creada_en: DateTime<Utc>,
}

impl PersistenciaSqlite {
    /// Crea un área de trabajo. `ruta` debe ser absoluta (el llamador la
    /// valida); el UNIQUE sobre `ruta` garantiza una sola área por carpeta.
    /// Falla con error claro si ya existe un área con esa ruta (el llamador
    /// decide si renombra/activa la existente vía `workspace_por_ruta`).
    pub fn workspace_crear(&self, user_id: Uuid, nombre: &str, ruta: &str) -> HarnessResult<Workspace> {
        let id = Uuid::new_v4();
        let ahora = ahora_rfc3339();
        bloquear(&self.conn)
            .execute(
                "INSERT INTO workspaces (id, user_id, nombre, ruta, creada_en)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string(),
                    nombre.trim(),
                    ruta,
                    ahora
                ],
            )
            .map_err(|e| {
                if e.to_string().contains("UNIQUE") {
                    Error::Persistencia("ya existe un área de trabajo con esa carpeta".into())
                } else {
                    Error::Persistencia(e.to_string())
                }
            })?;
        Ok(Workspace {
            id,
            nombre: nombre.trim().to_string(),
            ruta: ruta.to_string(),
            creada_en: a_fecha(ahora)?,
        })
    }

    /// Lista las áreas del usuario (recientes primero).
    pub fn workspaces_listar(&self, user_id: Uuid) -> HarnessResult<Vec<Workspace>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, nombre, ruta, creada_en FROM workspaces
                 WHERE user_id = ?1 ORDER BY creada_en DESC",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                Ok((
                    f.get::<_, String>(0)?,
                    f.get::<_, String>(1)?,
                    f.get::<_, String>(2)?,
                    f.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (id, nombre, ruta, creada) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(Workspace {
                id: a_uuid(id)?,
                nombre,
                ruta,
                creada_en: a_fecha(creada)?,
            });
        }
        Ok(out)
    }

    /// Área del usuario con esa ruta exacta (`None` si no existe).
    pub fn workspace_por_ruta(&self, user_id: Uuid, ruta: &str) -> HarnessResult<Option<Workspace>> {
        let conn = bloquear(&self.conn);
        let fila = conn
            .query_row(
                "SELECT id, nombre, ruta, creada_en FROM workspaces
                 WHERE user_id = ?1 AND ruta = ?2",
                params![user_id.as_hyphenated().to_string(), ruta],
                |f| {
                    Ok((
                        f.get::<_, String>(0)?,
                        f.get::<_, String>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        match fila {
            Some((id, nombre, ruta, creada)) => Ok(Some(Workspace {
                id: a_uuid(id)?,
                nombre,
                ruta,
                creada_en: a_fecha(creada)?,
            })),
            None => Ok(None),
        }
    }

    /// Área por id (de este usuario).
    pub fn workspace_por_id(&self, user_id: Uuid, id: Uuid) -> HarnessResult<Option<Workspace>> {
        let conn = bloquear(&self.conn);
        let fila = conn
            .query_row(
                "SELECT id, nombre, ruta, creada_en FROM workspaces
                 WHERE user_id = ?1 AND id = ?2",
                params![user_id.as_hyphenated().to_string(), id.as_hyphenated().to_string()],
                |f| {
                    Ok((
                        f.get::<_, String>(0)?,
                        f.get::<_, String>(1)?,
                        f.get::<_, String>(2)?,
                        f.get::<_, String>(3)?,
                    ))
                },
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        match fila {
            Some((id, nombre, ruta, creada)) => Ok(Some(Workspace {
                id: a_uuid(id)?,
                nombre,
                ruta,
                creada_en: a_fecha(creada)?,
            })),
            None => Ok(None),
        }
    }

    /// Renombra un área propia. Devuelve `false` si no existe.
    pub fn workspace_renombrar(&self, user_id: Uuid, id: Uuid, nombre: &str) -> HarnessResult<bool> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE workspaces SET nombre = ?1 WHERE id = ?2 AND user_id = ?3",
                params![
                    nombre.trim(),
                    id.as_hyphenated().to_string(),
                    user_id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n > 0)
    }

    /// Elimina un área propia. Las conversaciones del área quedan SIN área
    /// (`workspace_id = NULL`) para no perder historial (FK lógica).
    /// Devuelve `false` si no existía.
    pub fn workspace_eliminar(&self, user_id: Uuid, id: Uuid) -> HarnessResult<bool> {
        let conn = bloquear(&self.conn);
        let existe: bool = conn
            .query_row(
                "SELECT 1 FROM workspaces WHERE id = ?1 AND user_id = ?2",
                params![id.as_hyphenated().to_string(), user_id.as_hyphenated().to_string()],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))?
            .unwrap_or(false);
        if !existe {
            return Ok(false);
        }
        conn.execute(
            "UPDATE conversaciones SET workspace_id = NULL WHERE workspace_id = ?1 AND user_id = ?2",
            params![id.as_hyphenated().to_string(), user_id.as_hyphenated().to_string()],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        conn.execute(
            "DELETE FROM workspaces WHERE id = ?1 AND user_id = ?2",
            params![id.as_hyphenated().to_string(), user_id.as_hyphenated().to_string()],
        )
        .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use glory_harness_core::AgentPersistence;

    #[test]
    fn crud_workspaces_y_ruta_unica() {
        let p = PersistenciaSqlite::en_memoria().expect("memoria");
        let user = Uuid::new_v4();

        // Sin áreas al inicio.
        assert!(p.workspaces_listar(user).expect("listar").is_empty());

        // Crear dos áreas.
        let w1 = p
            .workspace_crear(user, "Área A", "C:\\tmp\\proyecto-a")
            .expect("crear A");
        let w2 = p
            .workspace_crear(user, "Área B", "C:\\tmp\\proyecto-b")
            .expect("crear B");
        let lista = p.workspaces_listar(user).expect("listar");
        assert_eq!(lista.len(), 2);

        // Ruta duplicada → error UNIQUE claro.
        let duplicada = p.workspace_crear(user, "A duplicada", "C:\\tmp\\proyecto-a");
        assert!(duplicada.is_err());

        // Buscar por ruta e id.
        let por_ruta = p
            .workspace_por_ruta(user, "C:\\tmp\\proyecto-a")
            .expect("por ruta")
            .expect("existe");
        assert_eq!(por_ruta.id, w1.id);
        assert!(p
            .workspace_por_ruta(user, "C:\\tmp\\inexistente")
            .expect("por ruta inexistente")
            .is_none());
        let por_id = p
            .workspace_por_id(user, w2.id)
            .expect("por id")
            .expect("existe");
        assert_eq!(por_id.ruta, w2.ruta);

        // Renombrar.
        assert!(p
            .workspace_renombrar(user, w1.id, "Área A renombrada")
            .expect("renombrar"));
        assert_eq!(
            p.workspace_por_id(user, w1.id)
                .expect("por id")
                .expect("existe")
                .nombre,
            "Área A renombrada"
        );

        // Ajeno: renombrar con otro user no toca nada.
        let otro = Uuid::new_v4();
        assert!(!p
            .workspace_renombrar(otro, w1.id, "ajeno")
            .expect("renombrar ajeno"));

        // Eliminar deja las conversaciones del área sin workspace_id.
        let conv = p.conversacion_crear(user, "conv en A").expect("crear conv");
        // La conversación por defecto nace sin área; se asigna explícitamente.
        p.conversacion_asignar_workspace(user, conv, Some(w1.id))
            .expect("asignar ws");
        assert!(p
            .workspace_eliminar(user, w1.id)
            .expect("eliminar A"));
        let por_id_ahora = p.workspace_por_id(user, w1.id).expect("por id");
        assert!(por_id_ahora.is_none());
        // La conversación sigue, ahora sin área.
        let lista_conv = p.conversaciones_listar(user).expect("listar convs");
        assert_eq!(lista_conv.len(), 1);
        assert!(p
            .conversaciones_listar_ws(user, None)
            .expect("listar sin area")
            .iter()
            .any(|c| c.id == conv));
        // El área B sigue intacta.
        assert_eq!(p.workspaces_listar(user).expect("listar").len(), 1);
        // Eliminar inexistente → false.
        assert!(!p.workspace_eliminar(user, Uuid::new_v4()).expect("eliminar fake"));
    }
}
