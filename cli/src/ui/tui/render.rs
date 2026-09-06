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

/// [059A-S6] Asegura que `cache_filas` cubra los bloques cerrados para `ancho`
/// (los bloques cerrados nunca mutan; todo cambio ocurre en el último bloque
/// mientras está abierto o al hacer push de uno nuevo). Devuelve cuántos
/// bloques quedaron en el prefijo cacheado.
pub(crate) fn sincronizar_cache(ui: &mut UiEstado, ancho: usize) -> usize {
    let n = ui.mensajes.len();
    // El prefijo cerrado son todos los bloques salvo el último cuando es un
    // asistente en streaming: ese se re-envuelve entero por frame porque crece
    // token a token; el resto nunca muta una vez cerrado.
    let cerrados = match (ui.ocupado, ui.mensajes.last()) {
        (true, Some(b)) if b.rol == Rol::Asistente => n - 1,
        _ => n,
    };
    if ui.cache_filas.ancho != ancho || ui.cache_filas.incluidos != cerrados {
        let mut filas: Vec<Line<'static>> = Vec::new();
        for msg in &ui.mensajes[..cerrados] {
            filas.extend(filas_de_bloque(msg, ancho, false));
        }
        ui.cache_filas = CacheFilas {
            ancho,
            incluidos: cerrados,
            filas,
        };
    }
    cerrados
}

/// [059A-S6] Filas de la cola abierta (bloques tras el prefijo cacheado): como
/// máximo el asistente en streaming, que crece y se re-envuelve por frame.
pub(crate) fn filas_abiertas(ui: &UiEstado, ancho: usize) -> Vec<Line<'static>> {
    let mut cola: Vec<Line<'static>> = Vec::new();
    let n = ui.mensajes.len();
    let inicio = ui.cache_filas.incluidos;
    for (i, msg) in ui.mensajes[inicio..].iter().enumerate() {
        // El cursor ▍ solo pinta en el último bloque del chat, de asistente,
        // mientras el worker está ocupado (misma regla que antes del fix).
        let es_ultimo = inicio + i + 1 == n;
        cola.extend(filas_de_bloque(msg, ancho, es_ultimo && ui.ocupado));
    }
    cola
}

/// Convierte el historial visible en `Line`s ya pre-envueltas (una por fila
/// visual), listas para renderizar con `Paragraph` SIN wrap (el ancho se
/// respeta en `envolver_*`). Devuelve TODAS las filas (contrato de tests); la
/// ruta de render real usa `filas_visibles`, acotada a la ventana.
#[cfg(test)]
pub(crate) fn a_lineas(ui: &mut UiEstado, ancho: usize) -> Vec<Line<'static>> {
    let _cerrados = sincronizar_cache(ui, ancho);
    let mut salida: Vec<Line<'static>> = Vec::with_capacity(ui.cache_filas.filas.len() + 8);
    salida.extend(ui.cache_filas.filas.iter().cloned());
    salida.extend(filas_abiertas(ui, ancho));
    if salida.is_empty() {
        salida.push(Line::from(Span::styled(
            "escribe un mensaje y pulsa Enter (o /ayuda).",
            Style::default().fg(Color::DarkGray),
        )));
    }
    salida
}

