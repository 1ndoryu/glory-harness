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
use glory_harness_core::AmbitoMemoria;

mod compactacion;
mod conversaciones;
mod eventos_turno;
mod memoria;
mod puerto;
mod tareas;
mod workspaces;

pub use eventos_turno::EventoTurnoRegistrado;
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
    creada_en TEXT NOT NULL,
    /* [119A-2 F3] Fijado del proyecto (1 = primero en el sidebar). */
    fijado INTEGER NOT NULL DEFAULT 0
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
    /* [119A-6 F2] Programación canónica tz-aware (`clase:expresion@Zona`,
     * fuente de verdad para reprogramar) + zona duplicada para mostrar sin
     * reparsear. `tipo`/`cron_expr` quedan como espejo legible heredado. */
    programacion TEXT NOT NULL DEFAULT '',
    zona_horaria TEXT NOT NULL DEFAULT 'UTC',
    /* [119A-6 F2 C10] Políticas F3 (se persisten y muestran, no actúan). */
    notificacion TEXT NOT NULL DEFAULT 'fallos',
    reintentos INTEGER NOT NULL DEFAULT 0,
    proxima_ejecucion TEXT,
    estado TEXT NOT NULL,
    creado_en TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS tarea_logs (
    id TEXT PRIMARY KEY,
    tarea_id TEXT NOT NULL,
    ok INTEGER NOT NULL,
    resumen TEXT NOT NULL,
    ejecutada_en TEXT NOT NULL,
    /* [119A-6 F2 C8] Ventana de ejecución + clasificación declarada por la
     * propia tarea; NULL = fila legacy (sin clasificar, no inventar). */
    iniciado_en TEXT,
    finalizado_en TEXT,
    resultado TEXT
);
CREATE TABLE IF NOT EXISTS config (
    clave TEXT PRIMARY KEY,
    valor TEXT NOT NULL
);
/* [129A-4 F1] Log de eventos por turno (observabilidad): una fila por evento
 * del contrato `AgenteEvento` reenviado a la UI (`token` y
 * `razonamiento_delta` se omiten por volumen; el texto vive en `mensajes`).
 * `peticion_turno` atribuye la respuesta de aprobación (canal aparte) al
 * turno que emitió la petición. */
CREATE TABLE IF NOT EXISTS eventos_turno (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    turno_id TEXT NOT NULL,
    tipo TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    creado_en TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_eventos_turno ON eventos_turno (turno_id, id);
CREATE TABLE IF NOT EXISTS peticion_turno (
    peticion_id TEXT PRIMARY KEY,
    turno_id TEXT NOT NULL
);
";

/// Definición de `memoria` ([109A-2]). Vive fuera del bloque base porque la
/// migración a memoria por proyecto necesita recrear la tabla con exactamente
/// esta forma (SQLite no permite alterar una `PRIMARY KEY`).
///
/// `workspace_id` usa `''` como centinela del ámbito global en vez de NULL
/// porque en SQLite los NULL no colisionan en un `UNIQUE`: con NULL, dos
/// recuerdos globales de la misma clave convivirían y el `ON CONFLICT` del
/// upsert dejaría de ser fiable.
const ESQUEMA_MEMORIA: &str = "
CREATE TABLE IF NOT EXISTS memoria (
    user_id TEXT NOT NULL,
    workspace_id TEXT NOT NULL DEFAULT '',
    clave TEXT NOT NULL,
    contenido TEXT NOT NULL,
    /* [069A-4] Metadatos de auditoría y curaduría (migración para BDs
     * antiguas en MIGRACIONES; filas previas leen NULL → se tratan como
     * nuevas al leer, nunca como obsoletas). */
    actualizada_en TEXT NOT NULL DEFAULT '',
    origen TEXT NOT NULL DEFAULT '',
    usos INTEGER NOT NULL DEFAULT 0,
    ultimo_uso TEXT,
    /* La misma clave puede existir a la vez en ámbitos distintos: son
     * recuerdos independientes. */
    UNIQUE (user_id, workspace_id, clave)
);
";

/// `workspace_id` almacenado para un ámbito: `''` = global, UUID = proyecto.
fn ambito_a_workspace_id(ambito: AmbitoMemoria) -> String {
    ambito
        .proyecto_id()
        .map(|id| id.as_hyphenated().to_string())
        .unwrap_or_default()
}

/// Inverso de [`ambito_a_workspace_id`]. Un `workspace_id` que no sea ni
/// vacío ni un UUID se reporta como error en vez de degradarse a global.
fn workspace_id_a_ambito(valor: &str) -> HarnessResult<AmbitoMemoria> {
    if valor.is_empty() {
        return Ok(AmbitoMemoria::Global);
    }
    Ok(AmbitoMemoria::Proyecto(a_uuid(valor.to_string())?))
}

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
    /* [109A-4 F3] Punto de compactación por conversación (`/compactar`): el
     * resumen del tramo y su marca de tiempo. Los mensajes anteriores NO se
     * borran (el historial visible y el rewind siguen intactos): solo dejan de
     * enviarse al modelo, que arranca del resumen. NULL = sin compactar. */
    "ALTER TABLE conversaciones ADD COLUMN compactado_en TEXT",
    "ALTER TABLE conversaciones ADD COLUMN resumen_compactado TEXT",
    /* [119A-6 F2] Programación canónica + políticas (BDs creadas antes de F2:
     * `''` = legacy, cae al espejo `tipo`+`cron_expr`; zona `UTC`). */
    "ALTER TABLE tareas ADD COLUMN programacion TEXT NOT NULL DEFAULT ''",
    "ALTER TABLE tareas ADD COLUMN zona_horaria TEXT NOT NULL DEFAULT 'UTC'",
    "ALTER TABLE tareas ADD COLUMN notificacion TEXT NOT NULL DEFAULT 'fallos'",
    "ALTER TABLE tareas ADD COLUMN reintentos INTEGER NOT NULL DEFAULT 0",
    "ALTER TABLE tarea_logs ADD COLUMN iniciado_en TEXT",
    "ALTER TABLE tarea_logs ADD COLUMN finalizado_en TEXT",
    "ALTER TABLE tarea_logs ADD COLUMN resultado TEXT",
    /* [119A-2 F3] Fijado de proyectos (BDs anteriores a F3: 0 = no fijado). */
    "ALTER TABLE workspaces ADD COLUMN fijado INTEGER NOT NULL DEFAULT 0",
    /* [139A-8 F5n/R4] Índices de los listados calientes de la auditoría §R4:
     * evitan el barrido completo en el historial, el último turno, la cola
     * de tareas, sus logs y la barra lateral de áreas. La forma (columna de
     * igualdad + columna de orden) deja el ORDER BY resuelto por el propio
     * índice, sin `TEMP B-TREE`. `IF NOT EXISTS` los hace idempotentes en
     * BD ya creadas. */
    "CREATE INDEX IF NOT EXISTS idx_conversaciones_user_act ON conversaciones (user_id, actualizada_en DESC)",
    "CREATE INDEX IF NOT EXISTS idx_turnos_conv ON turnos (conversacion_id, creado_en)",
    "CREATE INDEX IF NOT EXISTS idx_acciones_turno ON acciones (turno_id, id)",
    "CREATE INDEX IF NOT EXISTS idx_tareas_user ON tareas (user_id, creado_en)",
    "CREATE INDEX IF NOT EXISTS idx_tarea_logs_tarea ON tarea_logs (tarea_id, ejecutada_en DESC)",
    "CREATE INDEX IF NOT EXISTS idx_workspaces_user_fij ON workspaces (user_id, fijado DESC, creada_en DESC)",
];

