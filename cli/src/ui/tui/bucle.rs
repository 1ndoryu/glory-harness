//! [059A-15 S2] Split mecánico de `tui.rs`: bucle de UI, worker y relevador de eventos. Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::*;
use super::gate::resolver_gate_aprobaciones;
use super::render::dibujar;
use crate::turno::TurnoResultado;
use crate::ui::exportar::{guardar_export, render_markdown, ItemExport};

/* [318A-17 B3-F5] Helpers de transcripción para `/export`: mantienen el
 * bucle worker corto (el cuerpo de `spawn_worker` ya está al límite). */

/// Anota el mensaje de usuario solo si es nuevo (los reenvíos tras aprobar
/// repiten el mismo texto y no deben duplicar la transcripción).
fn anotar_usuario(transcripcion: &mut Vec<ItemExport>, texto: &str, es_reenvio: bool) {
    if !es_reenvio {
        transcripcion.push(ItemExport::usuario(texto.to_string()));
    }
}

/// Anota la respuesta del asistente con sus tools; se omite solo si no hubo
/// ni texto ni herramientas (p. ej. turnos sin contenido).
fn anotar_asistente(transcripcion: &mut Vec<ItemExport>, respuesta: &TurnoResultado) {
    if !respuesta.texto.is_empty() || !respuesta.herramientas.is_empty() {
        transcripcion.push(ItemExport::asistente(
            respuesta.texto.clone(),
            respuesta.herramientas.clone(),
        ));
    }
}

/// [318A-17 B3-F5] `/export [archivo]` en la TUI: escribe el Markdown a
/// archivo (ruta dada o nombre por defecto `export-<fecha>.md`) y avisa por
/// `EventoTui::Estado`/`Error`. La TUI no vuelca a consola (pantalla raw).
fn exportar_en_tui(
    tx_eventos: &tokio::sync::mpsc::UnboundedSender<EventoTui>,
    transcripcion: &[ItemExport],
    conversacion_id: Uuid,
    ruta: Option<&str>,
) {
    let markdown = render_markdown(conversacion_id, transcripcion, chrono::Utc::now());
    let destino = match ruta {
        Some(ruta) if !ruta.trim().is_empty() => ruta.trim().to_string(),
        _ => crate::ui::exportar::ruta_predeterminada(),
    };
    match guardar_export(&destino, &markdown) {
        Ok(()) => {
            let _ = tx_eventos.send(EventoTui::Estado(format!(
                "conversación exportada a {destino}"
            )));
        }
        Err(e) => {
            let _ = tx_eventos.send(EventoTui::Error(e));
        }
    }
}


/// Ejecuta `chat --tui`: pantalla completa (modo raw + alternate screen).
/// Devuelve `Err` solo si la terminal no admite la TUI; Ctrl+C/Esc/`/salir`
/// salen con `Ok(())`.
pub async fn tui(opciones: OpcionesRun) -> Result<(), String> {
    /* [069A-2] Harness durable como el REPL: los turnos y mensajes de la TUI
     * comparten la BD (ver `session list`); la TUI no crea fila de
     * conversación (límite documentado: `session resume` abre el REPL). */
    let harness = construir_harness_durable(&opciones).await?;
    // [069A-3] Avisos de escritorio solo si el operador los pidió.
    crate::notificar::aplicar_notificacion(&harness.runtime, opciones.notificar);

    /* [Bloque 3, F3] Comandos slash personalizados del workspace: las
     * plantillas se expanden en el worker ANTES de enviar el mensaje al
     * turno; los built-ins mandan (fail-closed). */
    let workspace = harness.workspace.clone();
    let comandos = workspace
        .as_deref()
        .map(|ws| glory_harness_core::skill::descubrir_comandos(&ws.join(".glory").join("comandos")))
        .unwrap_or_default();
    let raiz = workspace
        .as_deref()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    let cabecera = format!(
        "glory-harness · {}/{} · {raiz}",
        harness.config.provider, harness.config.modelo
    );

    let (tx_entrada, rx_entrada) = tokio::sync::mpsc::channel::<String>(16);
    let (tx_eventos, rx_eventos) = tokio::sync::mpsc::unbounded_channel::<EventoTui>();
    spawn_worker(
        harness.persistencia,
        harness.runtime,
        harness.user_id,
        rx_entrada,
        tx_eventos,
        comandos,
        workspace,
    );

    /* Bucle de UI en modo raw. Si la terminal no lo soporta (p. ej. salida
     * redirigida), fallamos pronto con la causa, sin pantallas a medias. */
    enable_raw_mode().map_err(|e| format!("no se pudo activar modo raw: {e}"))?;
    let mut terminal = match Terminal::new(CrosstermBackend::new(io::stdout())) {
        Ok(t) => t,
        Err(e) => {
            let _ = disable_raw_mode();
            return Err(format!("no se pudo iniciar la TUI: {e}"));
        }
    };
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        EnterAlternateScreen,
        EnableMouseCapture
    );

    let resultado = bucle_ui(&mut terminal, rx_eventos, tx_entrada, &cabecera).await;

    /* Restaurar terminal pase lo que pase. */
    let _ = crossterm::execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    let _ = terminal.show_cursor();
    let _ = disable_raw_mode();
    resultado
}

