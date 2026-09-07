//! Skills y comandos slash definidos como markdown (Bloque 3 Fase 3).
//! Referencias: grok `utils/skills.ts`, opencode `src/skill/discovery.ts` +
//! `config/command.ts` (slash = skill, patrón Claude).
//!
//! Una skill es un `.md` con frontmatter YAML mínimo:
//! ```md
//! ---
//! nombre: revisar-diff
//! descripcion: Revisa un diff contra la rama base
//! scope: proyecto
//! ---
//! <instrucciones de la skill>
//! ```
//! Un comando slash es un `.md` con `tipo: comando` y `comando: <nombre>`: la
//! plantilla (cuerpo) se expande con `$ARGUMENTOS` y referencias `@archivo`
//! (el contenido del archivo se embebe; opencode `@file`). Todo es puro y
//! determinista: sin I/O implícita, sin LLM.

use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::sync::Arc;

use crate::error::{Error, Result};
use crate::tool::{AgentTool, AgentToolContext, AgentToolResult};

/// Skill descubierta desde un `.md` con frontmatter.
#[derive(Debug, Clone)]
pub struct Skill {
    pub nombre: String,
    pub descripcion: String,
    /// `proyecto` | `usuario` | `global` (informativo para el índice).
    pub scope: String,
    pub contenido: String,
}

/// Comando slash definido como markdown (`tipo: comando`).
#[derive(Debug, Clone)]
pub struct ComandoSlash {
    pub nombre: String,
    pub descripcion: String,
    pub plantilla: String,
}

/// `true` si el texto tiene frontmatter `---` cerrado en las primeras líneas.
fn separar_frontmatter(texto: &str) -> Option<(&str, &str)> {
    let resto = texto.strip_prefix("---\n")?;
    let fin = resto.find("\n---")?;
    Some((&resto[..fin], &resto[fin + 4..]))
}

/// Lee un campo `clave: valor` (primera ocurrencia) del frontmatter.
fn campo_frontmatter(frontmatter: &str, clave: &str) -> Option<String> {
    frontmatter.lines().find_map(|linea| {
        let (k, v) = linea.split_once(':')?;
        (k.trim() == clave).then(|| v.trim().to_string())
    })
}

/// Parsea una skill desde el texto de un archivo `.md`. `None` si falta el
/// frontmatter o `nombre`.
#[must_use]
pub fn parsear_skill(texto: &str) -> Option<Skill> {
    let (frontmatter, cuerpo) = separar_frontmatter(texto)?;
    let nombre = campo_frontmatter(frontmatter, "nombre")?;
    let contenido = cuerpo.trim().to_string();
    if contenido.is_empty() {
        return None;
    }
    Some(Skill {
        nombre,
        descripcion: campo_frontmatter(frontmatter, "descripcion")
            .unwrap_or_else(|| "(sin descripción)".into()),
        scope: campo_frontmatter(frontmatter, "scope").unwrap_or_else(|| "proyecto".into()),
        contenido,
    })
}

/// Parsea un comando slash desde el texto. `None` si no declara
/// `tipo: comando` + `comando: <nombre>`.
#[must_use]
pub fn parsear_comando(texto: &str) -> Option<ComandoSlash> {
    let (frontmatter, cuerpo) = separar_frontmatter(texto)?;
    if campo_frontmatter(frontmatter, "tipo").as_deref() != Some("comando") {
        return None;
    }
    let nombre = campo_frontmatter(frontmatter, "comando")?;
    let plantilla = cuerpo.trim().to_string();
    if plantilla.is_empty() {
        return None;
    }
    Some(ComandoSlash {
        nombre,
        descripcion: campo_frontmatter(frontmatter, "descripcion")
            .unwrap_or_else(|| "(sin descripción)".into()),
        plantilla,
    })
}

