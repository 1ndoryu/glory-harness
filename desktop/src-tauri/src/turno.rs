//! Comandos de turno del desktop (enviar/cancelar + auxiliares F5).

use super::*;

/// Marca el turno como terminado (lo llama la propia tarea al cerrar).
fn marcar_turno_terminado(estado: &Estado) {
    if let Ok(mut t) = estado.turno.lock() {
        t.handle = None;
        t.activo = false;
    }
}

/// Ejecuta un turno real y reemite cada `AgenteEvento` a la UI.
/// Emite `agente-evento` por evento y `turno-fin` (`{ok, error?}`) al cerrar.
/// [039A-3 P5] El turno actúa sobre el panel que lo lanza (`panel_id`, default
/// `principal`): M1 tiene UN turno a la vez (guard global), así que el panel
/// que está ejecutando es siempre el destino de los eventos que emite esta
/// ventana (no hace falta etiquetar el payload: nunca hay 2 streams vivos).
///
/// [109A-4 F4] `solo_lectura` = este turno corre en modo `meta` (deniega toda
/// tool con efecto) SIN cambiar el modo de la sesión. La UI lo usa para
/// `/meta <texto>`. El transporte no elige el modo: manda un booleano y el
/// backend traduce (una superficie con menos grados de libertad).
#[tauri::command]
pub(crate) async fn enviar_turno(
    estado: State<'_, Estado>,
    window: tauri::Window,
    mensaje: String,
    panel_id: Option<String>,
    solo_lectura: Option<bool>,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    let panel_id = normalizar_panel(panel_id);
    reclamar_turno(&estado)?;
    /* `Option<String>` (no `&str`): el valor tiene que moverse a la tarea. */
    let modo_turno: Option<String> = if solo_lectura == Some(true) {
        Some("meta".to_string())
    } else {
        None
    };
    let PaqueteTurno {
        conv_id,
        turno_id,
        historial_previo,
        mensaje_efectivo,
        runtime,
    } = preparar_paquete(&sesion, &panel_id, mensaje, modo_turno.as_deref()).await?;
    let (tx_ev, rx_ev) = tokio::sync::mpsc::channel::<AgenteEvento>(64);
    let w = window.clone();
    fijar_contexto_turno(&sesion, &panel_id, conv_id, turno_id);
    let uso_accum = std::sync::Arc::new(std::sync::Mutex::new(UsoAcumulado::default()));
    let uso_reenvio = Arc::clone(&uso_accum);
    let w_fw = w.clone();
    let handle = tauri::async_runtime::spawn(async move {
        // Al cerrar (ok, fallo o abort) se libera el flag para el próximo turno.
        // Reenvío en la misma tarea: el loop termina con Done (último evento).
        // [039A-3 P1] Se acumula el Usage real que emite el núcleo (cada
        // llm_llamada emite uno parcial; un turno con N tools acumula N) y se
        // propaga al cierre para persistirlo en `turnos`.
        let reenvio = tauri::async_runtime::spawn(reenviar_eventos(w_fw, rx_ev, uso_reenvio));
        let resultado = runtime
            .ejecutar_turno_con_modo(
                PeticionTurno {
                    user_id: sesion.user_id,
                    turno_id,
                    conversacion_id: conv_id,
                    historial: historial_previo,
                    mensaje_usuario: mensaje_efectivo,
                    modo_forzado: modo_turno.as_deref(),
                },
                &tx_ev,
            )
            .await;
        drop(tx_ev);
        let _ = reenvio.await;
        cerrar_turno(&w, &sesion, turno_id, resultado, &uso_accum);
        terminar_turno(&w, &sesion, &panel_id);
    });
    registrar_turno(&estado, handle)
}

/// [079A-1 F5] Turno preparado y listo para lanzar (paquete que
/// `preparar_paquete` entrega a `enviar_turno`).
struct PaqueteTurno {
    conv_id: Uuid,
    turno_id: Uuid,
    historial_previo: Vec<AiMessage>,
    mensaje_efectivo: String,
    runtime: Arc<AgentRuntime>,
}

/// [079A-1 F5] Reserva el turno global (auxiliar de `enviar_turno`).
fn reclamar_turno(estado: &State<'_, Estado>) -> Result<(), String> {
    let puede = match estado.turno.lock() {
        Ok(t) => !t.activo,
        Err(_) => return Err("no se pudo acceder al turno".into()),
    };
    if !puede {
        return Err("ya hay un turno en curso".into());
    }
    Ok(())
}

