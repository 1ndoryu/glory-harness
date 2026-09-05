//! [059A-15 S2] Split mecánico de `tui.rs`: renderizado (render_markdown_linea/a_lineas/dibujar). Movimiento puro — sin
//! cambios de lógica; el contenido se cortó por rangos del archivo original.
//!
use super::*;

use super::texto::*;


/// Render de una línea del asistente con markdown MUY ligero y seguro:
/// `**negrita**` → negrita, `` `código` `` → amarillo, líneas de bloque de
/// código (```) → cyan. No parsea HTML ni ejecuta nada.
pub(crate) fn render_markdown_linea(l: &str) -> Line<'static> {
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
pub(crate) fn a_lineas(ui: &UiEstado, ancho: usize) -> Vec<Line<'static>> {
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


/// Pinta el layout completo estilo opencode-ligero: cabecera, mensajes con
/// scroll (manual o al final), prompt inferior con borde y barra de estado.
pub(crate) fn dibujar(f: &mut Frame, ui: &mut UiEstado, cabecera: &str, estado: &str) {
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

