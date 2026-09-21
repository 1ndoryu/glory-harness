//! Comandos CRUD de conversaciones del desktop.

// [109A-6] El módulo vive en `chat/`: `super` ya no es la raíz del crate.
use crate::*;

/// [069A-7] Conversación actual de un panel. `Ok(None)` = el panel está en
/// borrador (sin conversación creada todavía); `Err` = panel inexistente.
/* [109A-6] `pub(crate)`: antes el módulo colgaba de la raíz y `pub(super)` ya
alcanzaba todo el crate; sigue usándose desde `main.rs` y `sesion.rs`. */
pub(crate) fn conv_id_de_panel(sesion: &Sesion, panel_id: &str) -> Result<Option<Uuid>, String> {
    sesion
        .paneles
        .lock()
        .map(|p| {
            p.get(panel_id)
                .map(|d| d.conversacion_id)
                .ok_or_else(|| format!("panel no encontrado: {panel_id}"))
        })
        .map_err(|_| "sesión bloqueada por otro turno".to_string())?
}

/// [069A-7] Id de conversación de un panel que DEBE tener una (los turnos
/// requieren conversación: el front crea antes de enviar). Devuelve error
/// claro si el panel está en borrador.
// [109A-6] `pub(crate)`: se usa desde `chat/turno.rs` y `sesion.rs`.
pub(crate) fn conv_id_de_panel_obligatoria(
    sesion: &Sesion,
    panel_id: &str,
) -> Result<Uuid, String> {
    conv_id_de_panel(sesion, panel_id)?.ok_or_else(|| {
        "no hay conversación en este panel: escribe el primer mensaje para crearla".to_string()
    })
}

/// [039A-3 P5] Abre un panel con una conversación dada (la deja como la que
/// muestra ese panel). No crea duplicados: si el panel ya existe, solo cambia
/// su conversación. El tramo pendiente se limpia (pertenece a la conversación
/// anterior del panel).
/// [069A-7] `Option<Uuid>`: `None` deja el panel en borrador (sin fila).
fn panel_poner_conversacion(
    sesion: &Sesion,
    panel_id: &str,
    conv_id: Option<Uuid>,
) -> Result<(), String> {
    sesion
        .paneles
        .lock()
        .map(|mut p| {
            let d = p.entry(panel_id.to_string()).or_insert(PanelDatos {
                conversacion_id: conv_id,
                turno_id: None,
                tramo_rewind: None,
            });
            d.conversacion_id = conv_id;
            d.tramo_rewind = None;
            Ok(())
        })
        .map_err(|_| "sesión bloqueada por otro turno".to_string())?
}

// --- F4: conversaciones (CRUD) ---

/// Crea una conversación y la deja como actual del panel (falla si hay turno
/// en curso). [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) fn conversacion_nueva(
    estado: State<'_, Estado>,
    titulo: Option<String>,
    panel_id: Option<String>,
) -> Result<InfoConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let titulo = titulo
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| "Nueva conversación".into());
    /* [069A-Proyectos] La conversación nueva nace dentro del área activa de
     * la carpeta actual (si es un proyecto registrado); si no, sin área. El
     * workspace_id se resuelve por ruta (nunca se duplica en `SesionComun`). */
    let area = area_activa(&sesion)?;
    let id = sesion
        .persistencia
        .conversacion_crear_en(sesion.user_id, &titulo, area.as_ref().map(|a| a.id))
        .map_err(|e| e.to_string())?;
    /* [039A-3 P5] La nueva conversación queda como actual SOLO de este panel.
     * [039A-3 P3] Conversación nueva = contexto nuevo: no hay tramo previo
     * que restaurar desde aquí (se limpia el del panel). */
    panel_poner_conversacion(&sesion, &panel_id, Some(id))?;
    Ok(InfoConversacion {
        id,
        titulo,
        archivada: false,
        creada_en: chrono::Utc::now(),
        actualizada_en: chrono::Utc::now(),
        workspace_id: None,
        workspace_nombre: None,
    })
}

/// [069A-Proyectos] Lista todas las conversaciones agrupables por proyecto.
/// Las conversaciones legacy conservan `workspace_id: null`.
#[tauri::command]
pub(crate) fn listar_conversaciones(
    estado: State<'_, Estado>,
) -> Result<Vec<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    sesion
        .persistencia
        .conversaciones_listar_con_proyecto(sesion.user_id)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
