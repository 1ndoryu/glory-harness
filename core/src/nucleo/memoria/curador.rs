//! Curador determinista de la memoria (diseño §3): duplicadas → poda,
//! obsoletas sin uso → archivo, maduras y muy usadas → promoción a skill.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Result;
use crate::ports::{AgentPersistence, MemoriaEntrada, SkillEntrada};

/// Marcador que el motor del cron intercepta para curar nativo (ver
/// [`es_peticion_curador`]): `schedule create --nombre curador-memoria
/// --prompt "[curador-memoria]" --cuando "diario a las 4"`.
pub const MARCADOR_CURADOR: &str = "[curador-memoria]";

/// ¿El prompt pide una pasada del curador (y no un turno de LLM)?
#[must_use]
pub fn es_peticion_curador(prompt: &str) -> bool {
    prompt.trim_start().starts_with(MARCADOR_CURADOR)
}

/// Políticas del curador (defaults hermes-compatibles, diseño §3).
#[derive(Debug, Clone)]
pub struct PoliticaCurador {
    /// Días sin actualizarse ni usarse para archivar (invisible al agente,
    /// conservada para auditar).
    pub stale_days: i64,
    /// Ventana de "uso reciente" que protege del archivo.
    pub uso_reciente_dias: i64,
    /// Usos mínimos + antigüedad mínima para promover a skill.
    pub min_usos_promocion: u32,
    pub antiguedad_promocion_dias: i64,
}

impl Default for PoliticaCurador {
    fn default() -> Self {
        Self {
            stale_days: 30,
            uso_reciente_dias: 30,
            min_usos_promocion: 3,
            antiguedad_promocion_dias: 7,
        }
    }
}

/// Resultado de una pasada del curador (claves afectadas por acción).
#[derive(Debug, Default)]
pub struct ResumenCurador {
    pub archivadas: Vec<String>,
    pub podadas: Vec<String>,
    pub consolidadas: Vec<String>,
    pub promovidas: Vec<String>,
    pub notas: Vec<String>,
}

impl ResumenCurador {
    #[must_use]
    pub fn vacio(&self) -> bool {
        self.archivadas.is_empty()
            && self.podadas.is_empty()
            && self.consolidadas.is_empty()
            && self.promovidas.is_empty()
    }

    /// Texto entregable (el cron lo deja en `tarea_logs` como la `entrega`).
    #[must_use]
    pub fn texto(&self) -> String {
        if self.vacio() && self.notas.is_empty() {
            return "curador: sin cambios (memoria sana)".to_string();
        }
        let mut partes = vec![format!(
            "curador: {} archivadas, {} podadas, {} consolidadas, {} promovidas",
            self.archivadas.len(),
            self.podadas.len(),
            self.consolidadas.len(),
            self.promovidas.len()
        )];
        for lista in [
            ("archivadas", &self.archivadas),
            ("podadas", &self.podadas),
            ("consolidadas", &self.consolidadas),
            ("promovidas", &self.promovidas),
        ] {
            if !lista.1.is_empty() {
                partes.push(format!("{}: {}", lista.0, lista.1.join(", ")));
            }
        }
        for nota in &self.notas {
            partes.push(format!("nota: {nota}"));
        }
        partes.join("; ")
    }
}

