//! Persistencia SQLite del CLI/desktop (plan 039A-1, Fase 3).
//!
//! Implementa [`AgentPersistence`] y [`ProgramadorTareas`] sobre rusqlite
//! bundled (WAL, sin SQLite del sistema). La app Tauri la usa para historial
//! durable; el CLI one-shot y el daemon siguen en memoria (`persistencia.rs`).
//!
//! Notas de diseño:
//! - Una sola `Connection` tras `Mutex` (`Clone` comparte el estado, como la
//!   versión en memoria). SQLite local con WAL responde en ms; no se usa
//!   `spawn_blocking` para no complicar los 20 métodos del puerto.
//! - `tarea_tomar` es atómica (`UPDATE ... WHERE estado='pendiente'` + filas
//!   afectadas); `tareas_recuperar_interrumpidas` devuelve las `ejecutando` a
//!   `pendiente` (la versión en memoria no tiene heartbeat y devuelve 0).
//! - Los logs de tareas quedan vacíos (el trait no tiene inserción de logs y
//!   la versión en memoria tampoco registra; la tabla existe para futuro).

use async_trait::async_trait;
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::ports::{
    AccionAuditable, LogTareaEjecucion, MemoriaEntrada, MensajePersistido, NuevaTareaProgramada,
    ProgramadorTareas, SkillEntrada, TareaProgramada, TareaProgramadaPendiente, TurnoPersistido,
};
use glory_harness_core::{AgentPersistence, HarnessResult};

/// Esquema inicial (idempotente: `IF NOT EXISTS`).
const ESQUEMA: &str = "
CREATE TABLE IF NOT EXISTS conversaciones (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    titulo TEXT NOT NULL,
    archivada INTEGER NOT NULL DEFAULT 0,
    creada_en TEXT NOT NULL,
    actualizada_en TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS mensajes (
    id TEXT PRIMARY KEY,
    conversacion_id TEXT NOT NULL,
    rol TEXT NOT NULL,
    contenido TEXT NOT NULL,
    creado_en TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_mensajes_conv ON mensajes (conversacion_id, creado_en);
CREATE TABLE IF NOT EXISTS turnos (
    id TEXT PRIMARY KEY,
    conversacion_id TEXT NOT NULL,
    user_id TEXT NOT NULL,
    estado TEXT NOT NULL,
    resumen TEXT,
    creado_en TEXT NOT NULL,
    provider TEXT,
    modelo TEXT,
    tokens_prompt INTEGER NOT NULL DEFAULT 0,
    tokens_complecion INTEGER NOT NULL DEFAULT 0,
    tools_ejecutadas INTEGER NOT NULL DEFAULT 0,
    duracion_ms INTEGER NOT NULL DEFAULT 0,
    error TEXT
);
CREATE TABLE IF NOT EXISTS acciones (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    turno_id TEXT NOT NULL,
    tool TEXT NOT NULL,
    ok INTEGER NOT NULL,
    resumen TEXT NOT NULL,
    argumentos_json TEXT,
    diff TEXT,
    creado_en TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS memoria (
    user_id TEXT NOT NULL,
    clave TEXT NOT NULL,
    contenido TEXT NOT NULL,
    /* [069A-4] Metadatos de auditoría y curaduría (migración para BDs
     * antiguas en MIGRACIONES; filas previas leen NULL → se tratan como
     * nuevas al leer, nunca como obsoletas). */
    actualizada_en TEXT NOT NULL DEFAULT '',
    origen TEXT NOT NULL DEFAULT '',
    usos INTEGER NOT NULL DEFAULT 0,
    ultimo_uso TEXT,
    PRIMARY KEY (user_id, clave)
);
CREATE TABLE IF NOT EXISTS skills (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    nombre TEXT NOT NULL,
    descripcion TEXT NOT NULL,
    instrucciones TEXT NOT NULL,
    activa INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS tareas (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    nombre TEXT NOT NULL,
    prompt TEXT NOT NULL,
    tipo TEXT NOT NULL,
    cron_expr TEXT,
    proxima_ejecucion TEXT,
    estado TEXT NOT NULL,
    creado_en TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tarea_logs (
    id TEXT PRIMARY KEY,
    tarea_id TEXT NOT NULL,
    ok INTEGER NOT NULL,
    resumen TEXT NOT NULL,
    ejecutada_en TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS config (
    clave TEXT PRIMARY KEY,
    valor TEXT NOT NULL
);
";

/// Migraciones idempotentes para BDs creadas con un esquema anterior
/// (`CREATE TABLE IF NOT EXISTS` no altera tablas existentes).
const MIGRACIONES: &[&str] = &[
    /* [039A-1 04-09 H6] Acciones: diff del cambio + marca de tiempo para
     * repintar las tools en orden al recargar el historial. La columna
     * `creado_en` admite filas previas sin valor (NULL) → se rellena al
     * insertar; el ORDER BY usa COALESCE al leer. */
    "ALTER TABLE acciones ADD COLUMN diff TEXT",
    "ALTER TABLE acciones ADD COLUMN creado_en TEXT",
    /* [069A-4] Metadatos de memoria (ver tabla `memoria`): columnas nuevas
     * sin DEFAULT salvo `usos` (las filas antiguas leen NULL y se tratan
     * como nuevas al leer). */
    "ALTER TABLE memoria ADD COLUMN actualizada_en TEXT",
    "ALTER TABLE memoria ADD COLUMN origen TEXT",
    "ALTER TABLE memoria ADD COLUMN usos INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE memoria ADD COLUMN ultimo_uso TEXT",
];

/// Vista de conversación para la sidebar (Tauri la serializa tal cual).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InfoConversacion {
    pub id: Uuid,
    pub titulo: String,
    pub archivada: bool,
    pub actualizada_en: DateTime<Utc>,
}

/// Acción (tool) recuperada para repintar el historial al recargar.
/// [039A-1 04-09 H6] El orden de las acciones se ancla en `turno_en`
/// (timestamp del turno al que pertenecen), no en un timestamp propio.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AccionRecuperada {
    pub tool: String,
    pub ok: bool,
    pub resumen: String,
    pub argumentos_json: Option<String>,
    pub diff: Option<String>,
    /// `creado_en` del turno (para intercalar entre los mensajes del turno).
    pub turno_en: String,
}

/// Implementación SQLite de [`AgentPersistence`] (+ [`ProgramadorTareas`]).
/// `Clone` comparte la conexión, así que un solo `Arc` sirve a ambos puertos.
#[derive(Debug, Clone)]
pub struct PersistenciaSqlite {
    conn: Arc<Mutex<Connection>>,
}

fn ahora_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true)
}

fn a_fecha(s: String) -> HarnessResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| Error::Persistencia(format!("fecha inválida en BD: {e}")))
}