/// Resultado de procesar una línea de comando `/` en el worker de la TUI.
enum ComandoTui {
    /// Terminar la sesión (el worker envía `Fin` y rompe el bucle).
    Salir,
    /// Conversación reseteada (id + transcripción): siguiente línea sin turno.
    NuevaConversacion,
    /// `/export` atendido: siguiente línea sin turno.
    Exportar,
    /// Comando personalizado expandido: sustituye el texto del usuario.
    Enviar(String),
    /// Ayuda mostrada o comando desconocido avisado: siguiente línea sin turno.
    Descartar,
}

/// Procesa los comandos `/` del worker de la TUI (mismo conjunto que el REPL
/// lineal, con la mutación de estado aquí para que el bucle del worker quede
/// como despachador fino). Fuera de turno: no ejecuta ninguna tool.
fn manejar_comando_tui(
    texto: &str,
    tx_eventos: &tokio::sync::mpsc::UnboundedSender<EventoTui>,
    conversacion_id: &mut Uuid,
    transcripcion: &mut Vec<ItemExport>,
    comandos: &[glory_harness_core::skill::ComandoSlash],
    workspace: &Option<std::path::PathBuf>,
) -> ComandoTui {
    match texto {
        "/salir" | "/exit" | "/quit" => ComandoTui::Salir,
        "/nuevo" | "/reset" => {
            *conversacion_id = Uuid::new_v4();
            transcripcion.clear();
            let _ = tx_eventos.send(EventoTui::Estado(
                "conversación nueva (el agente ya no recuerda lo anterior)".into(),
            ));
            ComandoTui::NuevaConversacion
        }
        s if s == "/export" || s.starts_with("/export ") => {
            let ruta = s.strip_prefix("/export ").map(str::trim);
            exportar_en_tui(tx_eventos, transcripcion, *conversacion_id, ruta);
            ComandoTui::Exportar
        }
        "/ayuda" | "/help" | "/?" => {
            let _ = tx_eventos.send(EventoTui::Estado(
                "comandos: /salir · /nuevo · /export [archivo] · /ayuda · ↑↓ historial · PgUp/PgDn o rueda scroll · Esc/Ctrl+C sale".into(),
            ));
            ComandoTui::Descartar
        }
        _ => {
            /* [Bloque 3, F3] Comandos personalizados (`.glory/comandos/`): si
             * coincide una plantilla, el texto expandido fluye como mensaje del
             * usuario (sin el aviso de desconocido). */
            if let Some(expandido) = glory_harness_core::skill::expandir_comando(
                texto,
                comandos,
                workspace.as_deref(),
            ) {
                ComandoTui::Enviar(expandido)
            } else {
                let _ = tx_eventos.send(EventoTui::Estado(format!(
                    "comando desconocido: {texto} (usa /ayuda)"
                )));
                ComandoTui::Descartar
            }
        }
    }
}

