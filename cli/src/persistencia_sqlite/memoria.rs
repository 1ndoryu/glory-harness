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
    use glory_harness_core::ports::{AmbitoMemoria, MemoriaEntrada};
    use glory_harness_core::AgentPersistence;

    /// Recuerdo mínimo para las pruebas de aislamiento.
    fn recuerdo(clave: &str, contenido: &str) -> MemoriaEntrada {
        MemoriaEntrada::nueva(clave.to_string(), contenido.to_string(), "prueba".to_string())
    }

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

    /// [109A-2] El aislamiento es estricto en las dos direcciones: el global
    /// no ve proyectos y un proyecto no ve ni el global ni a su hermano.
    #[tokio::test]
    async fn memoria_aislada_entre_global_y_proyectos() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let (area_a, area_b) = (Uuid::new_v4(), Uuid::new_v4());
        let (amb_a, amb_b) = (
            AmbitoMemoria::Proyecto(area_a),
            AmbitoMemoria::Proyecto(area_b),
        );

        // La MISMA clave en tres ámbitos: son tres recuerdos distintos.
        for (ambito, marca) in [
            (AmbitoMemoria::Global, "global"),
            (amb_a, "proyecto-a"),
            (amb_b, "proyecto-b"),
        ] {
            p.memoria_upsert(user, ambito, &recuerdo("color", marca))
                .await
                .expect("upsert");
        }

        let global = p
            .memoria_listar(user, AmbitoMemoria::Global)
            .await
            .expect("global");
        assert_eq!(global.len(), 1, "el global no ve los recuerdos de proyecto");
        assert_eq!(global[0].contenido, "global");

        let solo_a = p.memoria_listar(user, amb_a).await.expect("proyecto A");
        assert_eq!(solo_a.len(), 1, "el proyecto A no ve global ni al B");
        assert_eq!(solo_a[0].contenido, "proyecto-a");

        // Con contenido en los tres ámbitos, `memoria_ambitos` los lista todos.
        let ambitos = p.memoria_ambitos(user).await.expect("ámbitos");
        assert_eq!(ambitos.len(), 3);
        assert!(ambitos.contains(&AmbitoMemoria::Global));
        assert!(ambitos.contains(&amb_a) && ambitos.contains(&amb_b));

        // Borrar en A no toca B ni el global.
        p.memoria_borrar(user, amb_a, "color")
            .await
            .expect("borrar en A");
        assert!(p
            .memoria_listar(user, amb_a)
            .await
            .expect("proyecto A")
            .is_empty());
        assert_eq!(
            p.memoria_listar(user, amb_b)
                .await
                .expect("proyecto B")
                .len(),
            1
        );
        assert_eq!(
            p.memoria_listar(user, AmbitoMemoria::Global)
                .await
                .expect("global")
                .len(),
            1
        );

        // Y tras borrar, el ámbito A deja de aparecer: `memoria_ambitos` son
        // los ámbitos CON contenido (más el global, que siempre está).
        let restantes = p.memoria_ambitos(user).await.expect("ámbitos");
        assert_eq!(restantes.len(), 2);
        assert!(restantes.contains(&AmbitoMemoria::Global));
        assert!(restantes.contains(&amb_b));
    }

    /// Repetir la clave dentro del mismo ámbito actualiza la fila (upsert)
    /// en vez de duplicarla; el ámbito es parte de la identidad.
    #[tokio::test]
    async fn upsert_repite_clave_solo_en_su_ambito() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let amb = AmbitoMemoria::Proyecto(Uuid::new_v4());

        p.memoria_upsert(user, amb, &recuerdo("editor", "vim"))
            .await
            .expect("primer upsert");
        p.memoria_upsert(user, amb, &recuerdo("editor", "helix"))
            .await
            .expect("segundo upsert");
        let entradas = p.memoria_listar(user, amb).await.expect("listar");
        assert_eq!(entradas.len(), 1, "la clave se repite, no se duplica");
        assert_eq!(entradas[0].contenido, "helix");

        // La misma clave en el global sigue siendo otra fila.
        p.memoria_upsert(user, AmbitoMemoria::Global, &recuerdo("editor", "nano"))
            .await
            .expect("upsert global");
        assert_eq!(
            p.memoria_listar(user, AmbitoMemoria::Global)
                .await
                .expect("global")
                .len(),
            1
        );
    }

    /// El ámbito de un usuario no contamina a otro usuario con la misma área.
    #[tokio::test]
    async fn memoria_no_cruza_usuarios() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let (u1, u2) = (Uuid::new_v4(), Uuid::new_v4());
        let area = Uuid::new_v4();
        p.memoria_upsert(
            u1,
            AmbitoMemoria::Proyecto(area),
            &recuerdo("clave", "de u1"),
        )
        .await
        .expect("upsert u1");

        assert!(p
            .memoria_listar(u2, AmbitoMemoria::Proyecto(area))
            .await
            .expect("u2")
            .is_empty());
        // Y para u2 esa área no existe: sin recuerdos, no aparece en sus ámbitos.
        let ambitos = p.memoria_ambitos(u2).await.expect("ámbitos u2");
        assert_eq!(ambitos, vec![AmbitoMemoria::Global]);
    }

    /// Crea en disco una BD con el esquema ANTERIOR a [109A-2] (tabla
    /// `memoria` con PK `(user_id, clave)` y sin `workspace_id`), que es el
    /// caso real de una instalación que actualiza.
    fn bd_legacy(ruta: &std::path::Path, user: Uuid, clave: &str, contenido: &str) {
        let conn = rusqlite::Connection::open(ruta).expect("abrir BD legacy");
        conn.execute_batch(
            "CREATE TABLE memoria (
                 user_id TEXT NOT NULL,
                 clave TEXT NOT NULL,
                 contenido TEXT NOT NULL,
                 actualizada_en TEXT NOT NULL,
                 origen TEXT NOT NULL DEFAULT '',
                 usos INTEGER NOT NULL DEFAULT 0,
                 ultimo_uso TEXT,
                 PRIMARY KEY (user_id, clave)
             );",
        )
        .expect("esquema legacy");
        conn.execute(
            "INSERT INTO memoria (user_id, clave, contenido, actualizada_en, origen)
             VALUES (?1, ?2, ?3, ?4, 'turno')",
            params![
                user.as_hyphenated().to_string(),
                clave,
                contenido,
                chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            ],
        )
        .expect("fila legacy");
    }

    /// [109A-2] La migración conserva los recuerdos y los deja en el ámbito
    /// global (nadie pierde lo que ya sabía), y es idempotente al reabrir.
    #[tokio::test]
    async fn migra_memoria_legacy_al_ambito_global_sin_perder_datos() {
        let ruta = std::env::temp_dir().join(format!("glory-memoria-{}.db", Uuid::new_v4()));
        let user = Uuid::new_v4();
        bd_legacy(&ruta, user, "gusto", "prefiere té");

        let p = PersistenciaSqlite::abrir(&ruta).expect("abrir BD migrada");
        let global = p
            .memoria_listar(user, AmbitoMemoria::Global)
            .await
            .expect("global");
        assert_eq!(global.len(), 1, "el recuerdo legacy sobrevive");
        assert_eq!(global[0].clave, "gusto");
        assert_eq!(global[0].contenido, "prefiere té");
        assert_eq!(global[0].origen, "turno", "los metadatos también migran");

        // Un proyecto no lo ve: el legado es global.
        assert!(p
            .memoria_listar(user, AmbitoMemoria::Proyecto(Uuid::new_v4()))
            .await
            .expect("proyecto")
            .is_empty());
        assert_eq!(
            p.memoria_ambitos(user).await.expect("ámbitos"),
            vec![AmbitoMemoria::Global]
        );

        // Reabrir no repite la migración ni duplica la fila.
        drop(p);
        let p2 = PersistenciaSqlite::abrir(&ruta).expect("reabrir");
        assert_eq!(
            p2.memoria_listar(user, AmbitoMemoria::Global)
                .await
                .expect("global")
                .len(),
            1
        );
        drop(p2);
        for sufijo in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{sufijo}", ruta.display()));
        }
    }
}