fn a_uuid(s: String) -> HarnessResult<Uuid> {
    Uuid::parse_str(&s).map_err(|e| Error::Persistencia(format!("uuid inválido en BD: {e}")))
}

fn bloquear<'a>(conn: &'a Arc<Mutex<Connection>>) -> std::sync::MutexGuard<'a, Connection> {
    conn.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl PersistenciaSqlite {
    fn abrir_conexion(ruta: Option<&Path>) -> HarnessResult<Connection> {
        let conn = match ruta {
            Some(r) => {
                if let Some(padre) = r.parent() {
                    if !padre.as_os_str().is_empty() {
                        std::fs::create_dir_all(padre).map_err(Error::from)?;
                    }
                }
                Connection::open(r)
            }
            None => Connection::open_in_memory(),
        }
        .map_err(|e| Error::Persistencia(format!("no se pudo abrir la BD: {e}")))?;
        // WAL solo en archivo (en memoria no aplica); el resto siempre.
        if ruta.is_some() {
            conn.pragma_update(None, "journal_mode", "WAL")
                .map_err(|e| Error::Persistencia(format!("WAL no disponible: {e}")))?;
        }
        conn.pragma_update(None, "synchronous", "NORMAL")
            .map_err(|e| Error::Persistencia(format!("pragma synchronous: {e}")))?;
        conn.pragma_update(None, "cache_size", -2000)
            .map_err(|e| Error::Persistencia(format!("pragma cache_size: {e}")))?;
        conn.busy_timeout(std::time::Duration::from_millis(5000))
            .map_err(|e| Error::Persistencia(format!("busy_timeout: {e}")))?;
        conn.execute_batch(ESQUEMA)
            .map_err(|e| Error::Persistencia(format!("esquema inicial: {e}")))?;
        // Migraciones idempotentes: una BD antigua no tiene las columnas que
        // el `CREATE TABLE IF NOT EXISTS` no altera. Ignoramos "duplicate
        // column name" (ya migrada) y propagamos el resto.
        for migracion in MIGRACIONES {
            if let Err(e) = conn.execute_batch(migracion) {
                let msg = e.to_string();
                if !msg.contains("duplicate column name") {
                    return Err(Error::Persistencia(format!("migración: {msg}")));
                }
            }
        }
        Ok(conn)
    }

    /// Abre (o crea) la BD en `ruta`, con directorios padres si faltan.
    pub fn abrir(ruta: &Path) -> HarnessResult<Self> {
        Ok(Self {
            conn: Arc::new(Mutex::new(Self::abrir_conexion(Some(ruta))?)),
        })
    }

    /// BD en memoria (tests y usos efímeros; misma API).
    pub fn en_memoria() -> HarnessResult<Self> {
        Ok(Self {
            conn: Arc::new(Mutex::new(Self::abrir_conexion(None)?)),
        })
    }

    /// Ruta de la BD de la app: `%APPDATA%/glory-harness/glory-harness.db`
    /// en Windows, `~/.local/share/glory-harness/` en el resto.
    pub fn ruta_bd_app() -> Option<PathBuf> {
        #[cfg(windows)]
        let base = std::env::var_os("APPDATA").map(PathBuf::from);
        #[cfg(not(windows))]
        let base = std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share"));
        base.map(|b| b.join("glory-harness").join("glory-harness.db"))
    }

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
                 VALUES (?1, ?2, 'resumen', 'Resume en 3 viñetas', 'Al terminar, resume tu respuesta en 3 viñetas concisas.', 1)",
                params![Uuid::new_v4().as_hyphenated().to_string(), user_id.as_hyphenated().to_string()],
            );
        }
        self
    }

    // --- CRUD de conversaciones (inherente: no forma parte del trait) ---

    /// Crea una conversación y devuelve su id.
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

    /// Elimina la conversación con sus mensajes y turnos (transacción).
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

    /// Acciones (tools ejecutadas) de una conversación en orden de ejecución.
    /// El orden se ancla en el `creado_en` del TURNO al que pertenece cada
    /// acción (las acciones no tienen timestamp fiable de UI; el JOIN da el
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

    /// [039A-3 P1] Métricas reales del ÚLTIMO turno de una conversación, para
    /// repintar el pie de turno al cargar. `None` si no hay turnos o si el
    /// turno no registró uso real (los tokens quedan 0 y el modelo el
    /// solicitado). El turno más reciente es el de `creado_en` mayor; los
    /// `id` son UUID (orden aleatorio), así que el orden se ancla en el
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

    // --- Config de la app (clave → valor; fuera del trait del núcleo) ---

    /// Lee un valor de configuración (`None` si no existe).
    pub fn config_leer(&self, clave: &str) -> HarnessResult<Option<String>> {
        bloquear(&self.conn)
            .query_row(
                "SELECT valor FROM config WHERE clave = ?1",
                params![clave],
                |f| f.get::<_, String>(0),
            )
            .optional()
            .map_err(|e| Error::Persistencia(e.to_string()))
    }

    /// Guarda (upsert) un valor de configuración.
    pub fn config_guardar(&self, clave: &str, valor: &str) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "INSERT INTO config (clave, valor) VALUES (?1, ?2)
                 ON CONFLICT(clave) DO UPDATE SET valor = excluded.valor",
                params![clave, valor],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    /// [039A-3 P1] Persiste el uso/modelo REAL de un turno terminado.
    ///
    /// El runtime guarda el turno con `tokens_prompt/complecion = 0` y el
    /// provider/modelo SOLICITADO (no el que respondió tras fallback); el
    /// `AgenteEvento::Usage` real viaja transitorio por el canal del turno.
    /// El backend de Tauri acumula esos Usage parciales (un turno con N
    /// tools emite N Usage) y, al `turno-fin` ok, llama a este método para
    /// rellenar las columnas reales. Solo se actualizan campos SIEMPRE
    /// acumulados: los tokens se SUMAN; provider/modelo se conservan los del
    /// último Usage (el que respondió de verdad).
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

    /// [039A-3 P2] Rebobina una conversación hasta un mensaje de usuario.
    ///
    /// Borra, en una transacción, el tramo posterior a `hasta_mensaje_id`
    /// (mensajes, turnos y las acciones de esos turnos). Con `editar=true`
    /// borra también el propio mensaje objetivo para reescribirlo; con
    /// `editar=false` (volver a punto) lo conserva como último mensaje.
    ///
    /// [039A-3 P3] Devuelve los `turno_id` borrados (los del tramo) para que
    /// el consumidor (vault del desktop) pueda ofrecer "restaurar archivos de
    /// este tramo" sin depender de la BD ya borrada. El hook de respaldo del
    /// sandbox registra cada escritura con su `turno_id` en el log del vault;
    /// con estos ids el vault sabe qué entradas corresponden al tramo.
    ///
    /// Anclaje del borrado:
    /// - Mensajes: `rowid` implícito (orden de inserción estricto), a prueba
    ///   de timestamps con precisión de 1 s. El mensaje objetivo debe ser de
    ///   rol `user`.
    /// - Turnos y sus acciones: `creado_en` del turno >= al del mensaje
    ///   objetivo. El turno que responde a un mensaje se persiste SIEMPRE
    ///   después (o en el mismo segundo) de que ese mensaje llegó, y el turno
    ///   anterior terminó antes de que el usuario escribiera el siguiente
    ///   mensaje: el `>=` borra el turno del propio mensaje objetivo (el
    ///   "hilo" de ese punto) sin alcanzar al turno previo.
    ///
    /// Falla (sin borrado parcial) si la conversación no es del `user_id` o
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

        // Propiedad de la conversación + existencia del mensaje objetivo
        // (rol user). Si falta, error explícito: nunca borrado parcial mudo.
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
                "mensaje objetivo no encontrado, no es de usuario o conversación ajena".into(),
            )
        })?;

        /* [039A-3 P3] Turnos del tramo ANTES de borrarlos: los ids que el
         * vault usará para localizar los respaldos de este tramo. */
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
        // Turnos del tramo (>= borra también el turno que responde al propio
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

