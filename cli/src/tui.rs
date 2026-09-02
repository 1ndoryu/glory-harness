//! TUI enriquecida (Fase 5, opción B sobre el híbrido C): `glory-harness chat
//! --tui`. Paneles de mensajes/estado/entrada con ratatui+crossterm, sobre el
//! MISMO contrato `AgenteEvento` y el mismo `procesar_turno` que el REPL
//! lineal (`chat.rs`): solo cambia la capa de presentación, no el flujo.
//!
//! El bucle de UI es un único hilo: drena los eventos del worker del turno
//! (`try_recv`), lee teclado con `poll(50ms)` y redibuja. El worker (tokio)
//! ejecuta `procesar_turno` y devuelve eventos tipados (`EventoTui`) — así el
//! turno nunca bloquea el repintado y Ctrl+C/Esc salen en cualquier momento.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::{Frame, Terminal};
use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::AgentPersistence;

use crate::chat::{historial_desde_persistencia, procesar_turno};
use crate::persistencia::PersistenciaMemoria;
use crate::run::{construir_harness, OpcionesRun};

/// Eventos del worker del turno hacia la UI. Cada variante es una línea o un
/// cambio de estado que la TUI pinta en su panel.
enum EventoTui {
    /// Eco del texto que el usuario envió (se pinta como línea del usuario).
    Entrada(String),
    /// Línea de estado/aviso (tools, errores, avisos de comandos).
    Estado(String),
    /// Texto final del asistente.
    Respuesta(String),
    /// El worker terminó la sesión (comando `/salir` o canal cerrado).
    Fin,
}

/// Estado visible de la TUI: historial de líneas, entrada en curso, cursor y
/// la línea de estado superior (tool en curso, "pensando…", etc.).
struct UiEstado {
    lineas: Vec<(bool, String)>,
    entrada: String,
    cursor: usize,
    estado: String,
    salir: bool,
}

impl UiEstado {
    fn nuevo() -> Self {
        Self {
            lineas: Vec::new(),
            entrada: String::new(),
            cursor: 0,
            estado: String::new(),
            salir: false,
        }
    }

    fn insertar(&mut self, c: char) {
        self.entrada.insert(self.cursor, c);
        self.cursor += 1;
    }

    fn retroceder(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.entrada.remove(self.cursor);
        }
    }
}

/// Ejecuta `chat --tui`: pantalla completa (modo raw + alternate screen).
/// Devuelve `Err` solo si la terminal no admite la TUI; Ctrl+C/Esc/`/salir`
/// salen con `Ok(())`.
pub async fn tui(opciones: OpcionesRun) -> Result<(), String> {
    let harness = construir_harness(&opciones);

    let raiz = harness
        .workspace
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| "<desconocido>".to_string());
    let cabecera = format!(
        "glory-harness chat — {}/{} · workspace {raiz}",
        harness.config.provider, harness.config.modelo
    );

    let (tx_entrada, rx_entrada) = tokio::sync::mpsc::channel::<String>(16);
    let (tx_eventos, rx_eventos) = tokio::sync::mpsc::channel::<EventoTui>(64);
    spawn_worker(harness.persistencia, harness.runtime, harness.user_id, rx_entrada, tx_eventos.clone());

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
    let _ = crossterm::execute!(terminal.backend_mut(), EnterAlternateScreen);

    let resultado = bucle_ui(&mut terminal, rx_eventos, tx_entrada, &cabecera).await;

    /* Restaurar terminal pase lo que pase. */
    let _ = crossterm::execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = terminal.show_cursor();
    let _ = disable_raw_mode();
    resultado
}