/// Worker del turno: dueño de la conversación (id + historial acumulado).
/// Recibe las líneas del usuario, ejecuta `procesar_turno` y publica eventos
/// a la UI. El historial sale de la persistencia real (misma fuente que el
/// REPL); los errores se muestran en vivo vía `EventoTui`.
pub(crate) fn spawn_worker(
    persistencia: Arc<dyn glory_harness_core::AgentPersistence>,
    runtime: Arc<AgentRuntime>,
    user_id: Uuid,
    mut rx_entrada: tokio::sync::mpsc::Receiver<String>,
    tx_eventos: tokio::sync::mpsc::UnboundedSender<EventoTui>,
    comandos: Vec<glory_harness_core::skill::ComandoSlash>,
    workspace: Option<std::path::PathBuf>,
) {
    tokio::spawn(async move {
        let mut conversacion_id = Uuid::new_v4();
        /* [318A-16 F2] Mensaje a reintentar tras resolver aprobaciones con
         * palabras clave (misma semántica que el REPL): el agente re-propone
         * la tool y esta vez ejecuta. El texto libre rompe el gate y se
         * convierte directamente en el siguiente mensaje del usuario. */
        let mut reintento: Option<String> = None;
        /* [318A-17 B3-F5] Transcripción de la sesión para `/export` (misma
         * fuente que el REPL; se resetea con `/nuevo`). */
        let mut transcripcion: Vec<ItemExport> = Vec::new();
        loop {
            let es_reenvio = reintento.is_some();
            let linea = match reintento.take() {
                Some(linea) => linea,
                None => {
                    let Some(linea) = rx_entrada.recv().await else {
                        let _ = tx_eventos.send(EventoTui::Fin);
                        break;
                    };
                    linea
                }
            };
            let mut texto = linea.trim().to_string();
            if texto.is_empty() {
                continue;
            }
            if texto.starts_with('/') {
                match manejar_comando_tui(
                    &texto,
                    &tx_eventos,
                    &mut conversacion_id,
                    &mut transcripcion,
                    &comandos,
                    &workspace,
                ) {
                    ComandoTui::Salir => {
                        let _ = tx_eventos.send(EventoTui::Fin);
                        break;
                    }
                    /* [318A-16 F2] La plantilla expandida sustituye el texto y
                     * fluye como mensaje al turno. */
                    ComandoTui::Enviar(expandido) => texto = expandido,
                    /* NuevaConversacion, Exportar y Descartar ya aplicaron su
                     * estado o mostraron el aviso: siguiente línea sin turno. */
                    ComandoTui::NuevaConversacion
                    | ComandoTui::Exportar
                    | ComandoTui::Descartar => continue,
                }
            }
            anotar_usuario(&mut transcripcion, &texto, es_reenvio);
            let _ = tx_eventos.send(EventoTui::MensajeUsuario(texto.clone()));
            let _ = tx_eventos.send(EventoTui::EmpiezaTurno);

            let historial = match persistencia.listar_mensajes(conversacion_id).await {
                Ok(mensajes) => historial_desde_persistencia(mensajes),
                Err(e) => {
                    let _ = tx_eventos.send(EventoTui::Error(format!(
                        "no se pudo leer el historial: {e}"
                    )));
                    let _ = tx_eventos.send(EventoTui::FinTurno);
                    continue;
                }
            };

            let tx_ev = tx_eventos.clone();
            match procesar_turno(
                Arc::clone(&runtime),
                user_id,
                conversacion_id,
                historial,
                texto.clone(),
                move |evento| relevar_evento(&tx_ev, &evento),
            )
            .await
            {
                Ok(respuesta) => anotar_asistente(&mut transcripcion, &respuesta),
                Err(err) => {
                    let _ = tx_eventos
                        .send(EventoTui::Error(format!("el turno falló: {err}")));
                }
            }
            let _ = tx_eventos.send(EventoTui::FinTurno);

            let todo_resuelto = match resolver_gate_aprobaciones(
                &runtime,
                &mut rx_entrada,
                &tx_eventos,
                &mut reintento,
            )
            .await
            {
                None => return,
                Some(t) => t,
            };
            /* Todas las peticiones se respondieron con palabras clave:
             * reintentar el último mensaje para que el agente ejecute lo
             * aprobado (mismo comportamiento que el REPL). Si hubo texto
             * libre (`reintento` ya seteado) o una cancelación, no se
             * reenvía el mensaje original. */
            if todo_resuelto && reintento.is_none() {
                reintento = Some(texto);
            }
        }
    });
}

