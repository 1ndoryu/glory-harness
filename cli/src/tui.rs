//! TUI enriquecida (Fase 5, opción B sobre el híbrido C): `glory-harness chat
//! --tui`. Interfaz tipo opencode-ligero con ratatui+crossterm, sobre el MISMO
//! contrato `AgenteEvento` y el mismo `procesar_turno` que el REPL lineal
//! (`chat.rs`): solo cambia la capa de presentación, no el flujo.
//!
//! Modelo visual (inspirado en opencode/claude-code, simplificado para una
//! sola conversación):
//!   - cabecera (1 fila): `gh` + modelo/workspace + indicador de actividad;
//!   - mensajes: un bloque por turno (cabecera `tú`/`asistente` + cuerpo con
//!     markdown ligero + tools inline dim);
//!   - streaming en vivo: los `Token` se acumulan en el bloque actual y se
//!     repintan a ~20 fps (la v1 solo mostraba "pensando…" y volcaba la
//!     respuesta entera al final);
//!   - barra de estado (1 fila) bajo el prompt con el estado transitorio o la
//!     ayuda de teclas;
//!   - prompt inferior con borde: `> ` + entrada, cursor seguro con acentos
//!     (la v1 usaba índices de bytes → panic UTF-8 al escribir en español) y
//!     auto-desplazamiento horizontal cuando el texto supera el ancho.
//!
//! Scroll: el texto se pre-envuelve en filas visuales exactas (`envolver_*`)
//! antes de pasarlo a ratatui, así el número de filas reales se conoce sin
//! depender de APIs privadas de ratatui 0.29 (`line_count`/`WordWrapper` son
//! `mod` internos). El auto-scroll muestra siempre el final; PgUp/PgDn o la
//! rueda del ratón desplazan manualmente.
//!
//! El bucle de UI es un único hilo: drena los eventos del worker del turno
//! (`try_recv`), lee teclado con `poll(50ms)` y redibuja. El worker (tokio)
//! ejecuta `procesar_turno` y publica eventos tipados (`EventoTui`) por un
//! canal **sin límite** — `send` es síncrono y nunca bloquea ni paniquea con
//! "Cannot block the current thread from within a runtime" (bug real visto
//! 02-09-2026), y los tokens no se pierden si la UI va lenta.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};
use ratatui::{Frame, Terminal};
use unicode_width::UnicodeWidthStr;
use uuid::Uuid;

use glory_harness_core::evento::AgenteEvento;
use glory_harness_core::runtime::AgentRuntime;
use glory_harness_core::AgentPersistence;

use crate::chat::{historial_desde_persistencia, procesar_turno};
use crate::persistencia::PersistenciaMemoria;
use crate::run::{construir_harness, OpcionesRun};

/// Rol de un bloque del historial visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Rol {
    Usuario,
    Asistente,
}

/// Estado de una tool inline en el bloque del asistente.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolEstado {
    EnCurso,
    Ok,
    Error,
}

/// Línea de tool del bloque del asistente (visible inline, estilo opencode).
#[derive(Debug)]
struct ToolLinea {
    tool: String,
    estado: ToolEstado,
    resumen: Option<String>,
}

/// Eventos del worker del turno hacia la UI. Canal sin límite.
enum EventoTui {
    /// Eco del texto que el usuario envió (se pinta como bloque del usuario).
    MensajeUsuario(String),
    /// El asistente empezó a responder: empieza el bloque del asistente.
    EmpiezaTurno,
    /// Fragmento de texto generado por el LLM (streaming en vivo).
    Token(String),
    /// Tool en curso (se muestra inline en el bloque del asistente).
    ToolInicio { tool: String },
    /// Tool terminada (ok o error).
    ToolFin {
        tool: String,
        ok: bool,
        resumen: Option<String>,
    },
    /// Error del turno o del historial.
    Error(String),
    /// Estado/aviso transitorio (barra inferior).
    Estado(String),
    /// El turno terminó (cierra el bloque del asistente y libera el estado).
    FinTurno,
    /// El worker terminó la sesión (comando `/salir` o canal cerrado).
    Fin,
}

/// Un bloque de mensaje (turno de usuario o de asistente) con su rol.
#[derive(Debug)]
struct Bloque {
    rol: Rol,
    /// Texto completo del bloque (los tokens se acumulan aquí durante el
    /// streaming; se parte por líneas lógicas y se envuelve al renderizar).
    cuerpo: String,
    /// Tools ejecutadas dentro del bloque del asistente.
    tools: Vec<ToolLinea>,
}

