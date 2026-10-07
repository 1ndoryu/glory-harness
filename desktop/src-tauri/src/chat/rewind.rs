//! [Partición limite-lineas] Rewind y cambios de archivo del desktop:
//! `rewind_conversacion`, `restaurar_archivos_tramo`, `cambios_archivo`,
//! `rechazar_cambio`. Extraído de `conversaciones.rs` (god-object de 639
//! líneas efectivas). La `CargaConversacion` reconstruida vive en `crud.rs`.

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

use super::conversaciones::conv_id_de_panel_obligatoria;
use super::crud::{CargaConversacion, UsoTurnoPersistido, usos_turno};

/// [039A-3 P2] Rebobina la conversación hasta un mensaje de usuario.
///
/// Con `editar=false` (volver a este punto) conserva el mensaje objetivo como
/// último mensaje; con `editar=true` lo borra para reescribirlo (editar+enviar
/// hace rewind aquí y luego `enviar_turno` persiste el texto nuevo). Borra en
/// una transacción mensajes/turnos/acciones del tramo posterior. Falla con
/// turno en curso y si el mensaje no es de la conversación actual del panel.
/// [039A-3 P5] `panel_id` opcional (default `principal`).
/// Devuelve la `CargaConversacion` resultante para que el front se reconcilie
/// (repintar sin recargar).
#[tauri::command]
pub(crate) async fn rewind_conversacion(
    estado: State<'_, Estado>,
    hasta_mensaje_id: String,
    editar: bool,
    panel_id: Option<String>,
) -> Result<CargaConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let msg_id = Uuid::parse_str(hasta_mensaje_id.trim())
        .map_err(|_| "id de mensaje inválido".to_string())?;
    /* [069A-7] Rebobinar exige conversación real (mensajes que podar); un
     * panel en borrador no tiene nada que rebobinar. */
    let conv_id = conv_id_de_panel_obligatoria(&sesion, &panel_id)?;
    /* [039A-3 P3] El rewind devuelve los ids de los turnos borrados: con ellos
     * el vault localiza las rutas que tocó el tramo. En "volver a punto"
     * (editar=false) el tramo queda pendiente de una restauración EXPLÍCITA;
     * en "editar+reenviar" (editar=true) se limpia porque el reenvío inmediato
     * va a reescribir los archivos (ofrecer restaurar sería incoherente). */
    let turnos_tramo = sesion
        .persistencia
        .rewind_conversacion(conv_id, msg_id, sesion.user_id, editar)
        .map_err(|e| e.to_string())?;
    let archivos_tramo: Vec<String> = if editar {
        Vec::new()
    } else {
        sesion
            .vault
            .archivos_del_tramo(&turnos_tramo)
            .into_iter()
            .map(|e| e.ruta_relativa)
            .collect()
    };
    /* [039A-3 P5] El tramo rebobinado queda pendiente en el PANEL (la
     * restauración explícita opera sobre la conversación de ese panel). */
    if let Ok(mut g) = sesion.paneles.lock() {
        if let Some(d) = g.get_mut(&panel_id) {
            d.tramo_rewind = if editar {
                None
            } else {
                Some(TramoRewind {
                    archivos: archivos_tramo.clone(),
                    turnos: turnos_tramo,
                })
            };
        }
    }
    // Reconstruir la carga resultante (igual que cargar_conversacion).
    let titulo = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == conv_id)
        .map(|c| c.titulo)
        .ok_or_else(|| "conversación no encontrada".to_string())?;
    let mensajes = sesion
        .persistencia
        .listar_mensajes(conv_id)
        .await
        .map_err(|e| e.to_string())?;
    let acciones = sesion
        .persistencia
        .acciones_por_conversacion(conv_id)
        .map_err(|e| e.to_string())?;
    let ultimo_uso = sesion
        .persistencia
        .turno_ultimo_uso_por_conversacion(conv_id)
        .map_err(|e| e.to_string())?
        .map(
            |(provider, modelo, tokens_prompt, tokens_complecion)| UsoTurnoPersistido {
                provider,
                modelo,
                tokens_prompt,
                tokens_complecion,
            },
        );
    Ok(CargaConversacion {
        id: conv_id,
        titulo,
        mensajes,
        acciones,
        ultimo_uso,
        usos_turno: usos_turno(&sesion, conv_id)?,
        archivos_tramo,
    })
}

