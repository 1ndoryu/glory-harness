//! Lectura de mensajes y de una conversación puntual ([139A-8 F2]).
//!
//! Partido de `mod.rs` (limite-lineas 500 para servicio): el núcleo síncrono
//! `leer_mensajes`/`leer_conversacion` recibe `&Connection` para llamarse
//! desde `spawn_blocking` (`con_conn`) o desde contextos síncronos. El
//! `rowid` desempata el mismo segundo ([129A-1]): las filas `reasoning` se
//! insertan justo antes de su `assistant` y la precisión de `creado_en` es 1 s.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::types::Value;
use rusqlite::Connection;
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::ports::MensajePersistido;
use glory_harness_core::HarnessResult;

use super::super::{a_fecha, a_uuid, InfoConversacion, PersistenciaSqlite};

/// Núcleo síncrono de lectura de mensajes ([139A-8 F2]): una sola consulta
/// con filtros opcionales. `desde` es `creado_en >= ?` (misma precisión de
/// segundos que el almacenamiento, así que conserva la semántica `>=` de
/// `sesion.rs`); `limite` opcional. Sin filtros equivale al listado completo.
pub(crate) fn leer_mensajes(
    conn: &Connection,
    conversacion_id: Uuid,
    desde: Option<String>,
    limite: Option<i64>,
) -> HarnessResult<Vec<MensajePersistido>> {
    let mut sql = String::from(
        "SELECT id, rol, contenido, creado_en FROM mensajes
         WHERE conversacion_id = ?1",
    );
    let mut args = vec![Value::Text(
        conversacion_id.as_hyphenated().to_string(),
    )];
    if let Some(d) = desde {
        sql.push_str(&format!(" AND creado_en >= ?{}", args.len() + 1));
        args.push(Value::Text(d));
    }
    sql.push_str(" ORDER BY creado_en ASC, rowid ASC");
    if let Some(n) = limite {
        sql.push_str(&format!(" LIMIT ?{}", args.len() + 1));
        args.push(Value::Integer(n));
    }
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| Error::Persistencia(e.to_string()))?;
    let filas = stmt
        .query_map(rusqlite::params_from_iter(args), |f| {
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
        let (id, rol, contenido, creado) =
            fila.map_err(|e| Error::Persistencia(e.to_string()))?;
        out.push(MensajePersistido {
            id: a_uuid(id)?,
            conversacion_id,
            rol,
            contenido,
            creado_en: a_fecha(creado)?,
        });
    }
    Ok(out)
}

/// Núcleo síncrono de `conversacion_obtener` ([139A-8 F2] R3): una
/// conversación propia por id con SELECT puntual (evita el full-scan de
/// `conversaciones_listar` + `find`). Misma forma que `listar` (sin proyecto:
/// el auto-nombre no lo necesita).
pub(crate) fn leer_conversacion(
    conn: &Connection,
    user_id: Uuid,
    conversacion_id: Uuid,
) -> HarnessResult<Option<InfoConversacion>> {
    use rusqlite::{params, OptionalExtension};
    conn.query_row(
        "SELECT id, titulo, archivada, creada_en, actualizada_en FROM conversaciones
         WHERE id = ?1 AND user_id = ?2",
        params![
            conversacion_id.as_hyphenated().to_string(),
            user_id.as_hyphenated().to_string()
        ],
        |f| {
            Ok((
                f.get::<_, String>(0)?,
                f.get::<_, String>(1)?,
                f.get::<_, i64>(2)?,
                f.get::<_, String>(3)?,
                f.get::<_, String>(4)?,
            ))
        },
    )
    .optional()
    .map_err(|e| Error::Persistencia(e.to_string()))
    .and_then(|fila| {
        fila.map(|(id, titulo, archivada, creada, actualizada)| {
            Ok(InfoConversacion {
                id: a_uuid(id)?,
                titulo,
                archivada: archivada != 0,
                creada_en: a_fecha(creada)?,
                actualizada_en: a_fecha(actualizada)?,
                workspace_id: None,
                workspace_nombre: None,
            })
        })
        .transpose()
    })
}

/// Mensajes con filtro en SQL ([139A-8 F2] R3): convierte la marca a texto
/// con precisión de segundos y delega el SQL a `con_conn` (`spawn_blocking`).
/// El puerto (`listar_mensajes`) delega aquí sin filtros.
pub(crate) async fn listar_mensajes_desde(
    db: &PersistenciaSqlite,
    conversacion_id: Uuid,
    desde: Option<DateTime<Utc>>,
    limite: Option<u64>,
) -> HarnessResult<Vec<MensajePersistido>> {
    let desde_txt = desde.map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true));
    let limite_n = limite.map(|n| n.min(i64::MAX as u64) as i64);
    db.con_conn(move |conn| leer_mensajes(conn, conversacion_id, desde_txt, limite_n))
        .await
}

#[cfg(test)]
mod pruebas {
    use super::super::super::PersistenciaSqlite;
    use chrono::{SecondsFormat, Utc};
    use glory_harness_core::ports::MensajePersistido;
    use glory_harness_core::AgentPersistence;
    use uuid::Uuid;

    /// [139A-8 F2] (R3) DoD: el turno largo no hace full-scan — con marca, el
    /// SQL devuelve solo los posteriores (`>=`, misma precisión de segundos);
    /// `conversacion_obtener` resuelve una fila por PK (auto-nombre) y respeta
    /// ownership.
    #[tokio::test]
    async fn mensajes_desde_filtra_en_sql_y_obtener_es_puntual() {
        let p = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let user = Uuid::new_v4();
        let conv = p
            .conversacion_crear(user, "Nueva conversación")
            .expect("crear conversación");
        let antiguos_en = Utc::now() - chrono::Duration::hours(2);
        for i in 0..200 {
            p.guardar_mensaje(&MensajePersistido {
                id: Uuid::new_v4(),
                conversacion_id: conv,
                rol: "user".into(),
                contenido: format!("relleno {i}"),
                creado_en: if i >= 100 { Utc::now() } else { antiguos_en },
            })
            .await
            .expect("guardar mensaje");
        }
        let marca = (Utc::now() - chrono::Duration::minutes(30))
            .to_rfc3339_opts(SecondsFormat::Secs, true);
        assert!(
            p.conversacion_compactar(user, conv, &marca, "resumen")
                .expect("marcar")
        );

        let punto = p
            .conversacion_compactacion(user, conv)
            .expect("leer punto")
            .expect("punto");
        let cuando = chrono::DateTime::parse_from_rfc3339(&punto.compactado_en)
            .expect("fecha")
            .with_timezone(&Utc);
        let posteriores = p
            .listar_mensajes_desde(conv, Some(cuando), None)
            .await
            .expect("posteriores");
        assert_eq!(posteriores.len(), 100);
        // `>=`: un mensaje del mismo segundo que la marca entra.
        let completos = p.listar_mensajes(conv).await.expect("todos");
        assert_eq!(completos.len(), 200);

        let obtenida = p
            .conversacion_obtener(user, conv)
            .expect("obtener")
            .expect("existe");
        assert_eq!(obtenida.titulo, "Nueva conversación");
        assert!(p
            .conversacion_obtener(Uuid::new_v4(), conv)
            .expect("ajena")
            .is_none());
        assert!(p
            .conversacion_obtener(user, Uuid::new_v4())
            .expect("inexistente")
            .is_none());
    }
}
