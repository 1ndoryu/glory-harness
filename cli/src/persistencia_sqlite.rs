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
//!
//! [069A-5 F5] Partido por dominio (limite-lineas 1006 + nivel-2): el esquema,
//! los tipos y la apertura quedan aquí; `conversaciones` (chats, turnos,
//! mensajes, acciones, rewind), `memoria` (recuerdos + skills) y `tareas`
//! (cola del scheduler + `ProgramadorTareas`) viven en submódulos.

use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use glory_harness_core::error::Error;
use glory_harness_core::HarnessResult;

mod conversaciones;
mod memoria;
mod puerto;
mod tareas;
mod workspaces;

pub use workspaces::Workspace;

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
/* [069A-Proyectos] Áreas de trabajo (workspaces): nombre visible asignado
 * por el usuario + carpeta raíz única (ruta absoluta, UNIQUE). Una
 * conversación pertenece a UNA área vía `workspace_id` (NULL = sin área). */
CREATE TABLE IF NOT EXISTS workspaces (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    nombre TEXT NOT NULL,
    ruta TEXT NOT NULL UNIQUE,
    creada_en TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_workspaces_user ON workspaces (user_id);
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
    /* [069A-Proyectos] Áreas de trabajo: columna `workspace_id` en
     * `conversaciones` (NULL = conversación sin área, comportamiento previo).
     * FK lógica, no física: las áreas se borran sin arrastrar historial. */
    "ALTER TABLE conversaciones ADD COLUMN workspace_id TEXT",
    "CREATE INDEX IF NOT EXISTS idx_conversaciones_ws ON conversaciones (user_id, workspace_id)",
    "ALTER TABLE conversaciones ADD COLUMN meta_texto TEXT",
    "ALTER TABLE conversaciones ADD COLUMN meta_iniciada_en TEXT",
    "ALTER TABLE conversaciones ADD COLUMN meta_pausada_en TEXT",
    "ALTER TABLE conversaciones ADD COLUMN meta_logros TEXT NOT NULL DEFAULT '[]'",
];

/// Estado crudo de meta leído desde SQLite. La conversión a dominio vive en
/// `servicio::meta`, para que esta capa no dependa del ciclo de vida del agente.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MetaConversacionPersistida {
    pub texto: Option<String>,
    pub iniciada_en: Option<String>,
    pub pausada_en: Option<String>,
    pub logros_json: String,
}

/// Vista de conversación para la sidebar (Tauri la serializa tal cual).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InfoConversacion {
    pub id: Uuid,
    pub titulo: String,
    pub archivada: bool,
    pub actualizada_en: DateTime<Utc>,
    /// Proyecto asociado solo cuando la conversación se lista para la sidebar.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace_nombre: Option<String>,
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

    /// Elimina una opción persistida. Es idempotente para que limpiar una
    /// configuración ausente tenga el mismo resultado que limpiarla una vez.
    pub fn config_borrar(&self, clave: &str) -> HarnessResult<()> {
        bloquear(&self.conn)
            .execute("DELETE FROM config WHERE clave = ?1", params![clave])
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        Ok(())
    }
}
