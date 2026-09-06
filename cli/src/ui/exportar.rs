//! [318A-17 B3-F5] Export de la conversación a Markdown (evidencia: claurst
//! `commands/export.rs`, opencode `src/session/`).
//!
//! El export vive en el consumidor (CLI): el REPL y la TUI mantienen la
//! transcripción de la sesión (mensajes de usuario + respuestas del asistente
//! con las herramientas ejecutadas) y este módulo la serializa a Markdown.
//! Rendering puro y determinista (`render_markdown`), más escritura a archivo
//! o consola (`guardar_export`). Sin dependencias del núcleo: solo tipos
//! `chrono`/`uuid`.

use chrono::{DateTime, Utc};
use std::fmt::Write as _;
use uuid::Uuid;

/// Resultado de una herramienta ejecutada en un turno (evento `ToolResult`
/// del contrato `AgenteEvento`). Es lo que el export muestra como "decisiones
/// y eventos" del turno (claurst muestra Input/Output por tool).
#[derive(Debug, Clone)]
pub struct HerramientaEjecutada {
    pub tool: String,
    pub ok: bool,
    pub resumen: String,
    #[allow(dead_code)] // se usa en tests y en el conteo de líneas del diff
    pub diff: Option<String>,
}

/// Una entrada de la transcripción de la sesión (lo que el agente realmente
/// recibió/produjo, con fecha). El REPL y la TUI la alimentan igual.
#[derive(Debug, Clone)]
pub enum ItemExport {
    Usuario { texto: String, fecha: DateTime<Utc> },
    Asistente {
        texto: String,
        fecha: DateTime<Utc>,
        herramientas: Vec<HerramientaEjecutada>,
    },
}

impl ItemExport {
    /// Entrada de mensaje de usuario (fecha = ahora).
    pub fn usuario(texto: String) -> Self {
        Self::Usuario {
            texto,
            fecha: Utc::now(),
        }
    }

    /// Entrada de respuesta del asistente con sus tools (fecha = ahora).
    pub fn asistente(texto: String, herramientas: Vec<HerramientaEjecutada>) -> Self {
        Self::Asistente {
            texto,
            fecha: Utc::now(),
            herramientas,
        }
    }
}

/// Rendering puro: conversación → Markdown. Determinista (recibe la fecha de
/// export como parámetro) para que los tests no dependan del reloj.
pub fn render_markdown(conversacion_id: Uuid, items: &[ItemExport], exportado: DateTime<Utc>) -> String {
    let mut salida = String::new();
    let _ = writeln!(salida, "# Export de conversación — glory-harness");
    let _ = writeln!(salida);
    let _ = writeln!(salida, "- **Conversación:** {conversacion_id}");
    let _ = writeln!(salida, "- **Exportado:** {}", exportado.to_rfc3339());
    let _ = writeln!(salida, "- **Mensajes:** {}", items.len());
    let _ = writeln!(salida);
    let _ = writeln!(salida, "---");
    let _ = writeln!(salida);

    for item in items {
        match item {
            ItemExport::Usuario { texto, fecha } => {
                let _ = writeln!(salida, "## Usuario — {}", fecha.to_rfc3339());
                let _ = writeln!(salida, "{texto}");
            }
            ItemExport::Asistente {
                texto,
                fecha,
                herramientas,
            } => {
                let _ = writeln!(salida, "## Asistente — {}", fecha.to_rfc3339());
                let _ = writeln!(salida, "{texto}");
                if !herramientas.is_empty() {
                    let _ = writeln!(salida);
                    let _ = writeln!(salida, "### Herramientas ejecutadas");
                    for h in herramientas {
                        let estado = if h.ok { "ok" } else { "error" };
                        let resumen = resumen_una_linea(&h.resumen);
                        if let Some(diff) = &h.diff {
                            if !diff.is_empty() {
                                let lineas = diff.lines().count();
                                let _ = writeln!(
                                    salida,
                                    "- `{tool}` → {estado} · {resumen} · diff {lineas} líneas",
                                    tool = h.tool
                                );
                                continue;
                            }
                        }
                        let _ = writeln!(
                            salida,
                            "- `{tool}` → {estado} · {resumen}",
                            tool = h.tool
                        );
                    }
                }
            }
        }
        let _ = writeln!(salida);
        let _ = writeln!(salida, "---");
        let _ = writeln!(salida);
    }
    salida
}