/// Normaliza un contenido para detectar duplicados (minúsculas, espacios
/// colapsados).
fn normalizar_contenido(contenido: &str) -> String {
    contenido
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Antigüedad en días (redondeo abajo; futuro → 0).
fn edad_dias(fecha: DateTime<Utc>, ahora: DateTime<Utc>) -> i64 {
    ahora.signed_duration_since(fecha).num_days().max(0)
}

/// Ejecuta una pasada del curador: duplicadas → conserva la más usada y poda
/// el resto; obsoletas sin uso reciente → archiva (marca, no borra);
/// maduras y muy usadas → promueve a skill. Determinista, sin LLM.
/// Los fallos de escritura se propagan (la pasada queda visible como fallo
/// en `tarea_logs`, nunca a medias en silencio).
pub async fn ejecutar_curador(
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    politica: &PoliticaCurador,
) -> Result<ResumenCurador> {
    let ahora = Utc::now();
    let entradas = persistencia.memoria_listar(user_id).await?;
    let mut resumen = ResumenCurador::default();
    let mut vivas: HashMap<String, MemoriaEntrada> = HashMap::new();

    consolidar_duplicadas(persistencia, user_id, &entradas, &mut resumen, &mut vivas).await?;
    archivar_obsoletas(
        persistencia,
        user_id,
        ahora,
        politica,
        &mut resumen,
        &mut vivas,
    )
    .await?;
    promover_maduras(persistencia, user_id, ahora, politica, &mut resumen, &vivas).await?;
    Ok(resumen)
}

/// 1) Duplicadas: mismo contenido normalizado → conserva la de mayor
///    (usos, recencia) y poda las demás.
async fn consolidar_duplicadas(
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    entradas: &[MemoriaEntrada],
    resumen: &mut ResumenCurador,
    vivas: &mut HashMap<String, MemoriaEntrada>,
) -> Result<()> {
    let mut por_contenido: HashMap<String, Vec<MemoriaEntrada>> = HashMap::new();
    for entrada in entradas {
        if entrada.archivada() {
            continue;
        }
        por_contenido
            .entry(normalizar_contenido(&entrada.contenido))
            .or_default()
            .push(entrada.clone());
    }
    for grupo in por_contenido.values() {
        if grupo.len() < 2 {
            continue;
        }
        let mut ordenado = grupo.clone();
        ordenado.sort_by(|a, b| {
            b.usos.cmp(&a.usos).then_with(|| {
                b.ultimo_uso
                    .unwrap_or(b.actualizada_en)
                    .cmp(&a.ultimo_uso.unwrap_or(a.actualizada_en))
            })
        });
        for duplicada in ordenado.iter().skip(1) {
            persistencia
                .memoria_borrar(user_id, &duplicada.clave)
                .await?;
            resumen.consolidadas.push(duplicada.clave.clone());
        }
        vivas.insert(ordenado[0].clave.clone(), ordenado[0].clone());
    }
    for entrada in entradas {
        if !entrada.archivada()
            && !resumen.consolidadas.contains(&entrada.clave)
            && !vivas.contains_key(&entrada.clave)
        {
            vivas.insert(entrada.clave.clone(), entrada.clone());
        }
    }
    Ok(())
}

/// 2) Obsoletas: viejas y sin uso reciente → archiva (marca de origen;
///    `prefetch` las excluye pero se conservan para auditar/revertir).
async fn archivar_obsoletas(
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    ahora: DateTime<Utc>,
    politica: &PoliticaCurador,
    resumen: &mut ResumenCurador,
    vivas: &mut HashMap<String, MemoriaEntrada>,
) -> Result<()> {
    for entrada in vivas.values() {
        let vieja = edad_dias(entrada.actualizada_en, ahora) >= politica.stale_days;
        let sin_uso = entrada
            .ultimo_uso
            .map(|u| edad_dias(u, ahora) >= politica.uso_reciente_dias)
            .unwrap_or(true);
        if vieja && sin_uso {
            let mut archivada = entrada.clone();
            archivada.origen = format!("archivada:{}", ahora.format("%Y-%m-%d"));
            persistencia.memoria_upsert(user_id, &archivada).await?;
            resumen.archivadas.push(entrada.clave.clone());
        }
    }
    for clave in &resumen.archivadas {
        vivas.remove(clave);
    }
    Ok(())
}

/// 3) Promoción: madura + muy usada + sin skill homónima → skill activa.
///    La tienda sin `skills_registrar` deja nota (no rompe la pasada).
async fn promover_maduras(
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    ahora: DateTime<Utc>,
    politica: &PoliticaCurador,
    resumen: &mut ResumenCurador,
    vivas: &HashMap<String, MemoriaEntrada>,
) -> Result<()> {
    let skills = persistencia
        .skills_listar(user_id)
        .await
        .unwrap_or_default();
    let mut sin_registro_avisado = false;
    for entrada in vivas.values() {
        if entrada.usos < politica.min_usos_promocion
            || edad_dias(entrada.actualizada_en, ahora) < politica.antiguedad_promocion_dias
        {
            continue;
        }
        if skills
            .iter()
            .any(|s| s.nombre.eq_ignore_ascii_case(&entrada.clave))
        {
            continue;
        }
        let skill = SkillEntrada {
            id: Uuid::new_v4(),
            nombre: entrada.clave.clone(),
            descripcion: format!("Promovida del recuerdo '{}'", entrada.clave),
            instrucciones: entrada.contenido.clone(),
            activa: true,
        };
        match persistencia.skills_registrar(user_id, &skill).await {
            Ok(()) => {
                let mut marcada = entrada.clone();
                marcada.origen = format!("promovido-a-skill:{}", entrada.clave);
                persistencia.memoria_upsert(user_id, &marcada).await?;
                resumen.promovidas.push(entrada.clave.clone());
            }
            Err(e) => {
                if !sin_registro_avisado {
                    sin_registro_avisado = true;
                    resumen.notas.push(format!("promoción omitida: {e}"));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod pruebas {
    //! [069A-4] Curador: archiva, respeta uso, consolida, promueve y avisa.
    use super::*;
    use crate::memoria::soporte::{entrada_vieja, TiendaPrueba};

    #[tokio::test]
    async fn curador_archiva_obsoleta_sin_uso() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        tienda
            .memoria_upsert(user_id, &entrada_vieja("gusto", "le gusta el té", 40, 0))
            .await
            .expect("siembra");
        let resumen = ejecutar_curador(&tienda, user_id, &PoliticaCurador::default())
            .await
            .expect("curador");
        assert_eq!(resumen.archivadas, vec!["gusto".to_string()]);
        let archivada = tienda.memoria_listar(user_id).await.expect("listar");
        assert!(archivada[0].archivada());
    }

    #[tokio::test]
    async fn curador_respeta_uso_reciente() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        // Vieja pero usada hoy (y con 1 uso: bajo el umbral de promoción):
        // ni se archiva ni se promueve.
        let mut e = entrada_vieja("hábito", "corre por las mañanas", 40, 1);
        e.ultimo_uso = Some(Utc::now());
        tienda.memoria_upsert(user_id, &e).await.expect("siembra");
        let resumen = ejecutar_curador(&tienda, user_id, &PoliticaCurador::default())
            .await
            .expect("curador");
        assert!(resumen.vacio(), "uso reciente protege del archivo");
    }

    #[tokio::test]
    async fn curador_consolida_duplicadas() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let duplicada = entrada_vieja("gusto-b", "Le  gusta el TÉ", 2, 0);
        let original = entrada_vieja("gusto-a", "le gusta el té", 2, 4);
        tienda
            .memoria_upsert(user_id, &duplicada)
            .await
            .expect("siembra");
        tienda
            .memoria_upsert(user_id, &original)
            .await
            .expect("siembra");
        let resumen = ejecutar_curador(&tienda, user_id, &PoliticaCurador::default())
            .await
            .expect("curador");
        assert_eq!(resumen.consolidadas, vec!["gusto-b".to_string()]);
        let resto = tienda.memoria_listar(user_id).await.expect("listar");
        assert_eq!(resto.len(), 1);
        assert_eq!(resto[0].clave, "gusto-a");
    }

    #[tokio::test]
    async fn curador_promueve_madura_y_muy_usada() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        tienda
            .memoria_upsert(user_id, &entrada_vieja("atajo", "usa pnpm siempre", 10, 5))
            .await
            .expect("siembra");
        let resumen = ejecutar_curador(&tienda, user_id, &PoliticaCurador::default())
            .await
            .expect("curador");
        assert_eq!(resumen.promovidas, vec!["atajo".to_string()]);
        let skills = tienda.skills_listar(user_id).await.expect("skills");
        assert_eq!(skills.len(), 1);
        assert!(skills[0].activa);
        assert!(skills[0].instrucciones.contains("pnpm"));
    }

    #[tokio::test]
    async fn curador_no_duplica_skill_existente_y_avisa_sin_registro() {
        let tienda = Arc::new(TiendaPrueba::sin_registro());
        let user_id = Uuid::new_v4();
        tienda.sembrar(
            user_id,
            vec![entrada_vieja("atajo", "usa pnpm siempre", 10, 5)],
        );
        let persistencia: Arc<dyn AgentPersistence> = tienda;
        let resumen = ejecutar_curador(&persistencia, user_id, &PoliticaCurador::default())
            .await
            .expect("curador");
        assert!(resumen.promovidas.is_empty());
        assert_eq!(
            resumen.notas.len(),
            1,
            "la tienda legacy deja nota, no rompe"
        );
        assert!(resumen.texto().contains("promoción omitida"));
    }

    #[test]
    fn resumen_texto_y_marcador() {
        assert!(es_peticion_curador("[curador-memoria]"));
        assert!(es_peticion_curador("  [curador-memoria] extra"));
        assert!(!es_peticion_curador("hola, cura mi memoria"));
        let vacio = ResumenCurador::default();
        assert!(vacio.vacio());
        assert!(vacio.texto().contains("sin cambios"));
    }
}