/// Estado visible de la TUI: bloques de mensaje, entrada, cursor, scroll.
#[derive(Debug)]
struct UiEstado {
    mensajes: Vec<Bloque>,
    /// Entrada actual del usuario (búfer editable de una línea).
    entrada: String,
    /// Cursor de edición en **índice de caracteres** (no de bytes): con
    /// `byte_index` nunca paniqueamos por UTF-8 (bug real 02-09-2026: se usaba
    /// un índice de bytes incrementado por carácter).
    cursor: usize,
    /// Estado/aviso transitorio que se pinta en la barra inferior.
    estado: String,
    /// ¿El asistente está respondiendo ahora mismo? (cabecera + borde prompt).
    ocupado: bool,
    salir: bool,
    /// ¿Seguimos el final del chat? Se desactiva al hacer scroll hacia arriba.
    siguiendo_final: bool,
    /// Desplazamiento manual en filas (válido solo si `!siguiendo_final`).
    scroll_manual: u16,
}

impl UiEstado {
    fn nuevo() -> Self {
        Self {
            mensajes: Vec::new(),
            entrada: String::new(),
            cursor: 0,
            estado: String::new(),
            ocupado: false,
            salir: false,
            siguiendo_final: true,
            scroll_manual: 0,
        }
    }

    /// Inserta un carácter en la posición del cursor (índice de chars).
    fn insertar(&mut self, c: char) {
        let idx = byte_index(&self.entrada, self.cursor);
        self.entrada.insert(idx, c);
        self.cursor += 1;
    }

    /// Borra el carácter anterior al cursor (índice de chars).
    fn retroceder(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            let idx = byte_index(&self.entrada, self.cursor);
            self.entrada.remove(idx);
        }
    }

    /// Empieza el bloque del usuario con el texto enviado.
    fn push_usuario(&mut self, texto: String) {
        self.mensajes.push(Bloque {
            rol: Rol::Usuario,
            cuerpo: texto,
            tools: Vec::new(),
        });
    }

    /// Empieza el bloque del asistente (vacío hasta que llegan tokens/tools).
    fn push_asistente(&mut self) {
        self.mensajes.push(Bloque {
            rol: Rol::Asistente,
            cuerpo: String::new(),
            tools: Vec::new(),
        });
    }

    /// Añade un fragmento de texto al cuerpo del bloque del asistente en curso.
    fn push_token(&mut self, token: &str) {
        if let Some(ultimo) = self.mensajes.last_mut() {
            if ultimo.rol == Rol::Asistente {
                ultimo.cuerpo.push_str(token);
                return;
            }
        }
        // Sin bloque de asistente (caso raro): crea uno.
        self.mensajes.push(Bloque {
            rol: Rol::Asistente,
            cuerpo: token.to_string(),
            tools: Vec::new(),
        });
    }

    /// Marca la tool como en curso (la añade al bloque del asistente actual).
    fn tool_inicio(&mut self, tool: String) {
        // Garantiza un bloque de asistente en curso y toma su referencia sin
        // `.unwrap()` (si acabamos de crearlo, `last_mut` siempre es `Some`).
        if !matches!(self.mensajes.last(), Some(b) if b.rol == Rol::Asistente) {
            self.mensajes.push(Bloque {
                rol: Rol::Asistente,
                cuerpo: String::new(),
                tools: Vec::new(),
            });
        }
        let bloque = match self.mensajes.last_mut() {
            Some(b) if b.rol == Rol::Asistente => b,
            // Caso imposible tras el push anterior; si llegara, no añade tool.
            _ => return,
        };
        let ya_activa = bloque
            .tools
            .iter()
            .any(|t| t.tool == tool && t.estado == ToolEstado::EnCurso);
        if !ya_activa {
            bloque.tools.push(ToolLinea {
                tool,
                estado: ToolEstado::EnCurso,
                resumen: None,
            });
        }
    }

    /// Cierra la última tool en curso con el mismo nombre (ok/error).
    fn tool_fin(&mut self, tool: &str, ok: bool, resumen: Option<String>) {
        let bloque = match self.mensajes.last_mut() {
            Some(b) if b.rol == Rol::Asistente => b,
            _ => return,
        };
        if let Some(t) = bloque
            .tools
            .iter_mut()
            .rev()
            .find(|t| t.tool == tool && t.estado == ToolEstado::EnCurso)
        {
            t.estado = if ok { ToolEstado::Ok } else { ToolEstado::Error };
            t.resumen = resumen;
        } else if !ok {
            // Error sin ToolStart previo: se muestra igualmente.
            bloque.tools.push(ToolLinea {
                tool: tool.to_string(),
                estado: ToolEstado::Error,
                resumen,
            });
        }
    }
}