/// Genera las filas visuales pre-envueltas de UN bloque. `cursor` pinta el
/// indicador ▍ en la cabecera del asistente (bloque en streaming).
fn filas_de_bloque(msg: &Bloque, ancho: usize, cursor: bool) -> Vec<Line<'static>> {
    let mut filas: Vec<Line<'static>> = Vec::new();
    match msg.rol {
        Rol::Usuario => {
            filas.push(Line::from(vec![
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
                    filas.push(Line::from(Span::styled(
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
            if cursor {
                cabecera.push(Span::styled(
                    "▍",
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ));
            }
            filas.push(Line::from(cabecera));
            for linea_logica in msg.cuerpo.lines() {
                for fila in envolver_linea(linea_logica, ancho) {
                    filas.push(render_markdown_linea(&fila));
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
                for (j, fila) in envolver_con_prefijo(&texto, ancho, "  ").iter().enumerate() {
                    if j == 0 {
                        filas.push(Line::from(vec![
                            Span::raw("  "),
                            icono.clone(),
                            Span::raw(" "),
                            Span::styled(fila.trim_start().to_string(), Style::default().fg(color)),
                        ]));
                    } else {
                        filas.push(Line::from(Span::styled(
                            fila.to_string(),
                            Style::default().fg(color),
                        )));
                    }
                }
            }
        }
    }
    filas
}

/// Pinta el layout completo estilo opencode-ligero: cabecera, mensajes con
/// scroll (manual o al final), prompt inferior con borde y barra de estado.
/// [059A-S3] Fila superior: "gh" + cabecera (y el indicador ▍ mientras el
/// worker está ocupado).
fn pintar_cabecera(f: &mut Frame, area: Rect, cabecera: &str, ocupado: bool) {
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
    if ocupado {
        spans.push(Span::styled(
            "  ▍",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        ));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// [059A-S6] Devuelve SOLO las filas visibles `[scroll, scroll+alto)` del
/// historial pre-envuelto, clonando como máximo `alto` filas (la ventana) en
/// vez de todo el historial en cada frame, y el total de filas para clampear
/// el scroll. La caché de bloques cerrados (`sincronizar_cache`) evita además
/// re-envolver todo el historial: antes del fix cada frame costaba O(historial)
/// tanto en envoltura como en clonado (ver plan S6, ítem TUI).
pub(crate) fn filas_visibles(
    ui: &mut UiEstado,
    ancho: usize,
    alto: u16,
) -> (Vec<Line<'static>>, usize) {
    let _cerrados = sincronizar_cache(ui, ancho);
    let cola = filas_abiertas(ui, ancho);
    let cache_len = ui.cache_filas.filas.len();
    let total = cache_len + cola.len();
    if total == 0 {
        return (Vec::new(), 0);
    }
    let alto = alto as usize;
    let max_scroll = total.saturating_sub(alto);
    let scroll = if ui.siguiendo_final {
        max_scroll
    } else {
        ui.scroll_manual = ui.scroll_manual.min(max_scroll as u16);
        ui.scroll_manual as usize
    };
    let hasta = (scroll + alto).min(total);
    let mut vis: Vec<Line<'static>> = Vec::with_capacity(hasta - scroll);
    for idx in scroll..hasta {
        let fila = if idx < cache_len {
            &ui.cache_filas.filas[idx]
        } else {
            &cola[idx - cache_len]
        };
        vis.push(fila.clone());
    }
    (vis, total)
}

/// [059A-S3] Bloque de mensajes: pre-envuelve a `ancho − 2` y pinta la ventana
/// visible (final automático o manual). Mutar `scroll_manual` para clampearlo
/// al rango real es intencional.
fn pintar_mensajes(f: &mut Frame, area: Rect, ui: &mut UiEstado) {
    // El ancho de texto = ancho del área menos el borde (2 columnas).
    let ancho_texto = area.width.saturating_sub(2).max(1) as usize;
    let alto_interior = area.height.saturating_sub(2);
    let (visibles, total) = filas_visibles(ui, ancho_texto, alto_interior);
    let bloque = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(
            " conversación ",
            Style::default().fg(Color::DarkGray),
        ));
    let filas = if visibles.is_empty() && total == 0 {
        // Sin mensajes todavía: pinta la ayuda de arranque como única fila.
        vec![Line::from(Span::styled(
            "escribe un mensaje y pulsa Enter (o /ayuda).",
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        visibles
    };
    f.render_widget(Paragraph::new(filas).block(bloque), area);
}

/// [059A-S3] Prompt (entrada): borde, prefijo `> `, texto con desplazamiento
/// horizontal y posición del cursor.
fn pintar_prompt(f: &mut Frame, area: Rect, ui: &UiEstado) {
    let bloque_prompt = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if ui.ocupado {
            Color::Yellow
        } else {
            Color::DarkGray
        }));
    f.render_widget(bloque_prompt, area);
    let area_prompt = Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
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
    f.set_cursor_position((
        col_cursor.min(area_entrada.x + area_entrada.width),
        area_prompt.y,
    ));
}

/// [059A-S3] Barra inferior: ayuda por defecto o el estado actual (errores en
/// rojo, lo demás en amarillo).
fn pintar_barra_estado(f: &mut Frame, area: Rect, estado: &str) {
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
    f.render_widget(Paragraph::new(barra), area);
}

/// Redibuja la pantalla completa de la TUI. Fondo uniforme primero (para que el
/// color por defecto de la terminal no se vea en los huecos, en Windows a veces
/// pinta azul); luego cabecera, mensajes, prompt y barra de estado.
pub(crate) fn dibujar(f: &mut Frame, ui: &mut UiEstado, cabecera: &str, estado: &str) {
    let area = f.area();
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

    pintar_cabecera(f, chunks[0], cabecera, ui.ocupado);
    pintar_mensajes(f, chunks[1], ui);
    pintar_prompt(f, chunks[2], ui);
    pintar_barra_estado(f, chunks[3], estado);
}