/// Worker del turno: dueño de la conversación (id + historial acumulado).
/// Recibe las líneas del usuario, ejecuta `procesar_turno` y publica eventos
/// a la UI. El historial sale de la persistencia real (misma fuente que el
/// REPL); los errores se muestran en vivo vía `EventoTui::Estado`.
fn spawn_worker(
    persistencia: Arc<PersistenciaMemoria>,
    runtime: Arc<AgentRuntime>,
    user_id: Uuid,
    mut rx_entrada: tokio::sync::mpsc::Receiver<String>,
    tx_eventos: tokio::sync::mpsc::Sender<EventoTui>,
) {
    tokio::spawn(async move {
        let mut conversacion_id = Uuid::new_v4();
        loop {
            let Some(texto) = rx_entrada.recv().await else {
                break;
            };
            let texto = texto.trim().to_string();
            if texto.is_empty() {
                continue;
            }
            if texto.starts_with('/') {
                match texto.as_str() {
                    "/salir" | "/exit" | "/quit" => {
                        let _ = tx_eventos.send(EventoTui::Fin).await;
                        break;
                    }
                    "/nuevo" | "/reset" => {
                        conversacion_id = Uuid::new_v4();
                        let _ = tx_eventos
                            .send(EventoTui::Estado(
                                "conversación nueva (el agente ya no recuerda lo anterior)".into(),
                            ))
                            .await;
                        continue;
                    }
                    "/ayuda" | "/help" | "/?" => {
                        let _ = tx_eventos
                            .send(EventoTui::Estado(
                                "comandos: /salir · /nuevo · /ayuda · Ctrl+C/Esc sale".into(),
                            ))
                            .await;
                        continue;
                    }
                    _ => {
                        let _ = tx_eventos
                            .send(EventoTui::Estado(format!(
                                "comando desconocido: {texto} (usa /ayuda)"
                            )))
                            .await;
                        continue;
                    }
                }
            }

            let _ = tx_eventos.send(EventoTui::Entrada(texto.clone())).await;
            let _ = tx_eventos.send(EventoTui::Estado("pensando…".into())).await;

            let historial = match persistencia.listar_mensajes(conversacion_id).await {
                Ok(mensajes) => historial_desde_persistencia(mensajes),
                Err(e) => {
                    let _ = tx_eventos
                        .send(EventoTui::Estado(format!("no se pudo leer el historial: {e}")))
                        .await;
                    continue;
                }
            };

            let tx_ev = tx_eventos.clone();
            match procesar_turno(
                Arc::clone(&runtime),
                user_id,
                conversacion_id,
                historial,
                texto,
                move |evento| {
                    if let Some(e) = estado_desde_evento(&evento) {
                        let _ = tx_ev.blocking_send(EventoTui::Estado(e));
                    }
                },
            )
            .await
            {
                Ok(r) => {
                    if !r.tools.is_empty() {
                        let _ = tx_eventos
                            .send(EventoTui::Estado(format!("tools: {}", r.tools.join(", "))))
                            .await;
                    }
                    let _ = tx_eventos.send(EventoTui::Respuesta(r.texto)).await;
                }
                Err(err) => {
                    let _ = tx_eventos
                        .send(EventoTui::Estado(format!(
                            "el turno falló: {err} (puedes reintentar)"
                        )))
                        .await;
                }
            }
            let _ = tx_eventos.send(EventoTui::Estado(String::new())).await;
        }
    });
}

/// Traduce un evento del contrato a la línea de estado de la TUI (o `None` si
/// no pinta nada: tokens y tools OK se muestran al final del turno).
fn estado_desde_evento(evento: &AgenteEvento) -> Option<String> {
    match evento {
        AgenteEvento::Token { .. } => None,
        AgenteEvento::ToolStart { tool, .. } => Some(format!("⏱ {tool}")),
        AgenteEvento::RequiereAprobacion { tool, .. } => {
            Some(format!("⚠ {tool} requiere aprobación (modo predeterminado)"))
        }
        AgenteEvento::ToolResult {
            tool,
            ok: false,
            resumen,
            ..
        } => Some(format!("✗ {tool}: {resumen}")),
        AgenteEvento::Error { mensaje, .. } => Some(format!("✗ error: {mensaje}")),
        _ => None,
    }
}