/// Descubre skills en `dir` (archivos `*.md` válidos, orden alfabético).
/// Directorio inexistente → vacío (fail-closed: sin skills no hay tool).
pub fn descubrir_en(dir: &Path) -> Vec<Skill> {
    let mut skills = Vec::new();
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return skills;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Ok(texto) = std::fs::read_to_string(&ruta) {
            if let Some(skill) = parsear_skill(&texto) {
                skills.push(skill);
            }
        }
    }
    skills.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    skills
}

/// Igual que [`descubrir_en`] pero solo archivos con `tipo: comando`.
pub fn descubrir_comandos(dir: &Path) -> Vec<ComandoSlash> {
    let mut comandos = Vec::new();
    let Ok(entradas) = std::fs::read_dir(dir) else {
        return comandos;
    };
    for entrada in entradas.flatten() {
        let ruta = entrada.path();
        if ruta.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Ok(texto) = std::fs::read_to_string(&ruta) {
            if let Some(comando) = parsear_comando(&texto) {
                comandos.push(comando);
            }
        }
    }
    comandos.sort_by(|a, b| a.nombre.cmp(&b.nombre));
    comandos
}

/// Índice acotado de skills para la ranura [REGLAS]/contexto: una línea por
/// skill (nombre — descripción), nunca el contenido completo.
#[must_use]
pub fn indice(skills: &[Skill]) -> String {
    if skills.is_empty() {
        return String::new();
    }
    let mut lineas = vec![
        "Skills disponibles (usa la tool `skill` con el nombre para cargar la instrucción):"
            .to_string(),
    ];
    for skill in skills {
        lineas.push(format!(
            "- {} — {} (scope: {})",
            skill.nombre, skill.descripcion, skill.scope
        ));
    }
    lineas.join("\n")
}

/// Resuelve referencias `@archivo` (contenido embebido, acotado a 4 000
/// caracteres) y `$ARGUMENTOS` en una plantilla de comando. `@archivo` se
/// interpreta relativo a `workspace`. Puro salvo la lectura de archivos.
fn expandir_plantilla(plantilla: &str, argumentos: &str, workspace: Option<&Path>) -> String {
    let resultado = plantilla.replace("$ARGUMENTOS", argumentos);
    // Reemplaza cada `@ruta` por el contenido del archivo (una sola pasada).
    let mut con_archivos = String::new();
    let mut resto = resultado.as_str();
    while let Some(pos) = resto.find('@') {
        con_archivos.push_str(&resto[..pos]);
        resto = &resto[pos + 1..];
        let fin = resto
            .find(|c: char| c.is_whitespace() || c == ')' || c == ']' || c == '}')
            .unwrap_or(resto.len());
        let referencia = &resto[..fin];
        if referencia.is_empty() || !referencia.contains('.') {
            con_archivos.push('@');
            continue;
        }
        let contenido = workspace
            .map(|ws| ws.join(referencia))
            .filter(|p| p.is_file())
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|t| t.chars().take(4_000).collect::<String>());
        match contenido {
            Some(texto) => {
                con_archivos.push_str(&format!(
                    "\n[contenido de {referencia}]\n{texto}\n[/fin {referencia}]\n"
                ));
            }
            None => con_archivos.push_str(&format!("@{}", referencia)),
        }
        resto = &resto[fin..];
    }
    con_archivos.push_str(resto);
    con_archivos
}

/// Busca `/nombre argumentos` contra los comandos personalizados y devuelve
/// la plantilla expandida. `None` si no coincide ningún comando.
#[must_use]
pub fn expandir_comando(
    texto: &str,
    comandos: &[ComandoSlash],
    workspace: Option<&Path>,
) -> Option<String> {
    let (nombre, argumentos) = texto.split_once(' ').unwrap_or((texto, ""));
    let nombre = nombre.strip_prefix('/')?;
    let comando = comandos.iter().find(|c| c.nombre == nombre)?;
    Some(expandir_plantilla(
        &comando.plantilla,
        argumentos.trim(),
        workspace,
    ))
}