pub(crate) struct CargaConversacion {
    id: Uuid,
    titulo: String,
    mensajes: Vec<MensajePersistido>,
    /// [039A-1 04-09 H6] Acciones (tools) de la conversación en orden de
    /// ejecución, para repintar los bloques `.herramienta` al recargar.
    acciones: Vec<glory_harness::AccionRecuperada>,
    /// [039A-3 P1] Uso/modelo real del último turno (para repintar el pie de
    /// turno al recargar). `None` si no hay turno con uso registrado.
    /// Se conserva por compatibilidad; el front prefiere `usos_turno`.
    ultimo_uso: Option<UsoTurnoPersistido>,
    /// [20-09-2026] Uso/modelo real de TODOS los turnos (para repintar CADA
    /// pie de turno al recargar, no solo el último).
    usos_turno: Vec<UsoTurnoPorTurno>,
    /// [039A-3 P3] Archivos que tocó el último tramo rebobinado ("volver a
    /// punto"), listos para la acción EXPLÍCITA "restaurar archivos de este
    /// tramo". Vacío cuando la carga no viene de un rewind (no hay nada que
    /// restaurar). El front lo ofrece solo cuando no está vacío.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    archivos_tramo: Vec<String>,
}

/// [039A-3 P1] Uso real de un turno persistido (serializable al front).
#[derive(serde::Serialize)]
struct UsoTurnoPersistido {
    provider: String,
    modelo: String,
    tokens_prompt: u32,
    tokens_complecion: u32,
}

/// [20-09-2026] Uso real de un turno con su ancla temporal (para que el front
/// lo ancle a los mensajes de ESE turno en vez de pintar solo el último pie).
#[derive(serde::Serialize)]
struct UsoTurnoPorTurno {
    turno_en: String,
    provider: String,
    modelo: String,
    tokens_prompt: u32,
    tokens_complecion: u32,
}

/// [20-09-2026] Usos de todos los turnos de una conversación (ordenados por
/// `creado_en` desde la query). Auxiliar común de `cargar_conversacion` y del
/// rewind: la carga reconstruida tras "volver a punto" también repinta pies.
fn usos_turno(sesion: &Sesion, conv_id: Uuid) -> Result<Vec<UsoTurnoPorTurno>, String> {
    sesion
        .persistencia
        .usos_turno_por_conversacion(conv_id)
        .map_err(|e| e.to_string())
        .map(|usos| {
            usos.into_iter()
                .map(|u| UsoTurnoPorTurno {
                    turno_en: u.turno_en,
                    provider: u.provider,
                    modelo: u.modelo,
                    tokens_prompt: u.tokens_prompt,
                    tokens_complecion: u.tokens_complecion,
                })
                .collect()
        })
}

/// Carga una conversación como actual del panel con su historial (falla con
/// turno vivo). [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) async fn cargar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<CargaConversacion, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = sesion
        .persistencia
        .conversaciones_listar(sesion.user_id)
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|c| c.id == id)
        .map(|c| c.titulo)
        .ok_or_else(|| "conversación no encontrada".to_string())?;
    let mensajes = sesion
        .persistencia
        .listar_mensajes(id)
        .await
        .map_err(|e| e.to_string())?;
    let acciones = sesion
        .persistencia
        .acciones_por_conversacion(id)
        .map_err(|e| e.to_string())?;
    let ultimo_uso = sesion
        .persistencia
        .turno_ultimo_uso_por_conversacion(id)
        .map_err(|e| e.to_string())?
        .map(
            |(provider, modelo, tokens_prompt, tokens_complecion)| UsoTurnoPersistido {
                provider,
                modelo,
                tokens_prompt,
                tokens_complecion,
            },
        );
    /* [039A-3 P5] La conversación cargada queda como actual de este panel.
     * [039A-3 P3] Al cambiar de conversación se limpia el tramo pendiente de
     * restaurar (pertenece a la conversación anterior): su restauración ya no
     * es accesible desde aquí. */
    panel_poner_conversacion(&sesion, &panel_id, Some(id))?;
    Ok(CargaConversacion {
        id,
        titulo,
        mensajes,
        acciones,
        ultimo_uso,
        usos_turno: usos_turno(&sesion, id)?,
        archivos_tramo: Vec::new(),
    })
}

/// Renombra una conversación (`false` = no existe o no es del usuario).
#[tauri::command]
pub(crate) fn renombrar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    titulo: String,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let titulo = titulo.trim();
    if titulo.is_empty() {
        return Err("título vacío".into());
    }
    sesion
        .persistencia
        .conversacion_renombrar(id, sesion.user_id, titulo)
        .map_err(|e| e.to_string())
}

/// Archiva/desarchiva (`false` = no existe o no es del usuario).
#[tauri::command]
pub(crate) fn archivar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    archivada: bool,
) -> Result<bool, String> {
    let sesion = sesion_actual(&estado)?;
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_archivar(id, sesion.user_id, archivada)
        .map_err(|e| e.to_string())
}