/// Bucle de UI: drena eventos del worker, lee teclado con `poll(50ms)` y
/// redibuja. Sale con `Ok(())` en `/salir` del worker, Esc o Ctrl+C.
async fn bucle_ui(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    mut rx_eventos: tokio::sync::mpsc::Receiver<EventoTui>,
    tx_entrada: tokio::sync::mpsc::Sender<String>,
    cabecera: &str,
) -> Result<(), String> {
    let mut ui = UiEstado::nuevo();
    loop {
        /* 1) Drenar eventos del worker (pueden llegar durante el poll). */
        while let Ok(ev) = rx_eventos.try_recv() {
            match ev {
                EventoTui::Entrada(t) => ui.lineas.push((true, t)),
                EventoTui::Estado(e) => ui.estado = e,
                EventoTui::Respuesta(t) => ui.lineas.push((false, t)),
                EventoTui::Fin => {
                    ui.salir = true;
                    break;
                }
            }
        }
        if ui.salir {
            return Ok(());
        }

        /* 2) Teclado (con timeout para que el redibujado siga vivo). */
        if event::poll(Duration::from_millis(50)).map_err(|e| format!("error de terminal: {e}"))?
        {
            match event::read().map_err(|e| format!("error de terminal: {e}"))? {
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    match k.code {
                        KeyCode::Enter => {
                            let texto = std::mem::take(&mut ui.entrada);
                            ui.cursor = 0;
                            let _ = tx_entrada.blocking_send(texto);
                        }
                        KeyCode::Backspace => ui.retroceder(),
                        KeyCode::Left => ui.cursor = ui.cursor.saturating_sub(1),
                        KeyCode::Right => ui.cursor = (ui.cursor + 1).min(ui.entrada.len()),
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Char(c)
                            if k.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' =>
                        {
                            return Ok(());
                        }
                        KeyCode::Char(c) => ui.insertar(c),
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        /* 3) Redibujar (cada iteración: barato a ~20 fps). */
        let estado_txt = ui.estado.clone();
        terminal
            .draw(|f| dibujar(f, &ui, cabecera, &estado_txt))
            .map_err(|e| format!("error de dibujo: {e}"))?;
    }
}

/// Pinta los tres paneles: cabecera+estado, conversación (con scroll al final)
/// y entrada con cursor visible.
fn dibujar(f: &mut Frame, ui: &UiEstado, cabecera: &str, estado: &str) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(area);

    let titulo = if estado.is_empty() {
        cabecera.to_string()
    } else {
        format!("{cabecera} · {estado}")
    };
    f.render_widget(Paragraph::new(titulo), chunks[0]);

    let lineas: Vec<Line> = ui
        .lineas
        .iter()
        .map(|(usuario, texto)| {
            if *usuario {
                Line::from(vec![
                    Span::styled(
                        "tú> ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(texto.clone()),
                ])
            } else {
                Line::from(Span::raw(texto.clone()))
            }
        })
        .collect();
    let total_lineas: usize = ui
        .lineas
        .iter()
        .map(|(_, t)| t.lines().count())
        .sum::<usize>()
        .saturating_add(lineas.len());
    let alto = chunks[1].height.saturating_sub(2) as usize;
    let scroll = total_lineas.saturating_sub(alto.max(1)) as u16;
    f.render_widget(
        Paragraph::new(lineas)
            .block(Block::default().borders(Borders::ALL).title("conversación"))
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false }),
        chunks[1],
    );

    let bloque_entrada = Block::default()
        .borders(Borders::ALL)
        .title("entrada (Enter envía · /ayuda · Esc/Ctrl+C sale)");
    let area_entrada = chunks[2].inner(ratatui::layout::Margin {
        horizontal: 1,
        vertical: 0,
    });
    f.render_widget(
        Paragraph::new(ui.entrada.as_str()).block(bloque_entrada),
        chunks[2],
    );
    f.set_cursor_position((
        area_entrada.x + ui.cursor as u16,
        area_entrada.y,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrada_inserta_y_retrocede() {
        let mut ui = UiEstado::nuevo();
        for c in "hola".chars() {
            ui.insertar(c);
        }
        assert_eq!(ui.entrada, "hola");
        assert_eq!(ui.cursor, 4);

        ui.retroceder();
        assert_eq!(ui.entrada, "hol");
        assert_eq!(ui.cursor, 3);

        // Retroceder en el inicio no hace nada (ni pánico ni cursor negativo).
        for _ in 0..10 {
            ui.retroceder();
        }
        assert_eq!(ui.entrada, "");
        assert_eq!(ui.cursor, 0);
    }

    #[test]
    fn cursor_se_mantiene_dentro_de_la_entrada() {
        let mut ui = UiEstado::nuevo();
        ui.insertar('a');
        ui.insertar('b');
        assert_eq!(ui.cursor, 2);

        // Left/Right solo se mueven dentro de [0, len]: los extremos se saturan.
        ui.cursor = ui.cursor.saturating_sub(1);
        ui.cursor = ui.cursor.saturating_sub(1);
        ui.cursor = ui.cursor.saturating_sub(1);
        assert_eq!(ui.cursor, 0);
        ui.cursor = (ui.cursor + 1).min(ui.entrada.len());
        ui.cursor = (ui.cursor + 1).min(ui.entrada.len());
        assert_eq!(ui.cursor, 2);
        ui.cursor = (ui.cursor + 1).min(ui.entrada.len());
        assert_eq!(ui.cursor, 2); // el extremo derecho se satura
    }

    #[test]
    fn insertar_en_medio_respeta_el_cursor() {
        let mut ui = UiEstado::nuevo();
        for c in "ab".chars() {
            ui.insertar(c);
        }
        ui.cursor = 1; // mover al medio
        ui.insertar('X');
        assert_eq!(ui.entrada, "aXb");
        assert_eq!(ui.cursor, 2);
    }
}