/// Recorta el resumen a la primera línea (máx. 200 caracteres) para que el
/// export no se convierta en un volcado (mismo criterio que claurst, que
/// muestra la primera línea del output de cada tool).
fn resumen_una_linea(resumen: &str) -> String {
    let primera = resumen.lines().next().unwrap_or("").trim();
    if primera.chars().count() > 200 {
        let cortado: String = primera.chars().take(200).collect();
        format!("{cortado}…")
    } else {
        primera.to_string()
    }
}

/// Nombre de archivo por defecto (`export-AAAAMMDD-HHMMSS.md`) cuando el
/// usuario no indica ruta y la superficie no puede volcar a consola (TUI).
pub fn ruta_predeterminada() -> String {
    format!("export-{}.md", Utc::now().format("%Y%m%d-%H%M%S"))
}

/// Escribe el Markdown en `ruta`. Los errores de E/S se devuelven como
/// `Err(String)` (nunca éxito falso).
pub fn guardar_export(ruta: &str, contenido: &str) -> Result<(), String> {
    std::fs::write(ruta, contenido).map_err(|e| format!("no se pudo escribir '{ruta}': {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fecha_fija(seg: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seg, 0).expect("timestamp válido")
    }

    #[test]
    fn render_incluye_cabecera_y_roles() {
        let conv = Uuid::new_v4();
        let items = vec![
            ItemExport::Usuario {
                texto: "explícame el runtime".into(),
                fecha: fecha_fija(1_700_000_000),
            },
            ItemExport::Asistente {
                texto: "El runtime orquesta turnos.".into(),
                fecha: fecha_fija(1_700_000_100),
                herramientas: vec![HerramientaEjecutada {
                    tool: "web_search".into(),
                    ok: true,
                    resumen: "3 resultados".into(),
                    diff: None,
                }],
            },
        ];
        let md = render_markdown(conv, &items, fecha_fija(1_700_000_200));
        assert!(md.contains("# Export de conversación"));
        assert!(md.contains(&format!("**Conversación:** {conv}")));
        assert!(md.contains("**Mensajes:** 2"));
        assert!(md.contains("## Usuario — 2023-11-14T22:13:20+00:00"));
        assert!(md.contains("explícame el runtime"));
        assert!(md.contains("## Asistente — 2023-11-14T22:15:00+00:00"));
        assert!(md.contains("El runtime orquesta turnos."));
        assert!(md.contains("### Herramientas ejecutadas"));
        assert!(md.contains("- `web_search` → ok · 3 resultados"));
    }

    #[test]
    fn render_marca_error_y_diff() {
        let items = vec![ItemExport::Asistente {
            texto: "cambio el archivo".into(),
            fecha: fecha_fija(1_700_000_000),
            herramientas: vec![
                HerramientaEjecutada {
                    tool: "file_patch".into(),
                    ok: true,
                    resumen: "parche aplicado".into(),
                    diff: Some("@@ -1 +1 @@\n-antes\n+despues\n".into()),
                },
                HerramientaEjecutada {
                    tool: "bash".into(),
                    ok: false,
                    resumen: "exit 1: no encontrado".into(),
                    diff: None,
                },
            ],
        }];
        let md = render_markdown(Uuid::new_v4(), &items, fecha_fija(1_700_000_100));
        assert!(md.contains("- `file_patch` → ok · parche aplicado · diff 3 líneas"));
        assert!(md.contains("- `bash` → error · exit 1: no encontrado"));
    }

    #[test]
    fn resumen_se_recorta_a_una_linea() {
        assert_eq!(resumen_una_linea("primera\nsegunda"), "primera");
        let larga = "x".repeat(300);
        assert_eq!(resumen_una_linea(&larga).chars().count(), 201); // 200 + "…"
    }

    #[test]
    fn export_vacio_tiene_cabecera_sin_secciones() {
        let md = render_markdown(Uuid::new_v4(), &[], fecha_fija(1_700_000_000));
        assert!(md.contains("**Mensajes:** 0"));
        assert!(!md.contains("## Usuario"));
        assert!(!md.contains("## Asistente"));
    }

    #[test]
    fn guardar_y_releer_archivo() {
        let ruta = std::env::temp_dir().join(format!("glory-export-test-{}.md", Uuid::new_v4()));
        let contenido = render_markdown(Uuid::new_v4(), &[], fecha_fija(1_700_000_000));
        guardar_export(&ruta.to_string_lossy(), &contenido).expect("escribe");
        let leido = std::fs::read_to_string(&ruta).expect("lee");
        assert_eq!(leido, contenido);
        let _ = std::fs::remove_file(&ruta);
    }
}