/// [109A-4 F3] Punto de compactación manual de una conversación.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompactacionPersistida {
    /// RFC3339 del momento en que se compactó.
    pub compactado_en: String,
    /// Resumen del tramo (texto ya enmarcado por el núcleo).
    pub resumen: String,
}

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
/// [119A-3 F1] `creada_en` permite el criterio «Created at» en el front.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct InfoConversacion {
    pub id: Uuid,
    pub titulo: String,
    pub archivada: bool,
    pub creada_en: DateTime<Utc>,
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
    /// [129A-7] Id del turno (para agrupar cambios por turno en el panel
    /// "Cambios" sin depender del timestamp).
    pub turno_id: String,
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
    /// [139A-8 F2] (R1/R2) Ejecuta `op` con la conexión en el pool de hilos
    /// bloqueantes de tokio en vez del worker async: rusqlite es síncrono y
    /// una consulta larga (turno sobre miles de mensajes) retenía el worker y
    /// elevaba el p99 de SSE, Tauri y `preparar_turno`. La conexión es `Send`,
    /// así que mover el `Arc` al cierre es seguro; el `Mutex` sigue
    /// serializando el acceso intra-proceso y el `busy_timeout` de 5 s cubre
    /// el `SQLITE_BUSY` entre procesos (CLI + escritorio sobre la misma BD).
    /// Los métodos inherentes síncronos de una sola fila (PK) no lo usan: son
    /// microsegundos y también los llaman contextos síncronos (CLI/Tauri).
    async fn con_conn<F, T>(&self, op: F) -> HarnessResult<T>
    where
        F: FnOnce(&Connection) -> HarnessResult<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let c = conn
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            op(&c)
        })
        .await
        .map_err(|e| Error::Persistencia(format!("hilo bloqueante: {e}")))?
    }
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
        conn.execute_batch(ESQUEMA_MEMORIA)
            .map_err(|e| Error::Persistencia(format!("esquema inicial (memoria): {e}")))?;
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
        Self::migrar_memoria_por_proyecto(&conn)?;
        Ok(conn)
    }

    /// `true` si `memoria` ya tiene la columna `workspace_id` ([109A-2]).
    fn memoria_ya_tiene_ambito(conn: &Connection) -> HarnessResult<bool> {
        let mut stmt = conn
            .prepare("PRAGMA table_info(memoria)")
            .map_err(|e| Error::Persistencia(format!("PRAGMA table_info(memoria): {e}")))?;
        let columnas = stmt
            .query_map([], |f| f.get::<_, String>(1))
            .map_err(|e| Error::Persistencia(format!("PRAGMA table_info(memoria): {e}")))?;
        for columna in columnas {
            if columna.map_err(|e| Error::Persistencia(e.to_string()))? == "workspace_id" {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// [109A-2] Pasa `memoria` a memoria por proyecto recreando la tabla una
    /// sola vez.
    ///
    /// La tabla anterior declaraba `PRIMARY KEY (user_id, clave)`, que impide
    /// tener la misma clave en dos ámbitos, y SQLite no permite alterar una
    /// PK: la única vía es copiar a una tabla nueva. Los recuerdos existentes
    /// conservan su contenido y pasan al ámbito global (`''`), que es
    /// exactamente lo que eran antes de esta feature. Idempotente (si la
    /// columna ya existe no toca nada) y atómica: la transacción revierte
    /// sola si algo falla, y el error se propaga en vez de perderse.
    fn migrar_memoria_por_proyecto(conn: &Connection) -> HarnessResult<()> {
        if Self::memoria_ya_tiene_ambito(conn)? {
            return Ok(());
        }
        let tx = conn
            .unchecked_transaction()
            .map_err(|e| Error::Persistencia(format!("migración memoria: {e}")))?;
        tx.execute_batch(
            "DROP TABLE IF EXISTS memoria_pre_109a2;
             ALTER TABLE memoria RENAME TO memoria_pre_109a2;",
        )
        .and_then(|()| tx.execute_batch(ESQUEMA_MEMORIA))
        .and_then(|()| {
            tx.execute_batch(
                "INSERT INTO memoria
                     (user_id, workspace_id, clave, contenido, actualizada_en, origen, usos, ultimo_uso)
                 SELECT user_id, '', clave, contenido, actualizada_en, origen, usos, ultimo_uso
                   FROM memoria_pre_109a2;
                 DROP TABLE memoria_pre_109a2;",
            )
        })
        .map_err(|e| Error::Persistencia(format!("migración memoria por proyecto: {e}")))?;
        tx.commit()
            .map_err(|e| Error::Persistencia(format!("migración memoria por proyecto: {e}")))?;
        Ok(())
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

#[cfg(test)]
mod pruebas {
    //! [139A-8 F5n/R4] Red de regresión: los listados calientes usan índice
    //! (nada de `SCAN` en `EXPLAIN QUERY PLAN`). Si una consulta nueva barre
    //! tabla, este test la señala y el fix es un índice en `MIGRACIONES`.
    use super::*;

    fn plan_de(conn: &Connection, sql: &str) -> Vec<String> {
        conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .expect("EXPLAIN preparable")
            .query_map([], |f| f.get::<_, String>(3))
            .expect("EXPLAIN ejecutable")
            .map(|r| r.expect("fila del plan"))
            .collect()
    }

    #[test]
    fn listados_calientes_usan_indice() {
        let bd = PersistenciaSqlite::en_memoria().expect("BD en memoria");
        let conn = bloquear(&bd.conn);
        // Las 4 consultas de la auditoría §R4 + las que cubren los otros dos
        // índices nuevos (logs de tarea, sidebar de áreas) + controles con
        // índice preexistente (eventos, mensajes).
        let mut malas = Vec::new();
        for sql in [
            "SELECT id FROM conversaciones WHERE user_id = 'u' ORDER BY actualizada_en DESC",
            "SELECT c.id FROM conversaciones c LEFT JOIN workspaces w ON w.id = c.workspace_id AND w.user_id = c.user_id WHERE c.user_id = 'u' ORDER BY c.actualizada_en DESC",
            "SELECT a.tool FROM acciones a JOIN turnos t ON t.id = a.turno_id WHERE t.conversacion_id = 'c' ORDER BY t.creado_en, a.id",
            "SELECT provider FROM turnos WHERE conversacion_id = 'c' ORDER BY creado_en DESC LIMIT 1",
            "SELECT id FROM tareas WHERE user_id = 'u' ORDER BY creado_en ASC",
            "SELECT id FROM tarea_logs WHERE tarea_id = 't' ORDER BY ejecutada_en DESC LIMIT 10",
            "SELECT id FROM workspaces WHERE user_id = 'u' ORDER BY fijado DESC, creada_en DESC",
            "SELECT id FROM eventos_turno WHERE turno_id = 't' ORDER BY id",
            "SELECT id FROM mensajes WHERE conversacion_id = 'c' ORDER BY creado_en ASC",
        ] {
            for linea in plan_de(&conn, sql) {
                if linea.contains("SCAN") {
                    malas.push(format!("{sql} -> {linea}"));
                }
            }
        }
        assert!(malas.is_empty(), "barridos de tabla:\n{}", malas.join("\n"));
    }
}