/// Publica un evento del contrato hacia la UI desde el hilo del runtime.
/// Canal sin límite: `send` es síncrono y nunca bloquea, así que no puede
/// paniquear con "Cannot block the current thread from within a runtime"
/// (bug real 02-09-2026) y no se pierden tokens por un canal lleno.
pub(crate) fn relevar_evento(tx: &tokio::sync::mpsc::UnboundedSender<EventoTui>, evento: &AgenteEvento) {
    let evt = match evento {
        AgenteEvento::Token { texto } => EventoTui::Token(texto.clone()),
        AgenteEvento::ToolStart { tool, .. } => EventoTui::ToolInicio {
            tool: tool.clone(),
        },
        AgenteEvento::ToolResult {
            tool,
            ok,
            resumen,
            ..
        } => EventoTui::ToolFin {
            tool: tool.clone(),
            ok: *ok,
            resumen: if *ok {
                None
            } else {
                Some(resumen.clone())
            },
        },
        AgenteEvento::SubagenteInicio {
            perfil,
            instruccion,
        } => EventoTui::Estado(format!(
            "[subagente {perfil}] {}",
            instruccion.lines().next().unwrap_or("")
        )),
        AgenteEvento::SubagenteFin { ok, .. } => EventoTui::Estado(format!(
            "[subagente] {}",
            if *ok { "fin" } else { "sin resumen" }
        )),
        AgenteEvento::RequiereAprobacion { tool, .. } => EventoTui::Estado(format!(
            "{tool} requiere aprobación (modo predeterminado)"
        )),
        AgenteEvento::Error { mensaje, .. } => EventoTui::Error(mensaje.clone()),
        _ => return,
    };
    let _ = tx.send(evt);
}

/// [059A-S3] Aplica un evento del worker al estado de la UI y al historial de
/// ↑↓. `Fin` marca `salir` (el drenador corta tras aplicarlo).
fn aplicar_evento_tui(
    ui: &mut UiEstado,
    historial: &mut Vec<String>,
    historial_idx: &mut usize,
    ev: EventoTui,
) {
    match ev {
        EventoTui::MensajeUsuario(t) => {
            ui.push_usuario(t.clone());
            if historial.last() != Some(&t) {
                historial.push(t);
            }
            *historial_idx = historial.len();
        }
        EventoTui::EmpiezaTurno => {
            ui.push_asistente();
            ui.ocupado = true;
            ui.estado = String::new();
        }
        EventoTui::Token(t) => ui.push_token(&t),
        EventoTui::ToolInicio { tool } => ui.tool_inicio(tool),
        EventoTui::ToolFin { tool, ok, resumen } => ui.tool_fin(&tool, ok, resumen),
        EventoTui::Error(e) => ui.estado = format!("✗ {e}"),
        EventoTui::Estado(e) => ui.estado = e,
        EventoTui::FinTurno => {
            ui.ocupado = false;
        }
        EventoTui::Fin => {
            ui.salir = true;
        }
    }
}

/// [059A-S3] Rueda del ratón: scroll manual (3 líneas por paso). Devuelve
/// `false` si el ratón no aportó nada (para no repintar en vano).
fn manejar_raton(ui: &mut UiEstado, m: crossterm::event::MouseEvent) -> bool {
    match m.kind {
        MouseEventKind::ScrollUp => {
            ui.siguiendo_final = false;
            ui.scroll_manual = ui.scroll_manual.saturating_add(3);
            true
        }

        MouseEventKind::ScrollDown => {
            if ui.scroll_manual > 3 {
                ui.scroll_manual -= 3;
            } else {
                ui.scroll_manual = 0;
                ui.siguiendo_final = true;
            }
            true
        }
        _ => false,
    }
}

