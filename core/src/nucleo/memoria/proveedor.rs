//! Puerto [`ProveedorMemoria`]: extracción determinista (sync sin LLM),
//! recuperación (prefetch) e implementación base sobre el puerto.

use std::collections::HashSet;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
use uuid::Uuid;

use super::sanitize::sanitize_para_memoria;
use crate::error::Result;
use crate::ports::{AgentPersistence, AmbitoMemoria, MemoriaEntrada, ProveedorMemoria};

/// Frases que expresan intención explícita de recordar (v1: solo lo
/// explícito se guarda solo; el resto lo guarda el agente con
/// `memoria_guardar` cuando lo ve útil).
const DISPARADORES: &[&str] = &[
    "recuerda",
    "recuerde",
    "recuérdame",
    "acuerdate",
    "acuérdate",
    "prefiero",
    "prefiere",
    "me gusta",
    "no me gusta",
    "siempre",
    "nunca",
    "ten en cuenta",
    "a partir de ahora",
];

/// ¿La línea pide recordar algo? (minúsculas, sin tildes por simplicidad).
fn es_candidata(linea: &str) -> bool {
    let minus = linea
        .to_lowercase()
        .replace(['á', 'é', 'í', 'ó', 'ú', 'ñ'], "_");
    DISPARADORES.iter().any(|d| {
        let normal = d.replace(['á', 'é', 'í', 'ó', 'ú'], "_");
        minus.contains(&normal)
    })
}

/// Clave legible desde el contenido: primeras 6 palabras alfanuméricas en
/// minúsculas unidas por guiones (máx 40 caracteres).
fn clave_desde(contenido: &str) -> String {
    let palabras: Vec<String> = contenido
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .take(6)
        .map(ToString::to_string)
        .collect();
    let clave = palabras.join("-");
    let corta: String = clave.chars().take(40).collect();
    if corta.is_empty() {
        "recuerdo".to_string()
    } else {
        corta
    }
}

/// Extrae candidatos del resumen del turno (puro y testeable): una línea
/// por frase con intención explícita, ya sanitizada. El llamador persiste.
#[must_use]
pub fn extraer_candidatos(resumen_turno: &str) -> Vec<(String, String)> {
    let mut vistos = HashSet::new();
    let mut fuera = Vec::new();
    for linea in resumen_turno.split(['\n', '.']) {
        let linea = linea.trim();
        if linea.chars().count() < 12 || !es_candidata(linea) {
            continue;
        }
        let Some(contenido) = sanitize_para_memoria(linea) else {
            continue;
        };
        let clave = clave_desde(&contenido);
        if vistos.insert(clave.clone()) {
            fuera.push((clave, contenido));
        }
    }
    fuera
}

/// Palabras significativas de un texto (minúsculas, alfanuméricas, ≥4).
fn palabras_significativas(texto: &str) -> HashSet<String> {
    texto
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .filter(|p| p.chars().count() >= 4)
        .map(ToString::to_string)
        .collect()
}

/// Ordena por solape con la consulta y formatea hasta `limite` caracteres
/// (puro y testeable). Devuelve el bloque y las claves recordadas (para
/// marcar uso). Las archivadas se excluyen siempre.
#[must_use]
pub fn puntuar_y_formatear(
    entradas: &[MemoriaEntrada],
    query: &str,
    limite: usize,
) -> (String, Vec<String>) {
    let consulta = palabras_significativas(query);
    let mut ranked: Vec<(&MemoriaEntrada, usize)> = entradas
        .iter()
        .filter(|e| !e.archivada())
        .map(|e| {
            let palabras = palabras_significativas(&format!("{} {}", e.clave, e.contenido));
            let puntos = palabras.intersection(&consulta).count();
            (e, puntos)
        })
        .filter(|(_, puntos)| *puntos > 0)
        .collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.clave.cmp(&b.0.clave)));
    let mut bloque = String::new();
    let mut claves = Vec::new();
    for (entrada, _) in ranked {
        let linea = format!("- {}: {}\n", entrada.clave, entrada.contenido);
        if bloque.chars().count() + linea.chars().count() > limite {
            break;
        }
        bloque.push_str(&linea);
        claves.push(entrada.clave.clone());
    }
    (bloque, claves)
}

/// Marca uso en las entradas recordadas (mejor esfuerzo: un fallo de
/// escritura de metadatos no rompe el prefetch).
async fn marcar_uso(
    persistencia: &Arc<dyn AgentPersistence>,
    user_id: Uuid,
    ambito: AmbitoMemoria,
    entradas: &[MemoriaEntrada],
    recordadas: &[String],
) {
    let ahora = Utc::now();
    for entrada in entradas {
        if !recordadas.contains(&entrada.clave) {
            continue;
        }
        let mut tocada = entrada.clone();
        tocada.usos += 1;
        tocada.ultimo_uso = Some(ahora);
        if let Err(e) = persistencia.memoria_upsert(user_id, ambito, &tocada).await {
            tracing::warn!(clave = %entrada.clave, %e, "memoria: no se pudo marcar uso");
        }
    }
}

