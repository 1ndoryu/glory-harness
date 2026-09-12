//! Log de proceso + panic hook ([129A-4 F3]).
//!
//! Sin esto, un turno atascado o un panic en una tarea de fondo no deja
//! rastro: solo se veía `Running DevCommand` en el log de Tauri. Este módulo
//! escribe errores y panics en `%APPDATA%/glory-harness/logs/errores.log`
//! (rotación simple a 1 MB) y los duplica a stderr (fail-loud, nunca
//! silencioso). Solo std: sin dependencias nuevas.
//!
//! El log por turno (eventos) vive en SQLite (`eventos_turno`); aquí solo van
//! fallos de infraestructura (persistencia que falla, panics, errores fatales).

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// Tope del log antes de rotar (1 MB: disco C ajustado, ver roadmap).
const TOPE_BYTES: u64 = 1024 * 1024;

/// Ruta resuelta una vez al instalar (para no recomputar en cada `anotar`).
static RUTA: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();

/// Resuelve el archivo de log (APPDATA en Windows; temporal como respaldo).
fn resolver_ruta() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| Some(std::env::temp_dir()))
        .map(|b| b.join("glory-harness").join("logs"));
    let dir = base?;
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("errores.log"))
}

/// Rota `errores.log` → `errores.1.log` si supera el tope (best-effort).
fn rotar_si_grande(ruta: &std::path::Path) {
    let grande = std::fs::metadata(ruta)
        .map(|m| m.len() > TOPE_BYTES)
        .unwrap_or(false);
    if grande {
        let anterior = ruta.with_extension("1.log");
        let _ = std::fs::remove_file(&anterior);
        let _ = std::fs::rename(ruta, &anterior);
    }
}

/// Instala el log + el panic hook. Llamar al inicio de `main()`.
pub fn instalar() {
    let ruta = resolver_ruta();
    if let Some(ref r) = ruta {
        rotar_si_grande(r);
    }
    let _ = RUTA.set(Mutex::new(ruta));
    std::panic::set_hook(Box::new(|info| {
        let lugar = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "<desconocido>".to_string());
        let causa = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str))
            .unwrap_or("<payload no textual>");
        anotar(&format!("PANIC en {lugar}: {causa}"));
    }));
}

/// Anota un error en el archivo + stderr (best-effort: nunca lanza).
pub fn anotar(mensaje: &str) {
    let linea = format!(
        "[{}] {mensaje}\n",
        chrono_fecha(),
        mensaje = mensaje.replace('\n', " ")
    );
    eprint!("{linea}");
    let ruta = RUTA.get().and_then(|m| m.lock().ok()).and_then(|g| g.clone());
    if let Some(r) = ruta {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(r) {
            let _ = f.write_all(linea.as_bytes());
        }
    }
}

/// Fecha local simple sin dependencias (el contenido importa, no el formato).
fn chrono_fecha() -> String {
    // Segundos Unix: suficiente para ordenar líneas del mismo proceso.
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}