/// Elimina con mensajes y turnos; si era la actual del panel, ancla la más
/// reciente no-archivada restante o deja el panel en borrador (`None`) si no
/// queda ninguna. [069A-7] Create-on-write: NO se crea una vacía de reemplazo.
/// [039A-3 P5] `panel_id` opcional (default `principal`).
#[tauri::command]
pub(crate) async fn eliminar_conversacion(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<Option<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let id = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversacion_eliminar(id, sesion.user_id)
        .map_err(|e| e.to_string())?;
    /* [039A-3 P3] Al eliminar la conversación se limpia su índice del vault y
     * se hace GC de los hashes que quedaron huérfanos. */
    sesion.vault.eliminar_conversacion(id);
    /* [209A-1 F4-resto] Reap: las consolas vivas de la conversación
     * borrada se matan (su pump archiva el transcript acotado). */
    if let Some(ejecutor) = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
    {
        let matadas = ejecutor.matar_por_conversacion(id).await;
        if matadas > 0 {
            eprintln!(
                "[glory-harness-desktop] reap al eliminar conversación {id}: {matadas} matada(s)"
            );
        }
    }
    /* [039A-3 P5] "Era la actual" se decide POR PANEL: si este panel tenía esa
     * conversación cargada, hay que re-anclar el panel. */
    let actual = conv_id_de_panel(&sesion, &panel_id)?;
    if actual == Some(id) {
        /* El tramo pendiente pertenecía a la conversación borrada: se limpia.
         * [069A-7] Se ancla la más reciente no-archivada restante (lista en
         * orden `actualizada_en DESC`) o se deja el panel en borrador. */
        let restante = sesion
            .persistencia
            .conversaciones_listar(sesion.user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| !c.archivada);
        if let Ok(mut g) = sesion.paneles.lock() {
            if let Some(d) = g.get_mut(&panel_id) {
                d.conversacion_id = restante.as_ref().map(|c| c.id);
                d.tramo_rewind = None;
            }
        }
        return Ok(restante);
    }
    info_de_panel(&sesion, &panel_id).map(|i| i.conversacion)
}

/// [119A-2 F4] Archiva/desarchiva TODAS las conversaciones de un proyecto.
/// Devuelve cuántas cambió. Reversible hilo a hilo (igual que el single,
/// sin guard de turno).
#[tauri::command]
pub(crate) fn archivar_conversaciones_proyecto(
    estado: State<'_, Estado>,
    id: String,
    archivada: bool,
) -> Result<usize, String> {
    let sesion = sesion_actual(&estado)?;
    let ws = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    sesion
        .persistencia
        .conversaciones_archivar_por_workspace(sesion.user_id, ws, archivada)
        .map_err(|e| e.to_string())
}

/// [119A-2 F4] Elimina TODAS las conversaciones de un proyecto con sus
/// mensajes y turnos; limpia el vault por hilo y re-ancla el panel si
/// mostraba una de las borradas (la más reciente no-archivada restante o
/// borrador si no queda ninguna, espejo de `eliminar_conversacion`).
#[tauri::command]
pub(crate) async fn eliminar_conversaciones_proyecto(
    estado: State<'_, Estado>,
    id: String,
    panel_id: Option<String>,
) -> Result<Option<InfoConversacion>, String> {
    let sesion = sesion_actual(&estado)?;
    if estado.turno.lock().map(|t| t.activo).unwrap_or(true) {
        return Err("hay un turno en curso".into());
    }
    let panel_id = normalizar_panel(panel_id);
    let ws = Uuid::parse_str(id.trim()).map_err(|_| "id inválido".to_string())?;
    let borradas = sesion
        .persistencia
        .conversaciones_eliminar_por_workspace(sesion.user_id, ws)
        .map_err(|e| e.to_string())?;
    for b in &borradas {
        sesion.vault.eliminar_conversacion(*b);
    }
    /* [209A-1 F4-resto] Reap por conversación borrada (espejo del single):
     * las vivas de cada hilo se matan; el resto sigue. */
    if let Some(ejecutor) = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
    {
        let mut matadas = 0;
        for b in &borradas {
            matadas += ejecutor.matar_por_conversacion(*b).await;
        }
        if matadas > 0 {
            eprintln!(
                "[glory-harness-desktop] reap al eliminar proyecto {ws}: {matadas} matada(s)"
            );
        }
    }
    let actual = conv_id_de_panel(&sesion, &panel_id)?;
    if actual.is_some_and(|a| borradas.contains(&a)) {
        let restante = sesion
            .persistencia
            .conversaciones_listar(sesion.user_id)
            .map_err(|e| e.to_string())?
            .into_iter()
            .find(|c| !c.archivada);
        if let Ok(mut g) = sesion.paneles.lock() {
            if let Some(d) = g.get_mut(&panel_id) {
                d.conversacion_id = restante.as_ref().map(|c| c.id);
                d.tramo_rewind = None;
            }
        }
        return Ok(restante);
    }
    info_de_panel(&sesion, &panel_id).map(|i| i.conversacion)
}