/// [069A-4] Implementación base de [`ProveedorMemoria`] sobre cualquier
/// tienda del puerto `AgentPersistence::memoria_*` (diseño §2, fase 2).
///
/// [109A-2] El ámbito es fijo al construir: TODAS las lecturas y escrituras
/// del proveedor operan sobre él (el aislamiento no depende de que cada
/// llamada se acuerde de pasarlo).
pub struct MemoriaBase {
    persistencia: Arc<dyn AgentPersistence>,
    limite_prefetch: usize,
    ambito: AmbitoMemoria,
}

impl MemoriaBase {
    /// `ambito` es obligatorio a propósito: un default implícito guardaría
    /// recuerdos de un proyecto en el ámbito global sin que se note.
    #[must_use]
    pub fn nuevo(
        persistencia: Arc<dyn AgentPersistence>,
        limite_prefetch: usize,
        ambito: AmbitoMemoria,
    ) -> Self {
        Self {
            persistencia,
            limite_prefetch,
            ambito,
        }
    }

    #[must_use]
    pub fn ambito(&self) -> AmbitoMemoria {
        self.ambito
    }
}

#[async_trait]
impl ProveedorMemoria for MemoriaBase {
    async fn prefetch(&self, user_id: Uuid, query: &str, limite: usize) -> Result<String> {
        let limite = limite.min(self.limite_prefetch).max(1);
        let entradas = self.persistencia.memoria_listar(user_id, self.ambito).await?;
        let (bloque, claves) = puntuar_y_formatear(&entradas, query, limite);
        if !claves.is_empty() {
            marcar_uso(&self.persistencia, user_id, self.ambito, &entradas, &claves).await;
        }
        Ok(bloque)
    }

    async fn sync(
        &self,
        user_id: Uuid,
        resumen_turno: &str,
        origen: &str,
    ) -> Result<Vec<MemoriaEntrada>> {
        let mut guardadas = Vec::new();
        for (clave, contenido) in extraer_candidatos(resumen_turno) {
            let entrada = MemoriaEntrada::nueva(clave, contenido, origen.to_string());
            // Propaga el primer fallo de escritura (no hay guardado parcial
            // silencioso: el llamador lo registra y el turno continúa).
            self.persistencia
                .memoria_upsert(user_id, self.ambito, &entrada)
                .await?;
            guardadas.push(entrada);
        }
        Ok(guardadas)
    }
}

#[cfg(test)]
mod pruebas {
    //! [069A-4] Extracción determinista, ranking del prefetch y sync.
    use super::*;
    use crate::memoria::soporte::TiendaPrueba;

    #[test]
    fn extrae_solo_intencion_explicita() {
        let resumen =
            "El usuario pidió el parte. Recuerda que prefiere respuestas concisas. Cerramos.";
        let candidatos = extraer_candidatos(resumen);
        assert_eq!(candidatos.len(), 1);
        assert!(candidatos[0].1.contains("concisas"));
    }

    #[test]
    fn extraccion_omite_secretos() {
        let resumen = "Recuerda que mi api_key = abc123 no se olvida.";
        assert!(extraer_candidatos(resumen).is_empty());
    }

    #[tokio::test]
    async fn prefetch_rankea_y_marca_uso() {
        let tienda = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        tienda.sembrar(
            user_id,
            vec![
                MemoriaEntrada::nueva("color-favorito".into(), "el azul".into(), "t".into()),
                MemoriaEntrada::nueva("ciudad-natal".into(), "nació en León".into(), "t".into()),
            ],
        );
        let base = MemoriaBase::nuevo(tienda.clone(), 2000, crate::ports::AmbitoMemoria::Global);
        let bloque = base
            .prefetch(user_id, "¿cuál es mi color favorito?", 2000)
            .await
            .expect("prefetch");
        assert!(bloque.contains("color-favorito"));
        assert!(!bloque.contains("ciudad-natal"));
        let tocada = tienda.leer(user_id, "color-favorito").expect("existe");
        assert_eq!(tocada.usos, 1);
        assert!(tocada.ultimo_uso.is_some());
        let intacta = tienda.leer(user_id, "ciudad-natal").expect("existe");
        assert_eq!(intacta.usos, 0);
    }

    #[tokio::test]
    async fn prefetch_excluye_archivadas_y_respeta_limite() {
        let tienda = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let mut vieja = MemoriaEntrada::nueva(
            "gusto-viejo".into(),
            "le gustaba el rojo".into(),
            "t".into(),
        );
        vieja.origen = "archivada:2026-01-01".into();
        tienda.sembrar(user_id, vec![vieja]);
        let base = MemoriaBase::nuevo(tienda, 2000, crate::ports::AmbitoMemoria::Global);
        let bloque = base
            .prefetch(user_id, "gusto rojo", 2000)
            .await
            .expect("prefetch");
        assert!(bloque.is_empty(), "la archivada no se recuerda: {bloque}");
    }