/// Concatena el índice de skills a las reglas del consumidor (ranura
/// `[REGLAS]`). Pura: `reglas` vacío + skills vacías → `""` (nunca un
/// encabezado huérfano; mismo contrato que [318A-15 F2]).
#[must_use]
pub fn reglas_con_skills(reglas: &str, skills: &[Skill]) -> String {
    let indice = indice(skills);
    if reglas.trim().is_empty() {
        return indice;
    }
    if indice.is_empty() {
        return reglas.to_string();
    }
    format!("{reglas}\n\n{indice}")
}

/// Tool `skill`: carga el contenido de una skill bajo demanda. El índice vive
/// en la ranura [REGLAS]/contexto; esta tool devuelve la instrucción completa
/// (acotada) para que el modelo la aplique en el turno.
pub struct ToolSkill {
    skills: Arc<Vec<Skill>>,
}

impl ToolSkill {
    #[must_use]
    pub fn nuevo(skills: Vec<Skill>) -> Self {
        Self {
            skills: Arc::new(skills),
        }
    }
}

#[async_trait]
impl AgentTool for ToolSkill {
    fn id(&self) -> &str {
        "skill"
    }

    fn descripcion(&self) -> &str {
        "Carga el contenido completo de una skill listada en el índice (usa su nombre exacto)."
    }

    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "nombre": {
                    "type": "string",
                    "description": "Nombre de la skill (ver índice de skills en las reglas).",
                    "enum": self.skills.iter().map(|s| s.nombre.clone()).collect::<Vec<_>>(),
                }
            },
            "required": ["nombre"],
        })
    }

    async fn ejecutar(
        &self,
        _ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let nombre = argumentos
            .get("nombre")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("`nombre` (string) requerido".into()))?;
        let skill = self
            .skills
            .iter()
            .find(|s| s.nombre == nombre)
            .ok_or_else(|| Error::NoEncontrado(format!("skill `{nombre}` no está en el índice")))?;
        Ok(AgentToolResult::ok(
            skill.contenido.clone(),
            format!("skill {} cargada", skill.nombre),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const SKILL_MD: &str = "---\nnombre: revisar-diff\ndescripcion: Revisa el diff pendiente\ntitle: ignorado\n---\nRevisa `git diff` y lista riesgos.\n";

    const COMANDO_MD: &str = "---\ntipo: comando\ncomando: resumir\ndescripcion: Resumen con argumentos\ntitle: x\n---\nResume esto: $ARGUMENTOS\n";

    #[test]
    fn parsea_skill_frontmatter() {
        let skill = parsear_skill(SKILL_MD).expect("skill válida");
        assert_eq!(skill.nombre, "revisar-diff");
        assert_eq!(skill.descripcion, "Revisa el diff pendiente");
        assert_eq!(skill.scope, "proyecto"); // default
        assert_eq!(skill.contenido, "Revisa `git diff` y lista riesgos.");
        assert!(parsear_skill("sin frontmatter").is_none());
        assert!(parsear_skill("---\n---\n").is_none()); // sin nombre ni cuerpo
    }

    #[test]
    fn parsea_comando_solo_con_tipo_comando() {
        let comando = parsear_comando(COMANDO_MD).expect("comando válido");
        assert_eq!(comando.nombre, "resumir");
        assert_eq!(comando.descripcion, "Resumen con argumentos");
        assert_eq!(comando.plantilla, "Resume esto: $ARGUMENTOS");
        // Una skill normal no es un comando.
        assert!(parsear_comando(SKILL_MD).is_none());
    }

    #[test]
    fn descubre_solo_md_validos() {
        let dir = std::env::temp_dir().join(format!("gh-skill-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("a.md"), SKILL_MD).unwrap();
        fs::write(dir.join("b.md"), "sin frontmatter").unwrap();
        fs::write(dir.join("c.md"), COMANDO_MD).unwrap();
        fs::write(dir.join("nota.txt"), "no cuenta").unwrap();
        let skills = descubrir_en(&dir);
        assert_eq!(skills.len(), 1, "solo a.md es skill: {skills:?}");
        assert_eq!(skills[0].nombre, "revisar-diff");
        let comandos = descubrir_comandos(&dir);
        assert_eq!(comandos.len(), 1);
        assert_eq!(comandos[0].nombre, "resumir");
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reglas_con_skills_anexa_indice_sin_encabezado_huérfano() {
        let skill = parsear_skill(SKILL_MD).unwrap();
        let con = reglas_con_skills("Regla base del repo.", std::slice::from_ref(&skill));
        assert!(con.starts_with("Regla base del repo."));
        assert!(con.contains("Skills disponibles"));
        assert!(con.contains("revisar-diff"));
        // Sin reglas ni skills → vacío (la ranura no emite encabezado huérfano).
        assert_eq!(reglas_con_skills("", &[]), "");
        // Skills sin reglas → solo índice.
        let solo = reglas_con_skills("", std::slice::from_ref(&skill));
        assert!(solo.contains("revisar-diff"));
        assert!(!solo.contains("Skills disponibles\n\nSkills disponibles"));
    }

    #[test]
    fn indice_nunca_incluye_contenido() {
        let skill = parsear_skill(SKILL_MD).unwrap();
        let idx = indice(&[skill]);
        assert!(idx.contains("revisar-diff"));
        assert!(!idx.contains("git diff"), "el índice no lleva contenido");
        assert_eq!(indice(&[]), "");
    }

    #[test]
    fn expande_argumentos_y_archivos() {
        let dir = std::env::temp_dir().join(format!("gh-cmd-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("notas.txt"), "contenido del archivo").unwrap();
        let comando = parsear_comando(COMANDO_MD).unwrap();
        // $ARGUMENTOS
        let texto = expandir_comando(
            "/resumir el estado del repo",
            std::slice::from_ref(&comando),
            Some(&dir),
        )
        .expect("coincide /resumir");
        assert!(texto.contains("Resume esto: el estado del repo"));
        // @archivo embebido
        let con_archivo = expandir_plantilla("Mira @notas.txt y responde", "", Some(&dir));
        assert!(con_archivo.contains("[contenido de notas.txt]"));
        assert!(con_archivo.contains("contenido del archivo"));
        // Sin coincidencia → None
        assert!(expandir_comando("/otro", &[comando], Some(&dir)).is_none());
        fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn tool_skill_carga_y_valida() {
        let skill = parsear_skill(SKILL_MD).unwrap();
        let tool = ToolSkill::nuevo(vec![skill.clone()]);
        assert_eq!(tool.id(), "skill");
        // El schema enum expone los nombres disponibles.
        let enum_nombres = tool.schema()["properties"]["nombre"]["enum"].clone();
        assert_eq!(enum_nombres, json!(["revisar-diff"]));
        let persistencia = crate::contrato_tests::PersistenciaMock::default();
        let ctx = crate::tool::AgentToolContext {
            user_id: uuid::Uuid::new_v4(),
            persistencia: &persistencia,
            web_search: None,
            web_fetch: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        };
        let ok = tool
            .ejecutar(&ctx, json!({ "nombre": "revisar-diff" }))
            .await
            .expect("skill existente");
        assert!(ok.ok);
        assert_eq!(ok.contenido, "Revisa `git diff` y lista riesgos.");
        let err = tool
            .ejecutar(&ctx, json!({ "nombre": "otra" }))
            .await
            .expect_err("skill inexistente");
        assert!(err.to_string().contains("no está en el índice"));
        let err2 = tool
            .ejecutar(&ctx, json!({}))
            .await
            .expect_err("falta nombre");
        assert!(err2.to_string().contains("requerido"));
    }
}