/// [079A-1 F5] Exige conversación, lee la meta de borrador y prepara el turno
/// en el servicio común (auxiliar de `enviar_turno`).
/// [109A-5 F1] La meta se lee DURABLE por conversación dentro de
/// `preparar_turno`; el borrador solo entra si esa conversación no tiene meta
/// vigente, así cambiar de panel no arrastra la meta de otro.
async fn preparar_paquete(
    sesion: &Sesion,
    panel_id: &str,
    mensaje: String,
    modo_turno: Option<&str>,
) -> Result<PaqueteTurno, String> {
    /* [069A-7] El turno exige conversación: el front la crea (create-on-write)
     * antes de enviar el primer mensaje. Un panel en borrador no puede enviar
     * (error claro en vez de fallar después en preparar_turno). */
    let conv_id = super::conversaciones::conv_id_de_panel_obligatoria(sesion, panel_id)?;
    let meta = sesion
        .meta
        .lock()
        .map(|g| g.clone())
        .map_err(|_| "sesión bloqueada".to_string())?;
    /* [109A-4 F4] En un turno forzado a solo lectura el texto del comando es
     * la meta del turno (`/meta <texto>`): el usuario pide "trabaja con esta
     * meta, sin escribir". La meta durable de la conversación, si existe,
     * gana (la elige `preparar_turno_con_modo`). */
    let meta_borrador = if modo_turno.is_some() {
        Some(mensaje.clone())
    } else {
        meta
    };
    let preparacion = {
        let comun = sesion
            .comun
            .lock()
            .map_err(|_| "sesión bloqueada".to_string())?
            .clone();
        comun
            .preparar_turno_con_modo(conv_id, mensaje, meta_borrador, modo_turno)
            .await
            .map_err(|e| e.to_string())?
    };
    Ok(PaqueteTurno {
        conv_id,
        turno_id: preparacion.turno_id,
        historial_previo: preparacion.historial,
        mensaje_efectivo: preparacion.mensaje_efectivo,
        runtime: preparacion.runtime,
    })
}

/// [079A-1 F5] Fija el contexto del vault para el turno y limpia el tramo
/// de rewind obsoleto (auxiliar de `enviar_turno`).
fn fijar_contexto_turno(sesion: &Sesion, panel_id: &str, conv_id: Uuid, turno_id: Uuid) {
    /* [039A-3 P3] Fijar el contexto del vault para este turno (conversación +
     * turno): las escrituras del harness durante `ejecutar_turno` se
     * atribuyen a este tramo. El hook del núcleo no recibe el turno; el
     * desktop lo deja aquí ANTES de cada turno. */
    sesion.vault.fijar_contexto(vault::ContextoTurnoVault {
        conversacion_id: Some(conv_id),
        turno_id: Some(turno_id),
        tool_name: None,
    });
    /* [039A-3 P3] Al enviar un turno nuevo tras un "volver a punto" el tramo
     * pendiente de restaurar deja de ser el último (el usuario siguió
     * hablando en vez de restaurar): se limpia para no ofrecer una
     * restauración obsoleta. */
    if let Ok(mut g) = sesion.paneles.lock() {
        if let Some(d) = g.get_mut(panel_id) {
            d.tramo_rewind = None;
            d.turno_id = Some(turno_id);
        }
    }
}

/// [079A-1 F5] Bucle de reenvío de eventos del núcleo a la ventana; termina
/// con `Done` y acumula el `Usage` real del turno (auxiliar de `enviar_turno`).
async fn reenviar_eventos(
    ventana: tauri::Window,
    mut rx: tokio::sync::mpsc::Receiver<AgenteEvento>,
    uso: Arc<Mutex<UsoAcumulado>>,
) {
    while let Some(ev) = rx.recv().await {
        let es_done = matches!(ev, AgenteEvento::Done { .. });
        acumular_uso(&uso, &ev);
        let _ = ventana.emit("agente-evento", &ev);
        if es_done {
            break;
        }
    }
}

/// [079A-1 F5] Suma un `Usage` parcial al acumulado del turno (un turno con
/// N tools emite N parciales; provider/modelo = los del último).
fn acumular_uso(uso: &Mutex<UsoAcumulado>, ev: &AgenteEvento) {
    if let AgenteEvento::Usage {
        tokens_prompt,
        tokens_complecion,
        provider,
        modelo,
        ..
    } = ev
    {
        if let Ok(mut u) = uso.lock() {
            u.tokens_prompt = u.tokens_prompt.saturating_add(*tokens_prompt);
            u.tokens_complecion = u.tokens_complecion.saturating_add(*tokens_complecion);
            if let Some(p) = provider {
                u.provider = Some(p.clone());
            }
            if let Some(m) = modelo {
                u.modelo = Some(m.clone());
            }
        }
    }
}