/// [059A-S3] Maneja una tecla de la TUI. `Esc`/`Ctrl+C` cortan (fin de
/// sesión); `Enter` envía el texto pendiente por `.await` (no `blocking_send`:
/// esto se ejecuta dentro del runtime y bloqueando paniquea — bug real
/// 02-09-2026). Devuelve `false` para salir del bucle.
async fn manejar_tecla(
    ui: &mut UiEstado,
    historial: &mut [String],
    historial_idx: &mut usize,
    tx_entrada: &tokio::sync::mpsc::Sender<String>,
    k: crossterm::event::KeyEvent,
) -> bool {
    match k.code {
        KeyCode::Enter => {
            let texto = std::mem::take(&mut ui.entrada);
            ui.cursor = 0;
            if !texto.trim().is_empty() {
                let _ = tx_entrada.send(texto).await;
            }
        }
        KeyCode::Backspace => ui.retroceder(),
        KeyCode::Left => ui.cursor = ui.cursor.saturating_sub(1),
        KeyCode::Right => {
            ui.cursor = (ui.cursor + 1).min(ui.entrada.chars().count())
        }
        KeyCode::Home => ui.cursor = 0,
        KeyCode::End => ui.cursor = ui.entrada.chars().count(),
        KeyCode::Esc => return false,
        KeyCode::Char(c) if k.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' => {
            return false;
        }
        KeyCode::Up => {
            // Si el historial está vacío, `get` devuelve None y no ocurre
            // nada (el check externo era redundante).
            *historial_idx = historial_idx.saturating_sub(1);
            if let Some(prev) = historial.get(*historial_idx) {
                ui.entrada = prev.clone();
                ui.cursor = ui.entrada.chars().count();
            }
        }
        KeyCode::Down => {
            if *historial_idx + 1 < historial.len() {
                *historial_idx += 1;
                if let Some(sig) = historial.get(*historial_idx) {
                    ui.entrada = sig.clone();
                    ui.cursor = ui.entrada.chars().count();
                }
            } else {
                *historial_idx = historial.len();
                ui.entrada.clear();
                ui.cursor = 0;
            }
        }
        KeyCode::PageUp => {
            ui.siguiendo_final = false;
            ui.scroll_manual = ui.scroll_manual.saturating_add(10);
        }
        KeyCode::PageDown => {
            if ui.scroll_manual > 10 {
                ui.scroll_manual -= 10;
            } else {
                ui.scroll_manual = 0;
                ui.siguiendo_final = true;
            }
        }
        KeyCode::Char(c) => ui.insertar(c),
        _ => {}
    }
    true
}

/// Bucle de UI: drena eventos del worker, lee teclado con `poll(50ms)` y
/// redibuja. Sale con `Ok(())` en `/salir` del worker, Esc o Ctrl+C.
pub(crate) async fn bucle_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut rx_eventos: tokio::sync::mpsc::UnboundedReceiver<EventoTui>,
    tx_entrada: tokio::sync::mpsc::Sender<String>,
    cabecera: &str,
) -> Result<(), String> {
    let mut ui = UiEstado::nuevo();
    let mut historial: Vec<String> = Vec::new();
    let mut historial_idx: usize = 0;
    loop {
        /* 1) Drenar eventos del worker (pueden llegar durante el poll). */
        while let Ok(ev) = rx_eventos.try_recv() {
            aplicar_evento_tui(&mut ui, &mut historial, &mut historial_idx, ev);
            if ui.salir {
                break;
            }
        }
        if ui.salir {
            return Ok(());
        }

        /* 2) Teclado y ratón (con timeout para que el repintado siga vivo). */
        if event::poll(Duration::from_millis(50)).map_err(|e| format!("error de terminal: {e}"))?
        {
            match event::read().map_err(|e| format!("error de terminal: {e}"))? {
                Event::Mouse(m) => {
                    manejar_raton(&mut ui, m);
                }
                Event::Key(k)
                    if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat)
                        && !manejar_tecla(&mut ui, &mut historial, &mut historial_idx, &tx_entrada, k)
                            .await =>
                {
                    return Ok(());
                }
                _ => {}
            }
        }

        /* 3) Redibujar (cada iteración: barato a ~20 fps). */
        let estado_txt = ui.estado.clone();
        terminal
            .draw(|f| dibujar(f, &mut ui, cabecera, &estado_txt))
            .map_err(|e| format!("error de dibujo: {e}"))?;
    }
}