/// [209A-1 F4-resto] Mata UNA consola viva por su `id_ejecucion` (la × de
/// la tab Consola sobre una entrada viva). Devuelve `true` si estaba viva
/// y se mató; `false` si no existe o ya terminó (idempotente, sin error:
/// la vista ya la marca como terminada al llegar `consola_fin`).
#[tauri::command]
pub(crate) async fn consola_matar(
    estado: State<'_, Estado>,
    id_ejecucion: String,
) -> Result<bool, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let viva = ejecutor
        .lista()
        .await
        .map_err(|e| e.to_string())?
        .into_iter()
        .any(|c| c.id_ejecucion == id && c.viva);
    if !viva {
        return Ok(false);
    }
    ejecutor.matar(id).await.map_err(|e| e.to_string())?;
    Ok(true)
}

/// [219A-3] Vista de consola para la sub-barra de la tab Consola (vivas +
/// recientes). Se serializa con los mismos nombres que el endpoint web
/// (`id_ejecucion`, `comando`, `viva`, `codigo_salida`).
#[derive(serde::Serialize)]
pub(crate) struct InfoConsolaTauri {
    id_ejecucion: String,
    comando: String,
    viva: bool,
    codigo_salida: Option<i32>,
}

/// [219A-3] Transcript retenido para el backfill de la tab (mismos nombres
/// que el endpoint web; `flujo` serializa `stdout`/`stderr` por el contrato
/// de `FlujoConsola`).
#[derive(serde::Serialize)]
pub(crate) struct LineaConsolaTauri {
    flujo: glory_harness_core::evento::FlujoConsola,
    linea: String,
}

#[derive(serde::Serialize)]
pub(crate) struct TranscriptConsolaTauri {
    id_ejecucion: String,
    comando: String,
    viva: bool,
    codigo_salida: Option<i32>,
    lineas: Vec<LineaConsolaTauri>,
}

/// [219A-3] `consolas_listar` — vivas primero + recientes archivadas (el
/// runner ordena por inicio). Backfill de la sub-barra al abrir la tab.
#[tauri::command]
pub(crate) async fn consolas_listar(
    estado: State<'_, Estado>,
) -> Result<Vec<InfoConsolaTauri>, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let lista = ejecutor.lista().await.map_err(|e| e.to_string())?;
    Ok(lista
        .into_iter()
        .map(|c| InfoConsolaTauri {
            id_ejecucion: c.id_ejecucion,
            comando: c.comando,
            viva: c.viva,
            codigo_salida: c.codigo_salida,
        })
        .collect())
}

/// [219A-3] `consola_salida` — transcript retenido por `id_ejecucion`.
/// Error claro si el runner ya no retiene ese id.
#[tauri::command]
pub(crate) async fn consola_salida(
    estado: State<'_, Estado>,
    id_ejecucion: String,
) -> Result<TranscriptConsolaTauri, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    let t = ejecutor.salida(id).await.map_err(|e| e.to_string())?;
    Ok(TranscriptConsolaTauri {
        id_ejecucion: t.id_ejecucion,
        comando: t.comando,
        viva: t.viva,
        codigo_salida: t.codigo_salida,
        lineas: t
            .lineas
            .into_iter()
            .map(|l| LineaConsolaTauri {
                flujo: l.flujo,
                linea: l.linea,
            })
            .collect(),
    })
}

/// [219A-3] `consola_escribir` — bytes crudos al stdin de una viva.
/// Devuelve los bytes aceptados. Vacío o >64 KB se rechaza aquí (mismo tope
/// que el runner); terminada o desconocida → error claro del runner.
#[tauri::command]
pub(crate) async fn consola_escribir(
    estado: State<'_, Estado>,
    id_ejecucion: String,
    texto: String,
) -> Result<usize, String> {
    use glory_harness_core::ports::EjecutorComando;
    let sesion = sesion_actual(&estado)?;
    let id = id_ejecucion.trim();
    if id.is_empty() {
        return Err("id de ejecución vacío".into());
    }
    if texto.is_empty() {
        return Err("texto vacío: nada que escribir".into());
    }
    if texto.len() > 64 * 1024 {
        return Err("texto mayor de 64 KB: trocéalo en varias escrituras".into());
    }
    let ejecutor = sesion
        .comun
        .lock()
        .ok()
        .and_then(|g| g.ejecutor.clone())
        .ok_or_else(|| "sesión sin ejecutor".to_string())?;
    ejecutor
        .escribir(id, texto.as_bytes())
        .await
        .map_err(|e| e.to_string())
}

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