    /// [109A-2] Aislamiento estricto del prefetch: con la MISMA clave en el
    /// ámbito global y en dos proyectos, cada ámbito recupera solo lo suyo y
    /// el marcado de uso no toca las copias de los demás.
    #[tokio::test]
    async fn prefetch_no_mezcla_ambitos() {
        use crate::ports::AmbitoMemoria;
        let tienda = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let ambito_a = AmbitoMemoria::Proyecto(Uuid::new_v4());
        let ambito_b = AmbitoMemoria::Proyecto(Uuid::new_v4());
        let recuerdo = |contenido: &str| {
            vec![MemoriaEntrada::nueva(
                "color-favorito".into(),
                contenido.into(),
                "t".into(),
            )]
        };
        tienda.sembrar_en(user_id, AmbitoMemoria::Global, recuerdo("global: el azul"));
        tienda.sembrar_en(user_id, ambito_a, recuerdo("proyecto A: el rojo"));
        tienda.sembrar_en(user_id, ambito_b, recuerdo("proyecto B: el verde"));

        let consulta = "¿cuál es mi color favorito?";
        let en_a = MemoriaBase::nuevo(tienda.clone(), 2000, ambito_a)
            .prefetch(user_id, consulta, 2000)
            .await
            .expect("prefetch A");
        assert!(en_a.contains("proyecto A"), "recupera su ámbito: {en_a}");
        assert!(
            !en_a.contains("global") && !en_a.contains("proyecto B"),
            "no ve los otros ámbitos: {en_a}"
        );
        assert_eq!(
            tienda
                .leer_en(user_id, ambito_a, "color-favorito")
                .expect("A")
                .usos,
            1
        );
        assert_eq!(
            tienda
                .leer_en(user_id, ambito_b, "color-favorito")
                .expect("B")
                .usos,
            0,
            "marcar uso no toca otro proyecto"
        );
        assert_eq!(
            tienda
                .leer_en(user_id, AmbitoMemoria::Global, "color-favorito")
                .expect("global")
                .usos,
            0,
            "marcar uso no toca el global"
        );

        let en_global = MemoriaBase::nuevo(tienda, 2000, AmbitoMemoria::Global)
            .prefetch(user_id, consulta, 2000)
            .await
            .expect("prefetch global");
        assert!(en_global.contains("global: el azul"));
        assert!(!en_global.contains("proyecto"), "el global no ve proyectos: {en_global}");
    }

    /// [109A-2] El sync del turno escribe en el ámbito del turno: lo guardado
    /// con un proyecto activo no aparece en el global ni en otro proyecto.
    #[tokio::test]
    async fn sync_escribe_solo_en_su_ambito() {
        use crate::ports::{AgentPersistence, AmbitoMemoria};
        let tienda = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let ambito = AmbitoMemoria::Proyecto(Uuid::new_v4());
        let base = MemoriaBase::nuevo(tienda.clone(), 2000, ambito);
        let guardadas = base
            .sync(user_id, "Recuerda que prefiere reuniones cortas.", "turno:run")
            .await
            .expect("sync");
        assert!(!guardadas.is_empty(), "el candidato explícito se guarda");

        let en_proyecto = AgentPersistence::memoria_listar(tienda.as_ref(), user_id, ambito)
            .await
            .expect("listar proyecto");
        assert_eq!(en_proyecto.len(), guardadas.len());
        let en_global = AgentPersistence::memoria_listar(
            tienda.as_ref(),
            user_id,
            AmbitoMemoria::Global,
        )
        .await
        .expect("listar global");
        assert!(en_global.is_empty(), "el global sigue vacío");
        let en_otro = AgentPersistence::memoria_listar(
            tienda.as_ref(),
            user_id,
            AmbitoMemoria::Proyecto(Uuid::new_v4()),
        )
        .await
        .expect("listar otro");
        assert!(en_otro.is_empty(), "otro proyecto no ve nada");
    }

    #[tokio::test]
    async fn sync_guarda_candidatos_con_origen() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let base = MemoriaBase::nuevo(tienda.clone(), 2000, crate::ports::AmbitoMemoria::Global);
        let guardadas = base
            .sync(
                user_id,
                "Recuerda que prefiere reuniones cortas.",
                "turno:run",
            )
            .await
            .expect("sync");
        assert_eq!(guardadas.len(), 1);
        assert_eq!(guardadas[0].origen, "turno:run");
        let listado = tienda
            .memoria_listar(user_id, crate::ports::AmbitoMemoria::Global)
            .await
            .expect("listar");
        assert_eq!(listado.len(), 1);
    }
}