/// [039A-3 P3] Restaura los archivos del último tramo rebobinado ("volver a
/// punto"): acción EXPLÍCITA, nunca automática. Comprueba la fuente de cada
/// ruta contra su último respaldo GLOBAL (si alguien editó fuera del harness,
/// NO toca y avisa). Nunca borra archivos. Tras restaurar se limpia el tramo
/// por restaurar (las escrituras deshechas se podan del índice y se hace GC).
/// [039A-3 P5] `panel_id` opcional (default `principal`): opera sobre el tramo
/// por restaurar de la conversación de ESE panel.
#[tauri::command]
pub(crate) fn restaurar_archivos_tramo(
    estado: State<'_, Estado>,
    panel_id: Option<String>,
) -> Result<RestauracionTramo, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let tramo = sesion
        .paneles
        .lock()
        .map(|g| g.get(&panel_id).and_then(|d| d.tramo_rewind.clone()))
        .map_err(|_| "sesión bloqueada".to_string())?
        .ok_or_else(|| "no hay ningún tramo rebobinado pendiente de restaurar".to_string())?;
    let archivos = tramo.archivos.clone();
    let resultado = sesion.vault.restaurar_tramo(&tramo.turnos);
    /* Tras restaurar (o al decidir no hacerlo por completo) el tramo ya no
     * está pendiente: la próxima "restaurar" volvería a intentar los mismos
     * turnos (no-op tras la purga), así que se limpia el estado del panel. */
    if let Ok(mut g) = sesion.paneles.lock() {
        if let Some(d) = g.get_mut(&panel_id) {
            d.tramo_rewind = None;
        }
    }
    Ok(RestauracionTramo {
        archivos,
        restaurados: resultado.restaurados,
        omitidos: resultado.omitidos,
    })
}

/// [129A-7] Resultado de la restauración explícita de un tramo, para que
/// el front muestre qué se restauró y qué se omitió (y por qué).
#[derive(serde::Serialize)]
pub(crate) struct RestauracionTramo {
    /// Rutas del tramo que se intentaron restaurar (para el aviso).
    archivos: Vec<String>,
    restaurados: Vec<vault::RestauracionArchivo>,
    omitidos: Vec<vault::RestauracionArchivo>,
}

/// [129A-7] Un archivo tocado por el agente en un turno (panel "Cambios"):
/// primera escritura de cada (turno, ruta) del índice del vault.
#[derive(serde::Serialize)]
pub(crate) struct CambioArchivoTurno {
    turno_id: String,
    ruta: String,
    herramienta: String,
    en_ms: i64,
}

/// [129A-7] Lista los archivos que escribió el agente en una conversación,
/// agrupados por turno (primera escritura de cada (turno, ruta), en orden de
/// escritura). Solo lectura: no se bloquea con turno en curso (el panel se
/// refresca en vivo). Funciona sin git (el vault respalda cada escritura).
#[tauri::command]
pub(crate) fn cambios_archivo(
    estado: State<'_, Estado>,
    conversacion_id: String,
) -> Result<Vec<CambioArchivoTurno>, String> {
    let sesion = sesion_actual(&estado)?;
    let conv =
        Uuid::parse_str(conversacion_id.trim()).map_err(|_| "id inválido".to_string())?;
    let mut vistos = std::collections::HashSet::new();
    let mut cambios = Vec::new();
    for e in sesion.vault.leer_indice(conv) {
        let clave = format!("{}|{}", e.turno_id, e.ruta_relativa);
        if vistos.insert(clave) {
            cambios.push(CambioArchivoTurno {
                turno_id: e.turno_id,
                ruta: e.ruta_relativa,
                herramienta: e.tool_name,
                en_ms: e.timestamp_ms,
            });
        }
    }
    Ok(cambios)
}

/// [129A-7] "Rechazar" del panel Cambios: restaura el archivo al contenido
/// previo de la PRIMERA escritura de (turno, ruta) y purga sus entradas.
/// Directo, sin confirmación (decisión de usuario 12-09). Misma comprobación
/// de fuente que el rewind: si alguien editó fuera del harness, NO toca y
/// avisa. Nunca borra archivos. Se bloquea con turno en curso (escribe disco).
#[tauri::command]
pub(crate) fn rechazar_cambio(
    estado: State<'_, Estado>,
    conversacion_id: String,
    turno_id: String,
    ruta: String,
) -> Result<vault::RestauracionArchivo, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let conv =
        Uuid::parse_str(conversacion_id.trim()).map_err(|_| "id inválido".to_string())?;
    let turno = Uuid::parse_str(turno_id.trim()).map_err(|_| "id inválido".to_string())?;
    let punto = sesion
        .vault
        .leer_indice(conv)
        .into_iter()
        .find(|e| e.turno_id == turno.as_hyphenated().to_string() && e.ruta_relativa == ruta)
        .ok_or_else(|| "ese cambio ya no está en el índice".to_string())?;
    let resultado = sesion.vault.restaurar_ruta(&punto)?;
    if resultado.estado == "restaurado" {
        sesion.vault.purgar_escritura(conv, turno, &ruta);
    }
    Ok(resultado)
}