/// [079A-1 F5] Cierra el turno: persiste el uso real (best-effort) y emite
/// `turno-fin` ok/error (auxiliar de `enviar_turno`).
fn cerrar_turno(
    w: &tauri::Window,
    sesion: &Sesion,
    turno_id: Uuid,
    resultado: Result<(), glory_harness_core::error::Error>,
    uso_accum: &Mutex<UsoAcumulado>,
) {
    match resultado {
        Ok(()) => {
            // [039A-3 P1] Persistir el uso/modelo REAL del turno (los
            // campos que el runtime guardó son 0 / solicitado). El UPDATE
            // es best-effort: si falla, el pie de turno no se bloquea.
            if let Ok(uso) = uso_accum.lock() {
                if uso.tokens_prompt > 0 || uso.tokens_complecion > 0 || uso.provider.is_some() {
                    let _ = sesion.persistencia.turno_actualizar_uso(
                        turno_id,
                        uso.tokens_prompt,
                        uso.tokens_complecion,
                        uso.provider.as_deref(),
                        uso.modelo.as_deref(),
                    );
                }
            }
            let _ = w.emit("turno-fin", serde_json::json!({"ok": true}));
        }
        Err(e) => {
            let _ = w.emit(
                "agente-evento",
                &AgenteEvento::Error {
                    mensaje: e.to_string(),
                    retryable: true,
                },
            );
            let _ = w.emit(
                "turno-fin",
                serde_json::json!({"ok": false, "error": e.to_string()}),
            );
        }
    }
}

/// [079A-1 F5] Libera el flag de turno, desvincula el panel y limpia el
/// contexto del vault (auxiliar de `enviar_turno`).
fn terminar_turno(w: &tauri::Window, sesion: &Sesion, panel_id: &str) {
    if let Some(e) = w.app_handle().try_state::<Estado>() {
        marcar_turno_terminado(&e);
    }
    if let Ok(mut g) = sesion.paneles.lock() {
        if let Some(d) = g.get_mut(panel_id) {
            d.turno_id = None;
        }
    }
    /* [039A-3 P3] Limpiar el contexto del vault: el siguiente turno
     * vuelve a fijarlo; sin limpieza, una escritura fuera de turno
     * (p. ej. un setup posterior) se atribuiría al último turno. */
    sesion
        .vault
        .fijar_contexto(vault::ContextoTurnoVault::default());
}

/// [079A-1 F5] Registra el handle del turno lanzado (auxiliar de `enviar_turno`).
fn registrar_turno(
    estado: &State<'_, Estado>,
    handle: tauri::async_runtime::JoinHandle<()>,
) -> Result<(), String> {
    match estado.turno.lock() {
        Ok(mut t) => {
            t.handle = Some(handle);
            t.activo = true;
            Ok(())
        }
        Err(_) => Err("no se pudo registrar el turno".into()),
    }
}

/// Aborta el turno en curso (el runtime se detiene al cerrar el canal) y lo
/// marca `cancelado` en la BD para que no quede como pendiente eternamente.
/// [039A-3 P5] `panel_id` opcional: el turno a cancelar es el de ESE panel.
/// Como M1 no permite 2 turnos, en la práctica coincide con el único turno
/// activo; el id se lee del panel para marcarlo en BD.
#[tauri::command]
pub(crate) fn cancelar_turno(
    estado: State<'_, Estado>,
    window: tauri::Window,
    panel_id: Option<String>,
) -> Result<(), String> {
    let sesion = sesion_actual(&estado)?;
    let panel_id = normalizar_panel(panel_id);
    match estado.turno.lock() {
        Ok(mut t) => {
            if let Some(h) = t.handle.take() {
                h.abort();
            }
            t.activo = false;
        }
        Err(_) => return Err("no se pudo acceder al turno".into()),
    }
    let turno_id = sesion
        .paneles
        .lock()
        .map(|mut g| g.get_mut(&panel_id).and_then(|d| d.turno_id.take()))
        .map_err(|_| "sesión bloqueada".to_string())?;
    tauri::async_runtime::spawn(async move {
        if let Some(id) = turno_id {
            let comun = match sesion.comun.lock() {
                Ok(guard) => guard.clone(),
                Err(_) => {
                    let _ = window.emit(
                        "turno-fin",
                        serde_json::json!({"ok": false, "error": "sesión bloqueada al cancelar"}),
                    );
                    return;
                }
            };
            let _ = comun.cancelar_turno(id).await;
        }
        let _ = window.emit(
            "turno-fin",
            serde_json::json!({"ok": false, "error": "cancelado por el usuario"}),
        );
    });
    Ok(())
}
