//! Subcomando `session` ([069A-2]): gestiona las conversaciones durables del
//! CLI (`list`/`ver`/`resume`/`borrar`) sobre la misma BD sqlite y `user_id`
//! estable que `chat` usa desde 069A-2. `resume` recompone el contexto
//! (mensajes → transcripción de export) y abre el REPL sobre esa conversación.

use std::sync::Arc;

use glory_harness_core::AgentPersistence;
use uuid::Uuid;

use crate::persistencia_sqlite::{InfoConversacion, PersistenciaSqlite};
use crate::run::{abrir_tiendas_durables, OpcionesRun};

/// Resultado del subcomando `session`: `Uso` = error de argumentos (exit 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SalidaSesion {
    Ok,
    Uso,
}

/// `glory-harness session <list|ver|resume|borrar> [args]`.
pub async fn sesion(args: &[String], opciones: OpcionesRun) -> Result<SalidaSesion, String> {
    let accion = args.first().map(String::as_str).ok_or_else(|| {
        "uso: glory-harness session <list|ver|resume|borrar> [id]".to_string()
    })?;
    // `resume` necesita las opciones de provider/modelo para el REPL; el
    // resto solo toca la BD (las ignora).
    match accion {
        "list" | "listar" => accion_listar().await.map(|()| SalidaSesion::Ok),
        "ver" | "show" => accion_ver(args).await,
        "resume" | "retomar" => accion_resume(args, opciones).await,
        "borrar" | "rm" | "delete" => accion_borrar(args),
        otra => Err(format!(
            "session: acción desconocida '{otra}' (list|ver|resume|borrar)"
        )),
    }
}

/// Abre la tienda durable compartida (misma que `chat`/`schedule`).
fn abrir() -> Result<(Arc<PersistenciaSqlite>, Uuid), String> {
    abrir_tiendas_durables()
}

/// Extrae el id de conversación del argumento 2 (`session <acción> <id>`);
/// un id ausente o mal formado es error de uso (exit 2), no fallo interno.
/// No imprime: el uso lo imprime el llamador una sola vez.
fn extraer_id(args: &[String]) -> Result<Uuid, SalidaSesion> {
    args.get(1)
        .and_then(|s| Uuid::parse_str(s.trim()).ok())
        .ok_or(SalidaSesion::Uso)
}

/// La conversación debe existir y ser del usuario estable (una ajena se
/// rechaza sin revelar nada más: mismo error que si no existiera).
fn conversacion_propia(
    tiendas: &PersistenciaSqlite,
    user_id: Uuid,
    id: Uuid,
) -> Result<InfoConversacion, String> {
    tiendas
        .conversaciones_listar(user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("la conversación {id} no existe"))
}

/// `session list`: una línea por conversación (recientes primero).
async fn accion_listar() -> Result<(), String> {
    let (tiendas, user_id) = abrir()?;
    let lista = tiendas
        .conversaciones_listar(user_id)
        .map_err(|e| e.to_string())?;
    if lista.is_empty() {
        println!("(sin conversaciones; `chat` crea la primera)");
        return Ok(());
    }
    for c in &lista {
        let estado = if c.archivada { "arch" } else { "activa" };
        let mensajes = tiendas
            .listar_mensajes(c.id)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        println!(
            "{} [{}] \"{}\" ({} msgs, {})",
            c.id.as_hyphenated(),
            estado,
            c.titulo,
            mensajes,
            c.actualizada_en.format("%d-%m %H:%M"),
        );
    }
    Ok(())
}

/// `session ver <id>`: vuelca los mensajes (rol + texto).
async fn accion_ver(args: &[String]) -> Result<SalidaSesion, String> {
    let id = match extraer_id(args) {
        Ok(id) => id,
        Err(u) => return Ok(u),
    };
    let (tiendas, user_id) = abrir()?;
    conversacion_propia(&tiendas, user_id, id)?;
    let mensajes = tiendas
        .listar_mensajes(id)
        .await
        .map_err(|e| e.to_string())?;
    if mensajes.is_empty() {
        println!("(conversación vacía)");
        return Ok(SalidaSesion::Ok);
    }
    for m in &mensajes {
        let quien = if m.rol == "user" { "tú" } else { "agente" };
        println!("[{quien}] {}", m.contenido);
    }
    Ok(SalidaSesion::Ok)
}

/// `session resume <id>`: abre el REPL sobre la conversación (el contexto se
/// recompone de sus mensajes; ver `chat_resume`).
async fn accion_resume(args: &[String], opciones: OpcionesRun) -> Result<SalidaSesion, String> {
    let id = match extraer_id(args) {
        Ok(id) => id,
        Err(u) => return Ok(u),
    };
    crate::chat::chat_resume(opciones, id).await?;
    Ok(SalidaSesion::Ok)
}

/// `session borrar <id>`: elimina la conversación propia (sin confirmación:
/// el CLI no es interactivo aquí; el operador ya escribió el id).
fn accion_borrar(args: &[String]) -> Result<SalidaSesion, String> {
    let id = match extraer_id(args) {
        Ok(id) => id,
        Err(u) => return Ok(u),
    };
    let (tiendas, user_id) = abrir()?;
    conversacion_propia(&tiendas, user_id, id)?;
    if tiendas
        .conversacion_eliminar(id, user_id)
        .map_err(|e| e.to_string())?
    {
        println!("conversación {id} borrada");
        Ok(SalidaSesion::Ok)
    } else {
        Err(format!("no se pudo borrar la conversación {id}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uso_rechaza_id_malformado() {
        let args = vec!["ver".to_string(), "no-es-uuid".to_string()];
        assert_eq!(extraer_id(&args), Err(SalidaSesion::Uso));
    }

    #[test]
    fn uso_rechaza_falta_de_id() {
        let args = vec!["borrar".to_string()];
        assert_eq!(extraer_id(&args), Err(SalidaSesion::Uso));
    }

    #[test]
    fn uso_acepta_uuid_completo() {
        let id = Uuid::new_v4();
        let args = vec!["ver".to_string(), id.to_string()];
        assert_eq!(extraer_id(&args), Ok(id));
    }
}
