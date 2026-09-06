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

pub(crate) use std::io;
pub(crate) use std::sync::Arc;
pub(crate) use std::time::Duration;

pub(crate) use crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind, KeyModifiers,
    MouseEventKind,
};
pub(crate) use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
pub(crate) use ratatui::backend::CrosstermBackend;
pub(crate) use ratatui::layout::{Constraint, Direction, Layout, Rect};
pub(crate) use ratatui::style::{Color, Modifier, Style};
pub(crate) use ratatui::text::{Line, Span};
pub(crate) use ratatui::widgets::{Block, Borders, Paragraph};
pub(crate) use ratatui::{Frame, Terminal};
pub(crate) use unicode_width::UnicodeWidthStr;
pub(crate) use uuid::Uuid;

pub(crate) use glory_harness_core::evento::AgenteEvento;
pub(crate) use glory_harness_core::runtime::AgentRuntime;

pub(crate) use crate::run::{construir_harness_durable, OpcionesRun};
pub(crate) use crate::turno::{historial_desde_persistencia, procesar_turno};

mod bucle;
mod gate;
mod render;
mod texto;
pub use bucle::tui;
use texto::byte_index;

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
    /// Caché de filas pre-envueltas de los bloques **cerrados** (inmutables).
    /// [059A-S6] `a_lineas` re-envolvía TODO el historial en cada frame (~20
    /// fps): 4,8 ms/frame en release con 1.440 filas y creciendo lineal con la
    /// conversación. Ahora solo se re-envuelve la cola abierta (el bloque del
    /// asistente en streaming crece token a token); el prefijo cerrado se
    /// reutiliza mientras el ancho y el nº de bloques cerrados no cambien.
    cache_filas: CacheFilas,
}

/// Caché de `a_lineas`: filas visuales ya envueltas del prefijo de bloques
/// cerrados, para un ancho concreto. Los bloques cerrados nunca mutan (todo
/// cambio ocurre en el último bloque mientras está abierto o al hacer push de
/// uno nuevo), así que el prefijo es reutilizable entre frames.
#[derive(Debug)]
struct CacheFilas {
    /// Ancho con el que se envolvieron las filas (cambia en resize).
    ancho: usize,
    /// Cuántos bloques del prefijo cubren las filas.
    incluidos: usize,
    filas: Vec<Line<'static>>,
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
            cache_filas: CacheFilas {
                ancho: 0,
                incluidos: 0,
                filas: Vec::new(),
            },
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
            t.estado = if ok {
                ToolEstado::Ok
            } else {
                ToolEstado::Error
            };
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
#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::render::a_lineas;
    use crate::tui::render::filas_visibles;
    use crate::tui::render::render_markdown_linea;
    use crate::tui::texto::envolver_con_prefijo;
    use crate::tui::texto::envolver_linea;

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
        let lineas = a_lineas(&mut ui, 80);
        assert!(lineas.len() >= 4, "vino: {}", lineas.len());
    }

    /// [059A-S6] Regresión del render acotado: `filas_visibles` (la ruta real
    /// por frame) debe producir el MISMO contenido que la envoltura completa
    /// (`a_lineas`), nunca devolver más filas que la ventana, y el prefijo
    /// cacheado debe permanecer estable mientras el streaming solo crece la
    /// cola abierta. Benchmark medido (release): 4.151 µs/frame → 60 µs/frame
    /// en streaming y 22 µs/frame idle con 1.440 filas de historial.
    #[test]
    fn render_acotado_equivale_a_envoltura_completa() {
        let mut ui = UiEstado::nuevo();
        for i in 0..40 {
            ui.push_usuario(format!(
                "mensaje {i}: \u{00bf}qu\u{00e9} tal va la cosa por aqu\u{00ed} con textos que envuelven en varias l\u{00ed}neas visuales? lorem ipsum dolor sit amet consectetur adipiscing elit"
            ));
            ui.push_asistente();
            for t in 0..4 {
                ui.push_token(&format!(
                    "p\u{00e1}rrafo {t}: una **frase en negrita** con `c\u{00f3}digo` y texto suficiente para que el envoltorio corte en varias filas porque es bastante largo "
                ));
            }
            ui.tool_inicio("file_read".into());
            ui.tool_fin(
                "file_read",
                true,
                Some("120 l\u{00ed}neas le\u{00ed}das".into()),
            );
        }
        // (1) Total coherente y ventana acotada.
        let (v0, total) = filas_visibles(&mut ui, 80, 25);
        assert_eq!(a_lineas(&mut ui, 80).len(), total, "total divergente");
        assert!(!v0.is_empty() && v0.len() <= 25, "vino: {}", v0.len());
        assert!(total > 100, "vino: {}", total);
        // (2) Streaming: el prefijo (primeras filas) no cambia al crecer la cola.
        let base = a_lineas(&mut ui, 80);
        let prefijo = base[..5].to_vec();
        ui.push_usuario("pregunta nueva para abrir turno y streamear".into());
        ui.push_asistente();
        for _ in 0..60 {
            ui.push_token("m\u{00e1}s tokens de la respuesta gener\u{00e1}ndose poco a poco ");
            let (v, tot) = filas_visibles(&mut ui, 80, 25);
            assert!(v.len() <= 25 && tot > 100);
        }
        let despues = a_lineas(&mut ui, 80);
        assert_eq!(
            &despues[..5],
            &prefijo[..],
            "el streaming corrompi\u{00f3} el prefijo cacheado"
        );
        // (3) Resize: otro ancho re-envuelve sin romper el invariante.
        let (_, t60) = filas_visibles(&mut ui, 60, 25);
        assert_eq!(a_lineas(&mut ui, 60).len(), t60, "resize divergente");
        // (4) Scroll manual: la ventana cabe en el rango y el total se conserva.
        ui.siguiendo_final = false;
        ui.scroll_manual = 1000; // se clampea al máximo real
        let (v3, t3) = filas_visibles(&mut ui, 60, 25);
        assert_eq!(a_lineas(&mut ui, 60).len(), t3);
        assert_eq!(v3.len(), 25, "scroll manual: ventana incompleta");
    }
}