#[async_trait]
impl AgentPersistence for PersistenciaSqlite {
    async fn guardar_turno(&self, turno: &TurnoPersistido) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
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
    }

    async fn finalizar_turno(
        &self,
        turno_id: Uuid,
        estado_final: &str,
        resumen: Option<&str>,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "UPDATE turnos SET estado = ?1, resumen = ?2 WHERE id = ?3",
                params![estado_final, resumen, turno_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
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
    }

    async fn listar_mensajes(
        &self,
        conversacion_id: Uuid,
    ) -> HarnessResult<Vec<MensajePersistido>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, rol, contenido, creado_en FROM mensajes
                 WHERE conversacion_id = ?1 ORDER BY creado_en ASC",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![conversacion_id.as_hyphenated().to_string()], |f| {
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

    async fn conversacion_tocar(&self, conversacion_id: Uuid) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "UPDATE conversaciones SET actualizada_en = ?1 WHERE id = ?2",
                params![ahora_rfc3339(), conversacion_id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    async fn registrar_accion(&self, accion: &AccionAuditable) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
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
    }

    async fn memoria_listar(&self, user_id: Uuid) -> HarnessResult<Vec<MemoriaEntrada>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT clave, contenido, actualizada_en, origen, usos, ultimo_uso
                 FROM memoria WHERE user_id = ?1",
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let filas = stmt
            .query_map(params![user_id.as_hyphenated().to_string()], |f| {
                Ok((
                    f.get::<_, String>(0)?,
                    f.get::<_, String>(1)?,
                    f.get::<_, Option<String>>(2)?,
                    f.get::<_, Option<String>>(3)?,
                    f.get::<_, i64>(4)?,
                    f.get::<_, Option<String>>(5)?,
                ))
            })
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
    }

    async fn memoria_upsert(&self, user_id: Uuid, entrada: &MemoriaEntrada) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "INSERT INTO memoria (user_id, clave, contenido, actualizada_en, origen, usos, ultimo_uso)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(user_id, clave) DO UPDATE SET
                    contenido = excluded.contenido,
                    actualizada_en = excluded.actualizada_en,
                    origen = excluded.origen,
                    usos = excluded.usos,
                    ultimo_uso = excluded.ultimo_uso",
                params![
                    user_id.as_hyphenated().to_string(),
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
    }

    async fn memoria_borrar(&self, user_id: Uuid, clave: &str) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "DELETE FROM memoria WHERE user_id = ?1 AND clave = ?2",
                params![user_id.as_hyphenated().to_string(), clave],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    async fn skills_listar(&self, user_id: Uuid) -> HarnessResult<Vec<SkillEntrada>> {
        let conn = bloquear(&self.conn);
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
    }

    async fn skills_registrar(&self, user_id: Uuid, skill: &SkillEntrada) -> HarnessResult<()> {
        // [069A-4] Alta o sustitución por (user_id, nombre): el curador
        // promueve recuerdos sin duplicar skills.
        bloquear(&self.conn)
            .execute(
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
    }

    async fn tareas_recuperar_interrumpidas(&self) -> HarnessResult<u64> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE tareas SET estado = 'pendiente' WHERE estado = 'ejecutando'",
                [],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n as u64)
    }

    async fn tareas_pendientes(&self, limite: u32) -> HarnessResult<Vec<TareaProgramadaPendiente>> {
        let conn = bloquear(&self.conn);
        let mut stmt = conn
            .prepare(
                "SELECT id, user_id, nombre, prompt, tipo, cron_expr FROM tareas
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
                ))
            })
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let mut out = Vec::new();
        for fila in filas {
            let (id, user_id, nombre, prompt, tipo, cron_expr) =
                fila.map_err(|e| Error::Persistencia(e.to_string()))?;
            out.push(TareaProgramadaPendiente {
                id: a_uuid(id)?,
                user_id: a_uuid(user_id)?,
                nombre,
                prompt,
                tipo,
                cron_expr,
            });
        }
        Ok(out)
    }

    async fn tarea_tomar(&self, id: Uuid) -> HarnessResult<bool> {
        let n = bloquear(&self.conn)
            .execute(
                "UPDATE tareas SET estado = 'ejecutando' WHERE id = ?1 AND estado = 'pendiente'",
                params![id.as_hyphenated().to_string()],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(n == 1)
    }

    async fn tarea_finalizar(
        &self,
        id: Uuid,
        ok: bool,
        _resumen: Option<&str>,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "UPDATE tareas SET estado = ?1 WHERE id = ?2",
                params![
                    if ok { "completada" } else { "pendiente" },
                    id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }

    async fn tarea_reprogramar(
        &self,
        id: Uuid,
        _user_id: Uuid,
        proxima: Option<DateTime<Utc>>,
    ) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute(
                "UPDATE tareas SET proxima_ejecucion = ?1, estado = 'pendiente' WHERE id = ?2",
                params![
                    proxima.map(|d| d.to_rfc3339_opts(SecondsFormat::Secs, true)),
                    id.as_hyphenated().to_string()
                ],
            )
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }
}

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

    /// Ruta temporal única para la BD de un test (se borra al terminar).
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
                .expect("crear conversación");
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
        // Releer por SQL directo: el método es inherente y no expone lector.
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
        // Timestamps crecientes (precisión de BD = 1 s): el turno que responde
        // a un user SIEMPRE se persiste después de que ese user llegó y antes
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
            resumen: "leyó".into(),
            argumentos_json: None,
            diff: None,
        })
        .await
        .expect("acción 1");
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
            resumen: "editó".into(),
            argumentos_json: None,
            diff: None,
        })
        .await
        .expect("acción 2");
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
        // Turnos: solo queda el anterior al mensaje objetivo (ninguno aquí).
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
        // conservando los mensajes ANTERIORES al punto (u1 sigue ahí: volver
        // a un punto conservó su mensaje y editar u3 solo recorta desde u3).
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

        // Mensaje de conversación ajena.
        assert!(p
            .rewind_conversacion(conv, Uuid::new_v4(), user, false)
            .is_err());
        // Conversación de otro usuario.
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
        // Nada se borró en ningún caso (transacciones fallidas).
        let mensajes = p.listar_mensajes(conv).await.expect("listar");
        assert_eq!(mensajes.len(), 2);
    }
}