/// Convierte un índice de caracteres de `s` a su índice de bytes.
fn byte_index(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

/// Envuelve un texto plano en filas de ancho visual ≤ `ancho`, cortando en los
/// espacios (word wrap). Una palabra más ancha que `ancho` se parte por
/// caracteres para no desbordar. Devuelve filas sin espacios finales.
fn envolver_linea(texto: &str, ancho: usize) -> Vec<String> {
    let mut filas: Vec<String> = Vec::new();
    if ancho == 0 || texto.is_empty() {
        return filas;
    }
    let mut fila = String::new();
    let mut ancho_fila = 0usize;
    for token in texto.split_inclusive(' ') {
        // `split_inclusive` deja el espacio al final de cada token salvo el último.
        let (palabra, tiene_espacio) = match token.strip_suffix(' ') {
            Some(p) => (p, true),
            None => (token, false),
        };
        let ancho_palabra = UnicodeWidthStr::width(palabra);
        let ancho_token = ancho_palabra + usize::from(tiene_espacio);

        if ancho_fila + ancho_token <= ancho {
            fila.push_str(palabra);
            if tiene_espacio {
                fila.push(' ');
            }
            ancho_fila += ancho_token;
            continue;
        }

        // No cabe en la fila actual → cerrar la fila y empezar otra.
        if !fila.is_empty() {
            filas.push(fila.trim_end().to_string());
            fila.clear();
            ancho_fila = 0;
        }
        if ancho_palabra > ancho {
            // Palabra más ancha que la línea: partir por caracteres.
            let mut resto = palabra;
            while !resto.is_empty() {
                let mut trozo = String::new();
                let mut w = 0usize;
                for ch in resto.chars() {
                    let cw = UnicodeWidthStr::width(ch.to_string().as_str());
                    if w + cw > ancho {
                        break;
                    }
                    trozo.push(ch);
                    w += cw;
                }
                let tam = trozo.len();
                filas.push(trozo);
                resto = &resto[tam..];
            }
        } else {
            fila.push_str(palabra);
            if tiene_espacio {
                fila.push(' ');
            }
            ancho_fila += ancho_token;
        }
    }
    if !fila.is_empty() {
        filas.push(fila.trim_end().to_string());
    }
    filas
}

/// Envuelve un texto con un prefijo en la primera fila y un colgante del mismo
/// ancho en las siguientes (indentado de párrafo). Útil para el cuerpo del
/// usuario y las tools.
fn envolver_con_prefijo(texto: &str, ancho: usize, prefijo: &str) -> Vec<String> {
    let ancho_prefijo = UnicodeWidthStr::width(prefijo);
    let ancho_util = ancho.saturating_sub(ancho_prefijo).max(1);
    let mut filas = envolver_linea(texto, ancho_util);
    if filas.is_empty() {
        return filas;
    }
    let relleno = " ".repeat(ancho_prefijo);
    for (i, fila) in filas.iter_mut().enumerate() {
        let indent = if i == 0 { prefijo } else { &relleno };
        *fila = format!("{indent}{fila}");
    }
    filas
}

/// Render de una línea del asistente con markdown MUY ligero y seguro:
/// `**negrita**` → negrita, `` `código` `` → amarillo, líneas de bloque de
/// código (```) → cyan. No parsea HTML ni ejecuta nada.
fn render_markdown_linea(l: &str) -> Line<'static> {
    if l.starts_with("```") {
        return Line::from(Span::styled(
            l.to_string(),
            Style::default().fg(Color::Cyan),
        ));
    }

    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut restante = l;
    while !restante.is_empty() {
        if let Some(pos) = restante.find("**") {
            let (antes, despues) = restante.split_at(pos);
            if !antes.is_empty() {
                spans.push(Span::raw(antes.to_string()));
            }
            let despues = &despues[2..];
            match despues.find("**") {
                Some(cierre) => {
                    let (negrita, resto) = despues.split_at(cierre);
                    spans.push(Span::styled(
                        negrita.to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                    restante = &resto[2..];
                }
                None => {
                    spans.push(Span::raw("**".to_string()));
                    restante = despues;
                }
            }
        } else if let Some(pos) = restante.find('`') {
            let (antes, despues) = restante.split_at(pos);
            if !antes.is_empty() {
                spans.push(Span::raw(antes.to_string()));
            }
            let despues = &despues[1..];
            match despues.find('`') {
                Some(cierre) => {
                    let (codigo, resto) = despues.split_at(cierre);
                    spans.push(Span::styled(
                        codigo.to_string(),
                        Style::default().fg(Color::Yellow),
                    ));
                    restante = &resto[1..];
                }
                None => {
                    spans.push(Span::raw("`".to_string()));
                    restante = despues;
                }
            }
        } else {
            spans.push(Span::raw(restante.to_string()));
            break;
        }
    }
    Line::from(spans)
}

/// Convierte el historial visible en `Line`s ya pre-envueltas (una por fila
/// visual), listas para renderizar con `Paragraph` SIN wrap (el ancho se
/// respeta en `envolver_*`). Devuelve también `None`-indicador no: solo filas.
fn a_lineas(ui: &UiEstado, ancho: usize) -> Vec<Line<'static>> {
    let mut salida: Vec<Line<'static>> = Vec::new();
    for (i, msg) in ui.mensajes.iter().enumerate() {
        let es_ultimo = i + 1 == ui.mensajes.len();
        match msg.rol {
            Rol::Usuario => {
                salida.push(Line::from(vec![
                    Span::styled(
                        "tú ",
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled("· ", Style::default().fg(Color::DarkGray)),
                ]));
                for linea_logica in msg.cuerpo.lines() {
                    for fila in envolver_con_prefijo(linea_logica, ancho, "  ") {
                        salida.push(Line::from(Span::styled(
                            fila,
                            Style::default().fg(Color::Gray),
                        )));
                    }
                }
            }
            Rol::Asistente => {
                let mut cabecera = vec![
                    Span::styled(
                        "asistente",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                ];
                if es_ultimo && ui.ocupado {
                    cabecera.push(Span::styled(
                        "▍",
                        Style::default()
                            .fg(Color::Green)
                            .add_modifier(Modifier::BOLD),
                    ));
                }
                salida.push(Line::from(cabecera));
                for linea_logica in msg.cuerpo.lines() {
                    for fila in envolver_linea(linea_logica, ancho) {
                        salida.push(render_markdown_linea(&fila));
                    }
                }
                for t in &msg.tools {
                    let (icono, color) = match t.estado {
                        ToolEstado::EnCurso => (
                            Span::styled("→", Style::default().fg(Color::DarkGray)),
                            Color::DarkGray,
                        ),
                        ToolEstado::Ok => (
                            Span::styled("✓", Style::default().fg(Color::Green)),
                            Color::Green,
                        ),
                        ToolEstado::Error => (
                            Span::styled("✗", Style::default().fg(Color::Red)),
                            Color::Red,
                        ),
                    };
                    let texto = if let Some(r) = &t.resumen {
                        format!("{} {}", t.tool, r)
                    } else {
                        t.tool.clone()
                    };
                    for (j, fila) in
                        envolver_con_prefijo(&texto, ancho, "  ").iter().enumerate()
                    {
                        if j == 0 {
                            salida.push(Line::from(vec![
                                Span::raw("  "),
                                icono.clone(),
                                Span::raw(" "),
                                Span::styled(
                                    fila.trim_start().to_string(),
                                    Style::default().fg(color),
                                ),
                            ]));
                        } else {
                            salida.push(Line::from(Span::styled(
                                fila.to_string(),
                                Style::default().fg(color),
                            )));
                        }
                    }
                }
            }
        }
    }
    if salida.is_empty() {
        salida.push(Line::from(Span::styled(
            "escribe un mensaje y pulsa Enter (o /ayuda).",
            Style::default().fg(Color::DarkGray),
        )));
    }
    salida
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

/// Worker del turno: dueño de la conversación (id + historial acumulado).
/// Recibe las líneas del usuario, ejecuta `procesar_turno` y publica eventos
/// a la UI. El historial sale de la persistencia real (misma fuente que el
/// REPL); los errores se muestran en vivo vía `EventoTui`.
fn spawn_worker(
    persistencia: Arc<PersistenciaMemoria>,
    runtime: Arc<AgentRuntime>,
    user_id: Uuid,
    mut rx_entrada: tokio::sync::mpsc::Receiver<String>,
    tx_eventos: tokio::sync::mpsc::UnboundedSender<EventoTui>,
) {
    tokio::spawn(async move {
        let mut conversacion_id = Uuid::new_v4();
        /* [318A-16 F2] Mensaje a reintentar tras resolver aprobaciones con
         * palabras clave (misma semántica que el REPL): el agente re-propone
         * la tool y esta vez ejecuta. El texto libre rompe el gate y se
         * convierte directamente en el siguiente mensaje del usuario. */
        let mut reintento: Option<String> = None;
        loop {
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
            let texto = linea.trim().to_string();
            if texto.is_empty() {
                continue;
            }
            if texto.starts_with('/') {
                match texto.as_str() {
                    "/salir" | "/exit" | "/quit" => {
                        let _ = tx_eventos.send(EventoTui::Fin);
                        break;
                    }
                    "/nuevo" | "/reset" => {
                        conversacion_id = Uuid::new_v4();
                        let _ = tx_eventos.send(EventoTui::Estado(
                            "conversación nueva (el agente ya no recuerda lo anterior)".into(),
                        ));
                        continue;
                    }
                    "/ayuda" | "/help" | "/?" => {
                        let _ = tx_eventos.send(EventoTui::Estado(
                            "comandos: /salir · /nuevo · /ayuda · ↑↓ historial · PgUp/PgDn o rueda scroll · Esc/Ctrl+C sale".into(),
                        ));
                        continue;
                    }
                    _ => {
                        let _ = tx_eventos.send(EventoTui::Estado(format!(
                            "comando desconocido: {texto} (usa /ayuda)"
                        )));
                        continue;
                    }
                }
            }

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
                Ok(_) => {}
                Err(err) => {
                    let _ = tx_eventos
                        .send(EventoTui::Error(format!("el turno falló: {err}")));
                }
            }
            let _ = tx_eventos.send(EventoTui::FinTurno);

            /* [318A-16 F2] Gate de aprobación (tres vías) entre turnos: si el
             * turno dejó peticiones `ask` pendientes, el worker ofrece
             * Rechazar / Permitir una vez / Permitir siempre por cada una y
             * lee la decisión del canal de entrada (la UI queda viva; el
             * prompt lo indica). "Permitir siempre" pide confirmación antes
             * de persistir la regla de la CLASE (categoría + `**`). */
            let mut todo_resuelto = true;
            'gate: loop {
                let pendientes = runtime.peticiones_aprobacion_pendientes();
                if pendientes.is_empty() {
                    break 'gate;
                }
                for peticion in &pendientes {
                    let _ = tx_eventos.send(EventoTui::Estado(format!(
                        "⚠ {} pide aprobación (clase: {}) — [n] Rechazar · [p] Permitir una vez · [s] Permitir siempre",
                        peticion.tool, peticion.clasificacion
                    )));
                    let Some(linea) = rx_entrada.recv().await else {
                        let _ = tx_eventos.send(EventoTui::Fin);
                        return;
                    };
                    let decision = linea.trim().to_lowercase();
                    let aplicada = match decision.as_str() {
                        "n" | "no" | "rechazar" | "denegar" => {
                            runtime
                                .responder_aprobacion(&peticion.id, glory_harness_core::aprobacion::RespuestaAprobacion::Rechazar)
                                .map(|_| {
                                    let _ = tx_eventos.send(EventoTui::Estado(format!(
                                        "✗ clase '{}' denegada en esta conversación",
                                        peticion.clasificacion
                                    )));
                                })
                                .is_ok()
                        }
                        "p" | "permitir" | "si" | "aprobar" | "ok" => {
                            runtime
                                .responder_aprobacion(&peticion.id, glory_harness_core::aprobacion::RespuestaAprobacion::Aprobar)
                                .map(|_| {
                                    let _ = tx_eventos.send(EventoTui::Estado(
                                        "✓ permitida (solo esta vez)".into(),
                                    ));
                                })
                                .is_ok()
                        }
                        "s" | "siempre" | "always" | "allow" => {
                            /* Confirmación previa (opencode exige Confirm/Cancel
                             * antes de persistir "always"). */
                            let _ = tx_eventos.send(EventoTui::Estado(format!(
                                "¿Permitir SIEMPRE la clase '{}'? [s/n]",
                                peticion.clasificacion
                            )));
                            let Some(conf) = rx_entrada.recv().await else {
                                let _ = tx_eventos.send(EventoTui::Fin);
                                return;
                            };
                            if matches!(
                                conf.trim().to_lowercase().as_str(),
                                "s" | "si" | "siempre" | "y" | "yes" | "confirmar"
                            ) {
                                runtime
                                    .responder_aprobacion(&peticion.id, glory_harness_core::aprobacion::RespuestaAprobacion::Siempre)
                                    .map(|_| {
                                        let _ = tx_eventos.send(EventoTui::Estado(format!(
                                            "✓ permitida siempre: la clase '{}' ya no preguntará",
                                            peticion.clasificacion
                                        )));
                                    })
                                    .is_ok()
                            } else {
                                let _ = tx_eventos.send(EventoTui::Estado(
                                    "(cancelado — la petición sigue pendiente)".into(),
                                ));
                                false
                            }
                        }
                        _ => {
                            /* Texto libre: respuesta del usuario al agente. Se
                             * convierte en el siguiente mensaje sin resolver
                             * la petición estructurada. */
                            reintento = Some(linea);
                            todo_resuelto = false;
                            break 'gate;
                        }
                    };
                    if !aplicada {
                        todo_resuelto = false;
                    }
                }
            }
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
fn relevar_evento(tx: &tokio::sync::mpsc::UnboundedSender<EventoTui>, evento: &AgenteEvento) {
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

/// Bucle de UI: drena eventos del worker, lee teclado con `poll(50ms)` y
/// redibuja. Sale con `Ok(())` en `/salir` del worker, Esc o Ctrl+C.
async fn bucle_ui(
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
            match ev {
                EventoTui::MensajeUsuario(t) => {
                    ui.push_usuario(t.clone());
                    if historial.last() != Some(&t) {
                        historial.push(t);
                    }
                    historial_idx = historial.len();
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
                    break;
                }
            }
        }
        if ui.salir {
            return Ok(());
        }

        /* 2) Teclado y ratón (con timeout para que el repintado siga vivo). */
        if event::poll(Duration::from_millis(50)).map_err(|e| format!("error de terminal: {e}"))?
        {
            match event::read().map_err(|e| format!("error de terminal: {e}"))? {
                Event::Mouse(m) => match m.kind {
                    MouseEventKind::ScrollUp => {
                        ui.siguiendo_final = false;
                        ui.scroll_manual = ui.scroll_manual.saturating_add(3);
                    }
                    MouseEventKind::ScrollDown => {
                        if ui.scroll_manual > 3 {
                            ui.scroll_manual -= 3;
                        } else {
                            ui.scroll_manual = 0;
                            ui.siguiendo_final = true;
                        }
                    }
                    _ => {}
                },
                Event::Key(k) if matches!(k.kind, KeyEventKind::Press | KeyEventKind::Repeat) => {
                    match k.code {
                        KeyCode::Enter => {
                            let texto = std::mem::take(&mut ui.entrada);
                            ui.cursor = 0;
                            if !texto.trim().is_empty() {
                                // `.await` (no `blocking_send`): esto se ejecuta
                                // dentro del runtime y bloqueando paniquea (bug
                                // real 02-09-2026).
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
                        KeyCode::Esc => return Ok(()),
                        KeyCode::Char(c)
                            if k.modifiers.contains(KeyModifiers::CONTROL) && c == 'c' =>
                        {
                            return Ok(());
                        }
                        KeyCode::Up => {
                            // Si el historial está vacío, `get` devuelve None y
                            // no ocurre nada (el check externo era redundante).
                            historial_idx = historial_idx.saturating_sub(1);
                            if let Some(prev) = historial.get(historial_idx) {
                                ui.entrada = prev.clone();
                                ui.cursor = ui.entrada.chars().count();
                            }
                        }
                        KeyCode::Down => {
                            if historial_idx + 1 < historial.len() {
                                historial_idx += 1;
                                if let Some(sig) = historial.get(historial_idx) {
                                    ui.entrada = sig.clone();
                                    ui.cursor = ui.entrada.chars().count();
                                }
                            } else {
                                historial_idx = historial.len();
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

/// Pinta el layout completo estilo opencode-ligero: cabecera, mensajes con
/// scroll (manual o al final), prompt inferior con borde y barra de estado.
fn dibujar(f: &mut Frame, ui: &mut UiEstado, cabecera: &str, estado: &str) {
    let area = f.area();

    // Fondo uniforme para que el color por defecto de la terminal no se vea en
    // los huecos (en Windows el terminal a veces pinta azul).
    f.render_widget(
        Block::default().style(Style::default().bg(Color::Black)),
        area,
    );

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // cabecera
            Constraint::Min(3),    // mensajes
            Constraint::Length(3), // prompt (borde + 1 fila)
            Constraint::Length(1), // barra de estado
        ])
        .split(area);

    /* ---- Cabecera ---- */
    let mut spans = vec![
        Span::styled(
            "gh",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(cabecera.to_string(), Style::default().fg(Color::Gray)),
    ];
    if ui.ocupado {
        spans.push(Span::styled(
            "  ▍",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), chunks[0]);

    /* ---- Mensajes ---- */
    let area_msgs = chunks[1];
    // El ancho de texto = ancho del área menos el borde (2 columnas).
    let ancho_texto = area_msgs.width.saturating_sub(2).max(1) as usize;
    let lineas = a_lineas(ui, ancho_texto);
    let bloque = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " conversación ",
            Style::default().fg(Color::DarkGray),
        ));
    // Nº de filas reales = número de `Line`s pre-envueltas (cada una ocupa una
    // fila exacta porque ya caben en `ancho_texto`). El área interior del bloque
    // es `altura − 2` (borde superior + inferior).
    let total = lineas.len() as u16;
    let para = Paragraph::new(lineas).block(bloque);
    let alto_interior = area_msgs.height.saturating_sub(2);
    let max_scroll = total.saturating_sub(alto_interior);
    let scroll = if ui.siguiendo_final {
        max_scroll
    } else {
        ui.scroll_manual = ui.scroll_manual.min(max_scroll);
        ui.scroll_manual
    };
    f.render_widget(para.scroll((scroll, 0)), area_msgs);

    /* ---- Prompt (entrada) ---- */
    let bloque_prompt = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if ui.ocupado {
            Color::Yellow
        } else {
            Color::DarkGray
        }));
    f.render_widget(bloque_prompt, chunks[2]);
    let area_prompt = Rect {
        x: chunks[2].x + 1,
        y: chunks[2].y + 1,
        width: chunks[2].width.saturating_sub(2),
        height: chunks[2].height.saturating_sub(2),
    };

    // Prefijo `> ` + texto con desplazamiento horizontal si excede el ancho.
    let ancho_entrada = area_prompt.width.saturating_sub(2) as usize;
    let cursor_col = 2 + UnicodeWidthStr::width(
        ui.entrada
            .chars()
            .take(ui.cursor)
            .collect::<String>()
            .as_str(),
    );
    let hscroll = cursor_col.saturating_sub(ancho_entrada) as u16;

    f.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "> ",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))),
        Rect {
            x: area_prompt.x,
            y: area_prompt.y,
            width: 2,
            height: 1,
        },
    );
    let area_entrada = Rect {
        x: area_prompt.x + 2,
        y: area_prompt.y,
        width: ancho_entrada as u16,
        height: area_prompt.height,
    };
    f.render_widget(
        Paragraph::new(ui.entrada.as_str()).scroll((0, hscroll)),
        area_entrada,
    );
    // Cursor: prefijo (2) + (posición del cursor − desplazamiento horizontal).
    let col_cursor = area_entrada.x + 2 + (cursor_col as u16).saturating_sub(hscroll);
    f.set_cursor_position((col_cursor.min(area_entrada.x + area_entrada.width), area_prompt.y));

    /* ---- Barra de estado ---- */
    let barra = if estado.is_empty() {
        Line::from(Span::styled(
            "Enter enviar · /ayuda · ↑↓ historial · PgUp/PgDn o rueda scroll · Esc/Ctrl+C sale",
            Style::default().fg(Color::DarkGray),
        ))
    } else {
        let es_error = estado.starts_with('✗');
        Line::from(Span::styled(
            estado.to_string(),
            Style::default().fg(if es_error { Color::Red } else { Color::Yellow }),
        ))
    };
    f.render_widget(Paragraph::new(barra), chunks[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrada_inserta_y_retrocede_con_utf8() {
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
    fn entrada_utf8_no_paniquea_con_acentos() {
        // Bug real 02-09-2026: insertar/retroceder con índices de bytes
        // paniqueaba con "char boundary" al escribir "café" y editar después.
        let mut ui = UiEstado::nuevo();
        for c in "café".chars() {
            ui.insertar(c);
        }
        assert_eq!(ui.entrada, "café");
        assert_eq!(ui.cursor, 4);

        // Retroceder un acento (é = 2 bytes) no debe paniquear.
        ui.retroceder();
        assert_eq!(ui.entrada, "caf");
        assert_eq!(ui.cursor, 3);

        // Insertar en medio con acentos presentes (la é queda intacta).
        ui.cursor = 1;
        ui.insertar('X');
        assert_eq!(ui.entrada, "cXaf");
        assert_eq!(ui.cursor, 2);
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
        ui.cursor = (ui.cursor + 1).min(ui.entrada.chars().count());
        ui.cursor = (ui.cursor + 1).min(ui.entrada.chars().count());
        assert_eq!(ui.cursor, 2);
        ui.cursor = (ui.cursor + 1).min(ui.entrada.chars().count());
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

    #[test]
    fn byte_index_maneja_utf8() {
        let s = "café";
        // char 3 = 'é' empieza en el byte 3; el final (char 4) es el byte 5.
        assert_eq!(byte_index(s, 3), 3);
        assert_eq!(byte_index(s, 4), 5);
        assert_eq!(byte_index(s, 10), 5); // satura al final sin pánico
    }

    #[test]
    fn envolver_linea_respeta_el_ancho() {
        let filas = envolver_linea("hola mundo ancho", 8);
        for f in &filas {
            assert!(
                UnicodeWidthStr::width(f.as_str()) <= 8,
                "fila {f:?} excede el ancho 8"
            );
        }
        assert_eq!(filas, vec!["hola", "mundo", "ancho"]);
    }

    #[test]
    fn envolver_linea_no_pierde_palabras() {
        let texto = "uno dos tres cuatro cinco seis siete ocho nueve diez";
        let filas = envolver_linea(texto, 12);
        let unido: String = filas.join(" ");
        // Las palabras se conservan (unir filas con espacio debe reconstruir).
        assert!(unido.starts_with("uno"), "vino: {unido}");
        assert!(unido.ends_with("diez"), "vino: {unido}");
    }

    #[test]
    fn envolver_linea_parte_palabras_muy_largas() {
        // Una palabra más ancha que la línea se parte sin desbordar.
        let palabra = "x".repeat(30);
        let filas = envolver_linea(&palabra, 10);
        assert!(filas.len() >= 3);
        for f in &filas {
            assert!(UnicodeWidthStr::width(f.as_str()) <= 10);
        }
        assert_eq!(filas.concat(), palabra);
    }

    #[test]
    fn envolver_con_prefijo_indenta_primera_fila() {
        let filas = envolver_con_prefijo("hola mundo", 12, "» ");
        assert!(filas[0].starts_with("» "), "vino: {:?}", filas);
        // La segunda fila lleva el colgante del mismo ancho que el prefijo.
        if filas.len() > 1 {
            assert!(filas[1].starts_with("  "), "vino: {:?}", filas);
        }
    }

    #[test]
    fn bloques_usuario_y_asistente_se_apilan() {
        let mut ui = UiEstado::nuevo();
        ui.push_usuario("hola".into());
        ui.push_asistente();
        ui.push_token("mundo");
        assert_eq!(ui.mensajes.len(), 2);
        assert_eq!(ui.mensajes[0].rol, Rol::Usuario);
        assert_eq!(ui.mensajes[1].rol, Rol::Asistente);
        assert_eq!(ui.mensajes[1].cuerpo, "mundo");
    }

    #[test]
    fn push_token_acumula_en_el_bloque_actual() {
        let mut ui = UiEstado::nuevo();
        ui.push_asistente();
        ui.push_token("Ho");
        ui.push_token("la");
        assert_eq!(ui.mensajes.last().unwrap().cuerpo, "Hola");
    }

    #[test]
    fn markdown_negrita_y_codigo() {
        let linea = render_markdown_linea("Hola **mundo** con `código`");
        let texto = linea.to_string();
        assert!(texto.contains("mundo"), "esperaba texto, vino: {texto}");
        assert!(texto.contains("código"));
    }

    #[test]
    fn tools_se_marcan_ok_y_error() {
        let mut ui = UiEstado::nuevo();
        ui.push_asistente();
        ui.tool_inicio("file_read".into());
        ui.tool_fin("file_read", true, None);
        let bloque = ui.mensajes.last().unwrap();
        assert_eq!(bloque.tools.len(), 1);
        assert_eq!(bloque.tools[0].estado, ToolEstado::Ok);

        ui.tool_inicio("bash".into());
        ui.tool_fin("bash", false, Some("exit 1".into()));
        let bloque = ui.mensajes.last().unwrap();
        assert_eq!(bloque.tools.len(), 2);
        assert_eq!(bloque.tools[1].estado, ToolEstado::Error);
        assert_eq!(bloque.tools[1].resumen.as_deref(), Some("exit 1"));
    }

    #[test]
    fn a_lineas_produce_una_fila_por_entrada_logica() {
        let mut ui = UiEstado::nuevo();
        ui.push_usuario("hola".into());
        ui.push_asistente();
        ui.push_token("una **frase** con `código` y tools");
        ui.tool_inicio("file_read".into());
        ui.tool_fin("file_read", true, None);
        // Líneas: cabecera usuario + 1 cuerpo; cabecera asistente + cuerpo
        // envuelto + tool. El ancho 80 no corta el texto corto.
        let lineas = a_lineas(&ui, 80);
        assert!(lineas.len() >= 4, "vino: {}", lineas.len());
    }
}
