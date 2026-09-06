//! Memoria de aprendizaje ([069A-4], diseño
//! `Agente/documentacion/memoria-aprendizaje-diseno-2026-09-06.md`):
//! puerto [`ProveedorMemoria`](crate::ports::ProveedorMemoria) (contrato),
//! implementación base sobre `AgentPersistence::memoria_*`, sanitizado de
//! secretos, curador determinista y tools `memoria_*` del agente.
//!
//! Decisiones frente al diseño:
//! - `sync` recibe además `origen` (qué turno o pasada produjo el recuerdo;
//!   el diseño proponía solo `(user_id, resumen)`).
//! - Sin re-scoring con LLM en v1 (diseño §1): la extracción propone el
//!   turno con reglas deterministas (intención explícita) y el curador poda
//!   por edad/uso/duplicados.
//! - Archivo sin tabla propia: el curador marca `origen = "archivada:<fecha>"`
//!   y `prefetch` excluye archivadas (conservadas para auditar/revertir).
//! - El curador corre nativo vía marcador `[curador-memoria]` interceptado
//!   en el motor del cron (cero coste LLM, entrega normal en `tarea_logs`)
//!   o bajo demanda con el subcomando CLI `memoria curar`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::ports::{AgentPersistence, MemoriaEntrada, ProveedorMemoria};
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};

// ---------------------------------------------------------------------------
// Sanitizado (diseño §4.2: sesgo a no guardar)
// ---------------------------------------------------------------------------

/// Palabras que, seguidas de `:` o `=` (con espacios intermedios), indican
/// credencial (`api_key = ...`, `token: ...`). Sin separador NO匹配: "token"
/// solo cuenta como asignación, nunca como palabra suelta (los turnos hablan
/// de "tokens" del modelo constantemente).
const CLAVES_SECRETO: &[&str] = &[
    "api_key",
    "apikey",
    "api-key",
    "api key",
    "secret",
    "passwd",
    "password",
    "contraseña",
    "contrasena",
    "token",
    "bearer",
    "private key",
    "aws_secret",
    "aws_access",
    "client_secret",
];

/// Prefijos de credencial que bastan por sí solos (`sk-...`, `ghp_...`).
const PREFIJOS_SECRETO: &[&str] = &["sk-", "ghp_", "gho_", "xoxa", "xoxb", "xoxp", "xoxs"];

/// ¿El texto parece contener un secreto? `true` → no se persiste.
#[must_use]
pub fn parece_secreto(texto: &str) -> bool {
    let minus = texto.to_lowercase();
    if minus.contains("-----begin") {
        return true; // Clave PEM.
    }
    if PREFIJOS_SECRETO.iter().any(|p| minus.contains(p)) {
        return true;
    }
    let bytes = minus.as_bytes();
    for clave in CLAVES_SECRETO {
        let mut desde = 0;
        while let Some(pos) = minus[desde..].find(clave) {
            let tras = &bytes[desde + pos + clave.len()..];
            let mut resto = tras.iter().peekable();
            while matches!(resto.peek(), Some(b' ' | b'\t')) {
                resto.next();
            }
            if matches!(resto.peek(), Some(b':') | Some(b'=')) {
                return true;
            }
            // `Bearer <valor>` (cabecera Authorization) no lleva separador:
            // si tras la palabra viene un token (racha ≥4 sin espacios),
            // es credencial. El umbral evita "bearer of bad news".
            if *clave == "bearer" {
                let racha: usize = resto.take_while(|b| !b.is_ascii_whitespace()).count();
                if racha >= 4 {
                    return true;
                }
            }
            desde += pos + clave.len();
        }
    }
    false
}

/// Limpia un candidato a recuerdo: recorta, descarta vacíos y secretos.
/// `None` = no persistir (vacío o parece credencial).
#[must_use]
pub fn sanitize_para_memoria(texto: &str) -> Option<String> {
    const MAX_CHARS: usize = 2000;
    let limpio = texto.trim();
    if limpio.is_empty() || parece_secreto(limpio) {
        return None;
    }
    if limpio.chars().count() > MAX_CHARS {
        Some(limpio.chars().take(MAX_CHARS).collect())
    } else {
        Some(limpio.to_string())
    }
}

// ---------------------------------------------------------------------------
// Extracción determinista (sync sin LLM)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Recuperación (prefetch)
// ---------------------------------------------------------------------------

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
        if let Err(e) = persistencia.memoria_upsert(user_id, &tocada).await {
            tracing::warn!(clave = %entrada.clave, %e, "memoria: no se pudo marcar uso");
        }
    }
}

// ---------------------------------------------------------------------------
// Proveedor base sobre el puerto
// ---------------------------------------------------------------------------

/// [069A-4] Implementación base de [`ProveedorMemoria`] sobre cualquier
/// tienda del puerto `AgentPersistence::memoria_*` (diseño §2, fase 2).
pub struct MemoriaBase {
    persistencia: Arc<dyn AgentPersistence>,
    limite_prefetch: usize,
}

impl MemoriaBase {
    #[must_use]
    pub fn nuevo(persistencia: Arc<dyn AgentPersistence>, limite_prefetch: usize) -> Self {
        Self {
            persistencia,
            limite_prefetch,
        }
    }
}

#[async_trait]
impl ProveedorMemoria for MemoriaBase {
    async fn prefetch(&self, user_id: Uuid, query: &str, limite: usize) -> Result<String> {
        let limite = limite.min(self.limite_prefetch).max(1);
        let entradas = self.persistencia.memoria_listar(user_id).await?;
        let (bloque, claves) = puntuar_y_formatear(&entradas, query, limite);
        if !claves.is_empty() {
            marcar_uso(&self.persistencia, user_id, &entradas, &claves).await;
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
            self.persistencia.memoria_upsert(user_id, &entrada).await?;
            guardadas.push(entrada);
        }
        Ok(guardadas)
    }
}

// ---------------------------------------------------------------------------
// Curador determinista (diseño §3)
// ---------------------------------------------------------------------------

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

    // 1) Duplicadas: mismo contenido normalizado → conserva la de mayor
    //    (usos, recencia) y poda las demás.
    let mut por_contenido: HashMap<String, Vec<MemoriaEntrada>> = HashMap::new();
    for entrada in &entradas {
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
    for entrada in &entradas {
        if !entrada.archivada()
            && !resumen.consolidadas.contains(&entrada.clave)
            && !vivas.contains_key(&entrada.clave)
        {
            vivas.insert(entrada.clave.clone(), entrada.clone());
        }
    }

    // 2) Obsoletas: viejas y sin uso reciente → archiva (marca de origen;
    //    `prefetch` las excluye pero se conservan para auditar/revertir).
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

    // 3) Promoción: madura + muy usada + sin skill homónima → skill activa.
    //    La tienda sin `skills_registrar` deja nota (no rompe la pasada).
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
        let skill = crate::ports::SkillEntrada {
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
    Ok(resumen)
}

// ---------------------------------------------------------------------------
// Tools memoria_* del agente (diseño §1 + fase 5)
// ---------------------------------------------------------------------------

/// Tool `memoria_guardar`: alta explícita de un recuerdo (con sanitizado;
/// los secretos se rechazan con error claro, nunca se guardan).
pub struct ToolMemoriaGuardar;

#[async_trait]
impl AgentTool for ToolMemoriaGuardar {
    fn id(&self) -> &str {
        "memoria_guardar"
    }
    fn descripcion(&self) -> &str {
        "Guarda un recuerdo persistente del usuario para futuros turnos.\
        \nQUÉ HACE: alta (o sustitución) de una entrada clave → contenido con fecha y origen.\
        \nCUÁNDO USARLA: cuando el usuario pide recordar algo explícitamente ('recuerda que...', 'a partir de ahora...') o hay un dato claramente reutilizable (preferencias, rutas, nombres).\
        \nFORMATO DE SALIDA: confirma la clave guardada.\
        \nERRORES: clave o contenido vacíos; contenido que parece credencial (no se guarda nunca)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "clave": { "type": "string", "description": "Identificador corto (p. ej. 'color-favorito')" },
                "contenido": { "type": "string", "description": "Lo que hay que recordar (sin secretos)" }
            },
            "required": ["clave", "contenido"]
        })
    }
    fn efecto(&self) -> bool {
        true
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let clave = argumentos
            .get("clave")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_guardar: clave requerida".into()))?;
        let contenido = argumentos
            .get("contenido")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Argumentos("memoria_guardar: contenido requerido".into()))?;
        let Some(limpio) = sanitize_para_memoria(contenido) else {
            return Err(Error::Validacion(
                "memoria_guardar: el contenido parece una credencial o está vacío; no se guarda"
                    .into(),
            ));
        };
        let entrada =
            MemoriaEntrada::nueva(clave.to_string(), limpio, "tool:memoria_guardar".into());
        ctx.persistencia
            .memoria_upsert(ctx.user_id, &entrada)
            .await?;
        Ok(AgentToolResult::ok(
            format!("recuerdo '{}' guardado", entrada.clave),
            format!("memoria_guardar: {}", entrada.clave),
        ))
    }
}

/// Tool `memoria_recordar`: búsqueda explícita en los recuerdos (solo
/// lectura; el prefetch automático ya inyecta lo relevante sin pedirlo).
pub struct ToolMemoriaRecordar;

#[async_trait]
impl AgentTool for ToolMemoriaRecordar {
    fn id(&self) -> &str {
        "memoria_recordar"
    }
    fn descripcion(&self) -> &str {
        "Busca en los recuerdos persistentes del usuario.\
        \nQUÉ HACE: devuelve las entradas que solapan con la consulta (hasta el límite).\
        \nCUÁNDO USARLA: cuando necesitas un dato del usuario que el contexto automático no trajo.\
        \nFORMATO DE SALIDA: una línea por recuerdo ('- clave: contenido'); vacío si no hay coincidencias."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "consulta": { "type": "string", "description": "Qué buscar (p. ej. 'color favorito')" },
                "limite": { "type": "integer", "description": "Tope de caracteres (default 2000, máx 8000)" }
            },
            "required": ["consulta"]
        })
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let consulta = argumentos
            .get("consulta")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_recordar: consulta requerida".into()))?;
        let limite = argumentos
            .get("limite")
            .and_then(Value::as_u64)
            .map(|l| (l as usize).clamp(1, 8000))
            .unwrap_or(2000);
        let entradas = ctx.persistencia.memoria_listar(ctx.user_id).await?;
        let (bloque, claves) = puntuar_y_formatear(&entradas, consulta, limite);
        if bloque.is_empty() {
            return Ok(AgentToolResult::ok(
                "(sin recuerdos coincidentes)",
                "memoria_recordar: sin resultados".to_string(),
            ));
        }
        Ok(AgentToolResult::ok(
            bloque.clone(),
            format!("memoria_recordar: {} ({})", claves.len(), claves.join(", ")),
        ))
    }
}

/// Tool `memoria_borrar`: elimina un recuerdo por clave (reversible solo si
/// el operador lo recuerda: pedir confirmación en lenguaje natural antes).
pub struct ToolMemoriaBorrar;

#[async_trait]
impl AgentTool for ToolMemoriaBorrar {
    fn id(&self) -> &str {
        "memoria_borrar"
    }
    fn descripcion(&self) -> &str {
        "Borra un recuerdo persistente por su clave.\
        \nQUÉ HACE: elimina la entrada (el curador nunca la recuperará).\
        \nCUÁNDO USARLA: cuando el usuario pide olvidar algo explícitamente.\
        \nFORMATO DE SALIDA: confirma la clave borrada (aunque no existiera, para no filtrar qué hay guardado)."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "clave": { "type": "string", "description": "Clave del recuerdo a borrar" }
            },
            "required": ["clave"]
        })
    }
    fn efecto(&self) -> bool {
        true
    }
    async fn ejecutar(
        &self,
        ctx: &AgentToolContext<'_>,
        argumentos: Value,
    ) -> Result<AgentToolResult> {
        let clave = argumentos
            .get("clave")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .ok_or_else(|| Error::Argumentos("memoria_borrar: clave requerida".into()))?;
        ctx.persistencia.memoria_borrar(ctx.user_id, clave).await?;
        Ok(AgentToolResult::ok(
            format!("recuerdo '{clave}' borrado"),
            format!("memoria_borrar: {clave}"),
        ))
    }
}

/// Registra las tres tools de memoria (siempre: la persistencia es puerto
/// obligatorio, nunca `None`; el sanitizado protege la escritura).
pub fn registrar_tools_memoria(registry: &mut AgentToolRegistry) {
    registry.registrar(Box::new(ToolMemoriaGuardar));
    registry.registrar(Box::new(ToolMemoriaRecordar));
    registry.registrar(Box::new(ToolMemoriaBorrar));
}

#[cfg(test)]
mod pruebas {
    //! [069A-4] Sanitizado (secretos nunca se guardan), extracción
    //! determinista, ranking del prefetch, curador y tools, con tienda
    //! observable en memoria (el mock de contrato no observa escrituras).
    use super::*;
    use crate::ports::{
        AccionAuditable, MensajePersistido, SkillEntrada, TareaProgramadaPendiente, TurnoPersistido,
    };

    #[derive(Default)]
    struct TiendaPrueba {
        memoria: std::sync::Mutex<HashMap<Uuid, HashMap<String, MemoriaEntrada>>>,
        skills: std::sync::Mutex<HashMap<Uuid, Vec<SkillEntrada>>>,
        /// Simula una tienda sin `skills_registrar` (legacy): la promoción
        /// deja nota en vez de romper la pasada.
        sin_registro: bool,
    }

    impl TiendaPrueba {
        fn sembrar(&self, user_id: Uuid, entradas: Vec<MemoriaEntrada>) {
            let mut mapa = self.memoria.lock().unwrap_or_else(|p| p.into_inner());
            let slot = mapa.entry(user_id).or_default();
            for e in entradas {
                slot.insert(e.clave.clone(), e);
            }
        }

        fn leer(&self, user_id: Uuid, clave: &str) -> Option<MemoriaEntrada> {
            self.memoria
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&user_id)
                .and_then(|m| m.get(clave))
                .cloned()
        }
    }

    #[async_trait]
    impl AgentPersistence for TiendaPrueba {
        async fn guardar_turno(&self, _: &TurnoPersistido) -> Result<()> {
            Ok(())
        }
        async fn finalizar_turno(&self, _: Uuid, _: &str, _: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn guardar_mensaje(&self, _: &MensajePersistido) -> Result<()> {
            Ok(())
        }
        async fn listar_mensajes(&self, _: Uuid) -> Result<Vec<MensajePersistido>> {
            Ok(Vec::new())
        }
        async fn conversacion_tocar(&self, _: Uuid) -> Result<()> {
            Ok(())
        }
        async fn registrar_accion(&self, _: &AccionAuditable) -> Result<()> {
            Ok(())
        }
        async fn memoria_listar(&self, user_id: Uuid) -> Result<Vec<MemoriaEntrada>> {
            Ok(self
                .memoria
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&user_id)
                .map(|m| m.values().cloned().collect())
                .unwrap_or_default())
        }
        async fn memoria_upsert(&self, user_id: Uuid, entrada: &MemoriaEntrada) -> Result<()> {
            self.memoria
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .entry(user_id)
                .or_default()
                .insert(entrada.clave.clone(), entrada.clone());
            Ok(())
        }
        async fn memoria_borrar(&self, user_id: Uuid, clave: &str) -> Result<()> {
            if let Some(m) = self
                .memoria
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get_mut(&user_id)
            {
                m.remove(clave);
            }
            Ok(())
        }
        async fn skills_listar(&self, user_id: Uuid) -> Result<Vec<SkillEntrada>> {
            Ok(self
                .skills
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&user_id)
                .cloned()
                .unwrap_or_default())
        }
        async fn skills_registrar(&self, user_id: Uuid, skill: &SkillEntrada) -> Result<()> {
            if self.sin_registro {
                return Err(Error::Persistencia(
                    "skills_registrar no implementado por esta tienda".into(),
                ));
            }
            let mut guard = self.skills.lock().unwrap_or_else(|p| p.into_inner());
            let lista = guard.entry(user_id).or_default();
            if let Some(previa) = lista.iter_mut().find(|s| s.nombre == skill.nombre) {
                *previa = skill.clone();
            } else {
                lista.push(skill.clone());
            }
            Ok(())
        }
        async fn tareas_recuperar_interrumpidas(&self) -> Result<u64> {
            Ok(0)
        }
        async fn tareas_pendientes(&self, _: u32) -> Result<Vec<TareaProgramadaPendiente>> {
            Ok(Vec::new())
        }
        async fn tarea_tomar(&self, _: Uuid) -> Result<bool> {
            Ok(false)
        }
        async fn tarea_finalizar(&self, _: Uuid, _: bool, _: Option<&str>) -> Result<()> {
            Ok(())
        }
        async fn tarea_reprogramar(
            &self,
            _: Uuid,
            _: Uuid,
            _: Option<DateTime<Utc>>,
        ) -> Result<()> {
            Ok(())
        }
    }

    fn entrada_vieja(clave: &str, contenido: &str, dias: i64, usos: u32) -> MemoriaEntrada {
        let mut e = MemoriaEntrada::nueva(clave.into(), contenido.into(), "t".into());
        e.actualizada_en = Utc::now() - chrono::Duration::days(dias);
        e.usos = usos;
        e
    }

    // --- Sanitizado ---

    #[test]
    fn secretos_por_asignacion_se_rechazan() {
        for texto in [
            "mi api_key = abc123",
            "token: xyz-secreto",
            "password= qwerty",
            "usa el Bearer abcdef",
            "aws_secret = AKIA...",
            "client_secret: s3cr3t",
        ] {
            assert!(parece_secreto(texto), "debería parecer secreto: {texto}");
            assert!(sanitize_para_memoria(texto).is_none());
        }
    }

    #[test]
    fn prefijos_y_pem_se_rechazan() {
        assert!(parece_secreto("la clave sk-abc123def"));
        assert!(parece_secreto("token ghp_xxYYzz1122"));
        assert!(parece_secreto("-----BEGIN PRIVATE KEY-----"));
    }

    #[test]
    fn palabra_suelta_sin_asignacion_no_es_secreto() {
        // Los turnos hablan de "tokens" del modelo: sin `:`/`=` no hay
        // credencial y el recuerdo se conserva.
        for texto in [
            "cuenta los tokens usados en el turno",
            "el token de la sesión expiró y se renovó",
            "mi color favorito es el azul",
            "recuerda que prefiero respuestas concisas",
        ] {
            assert!(!parece_secreto(texto), "falso positivo: {texto}");
            assert!(sanitize_para_memoria(texto).is_some());
        }
    }

    #[test]
    fn vacio_y_recorte() {
        assert!(sanitize_para_memoria("   ").is_none());
        let largo = "x".repeat(3000);
        let recortado = sanitize_para_memoria(&largo).expect("no vacío");
        assert_eq!(recortado.chars().count(), 2000);
    }

    // --- Extracción ---

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

    // --- Prefetch ---

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
        let base = MemoriaBase::nuevo(tienda.clone(), 2000);
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
        let base = MemoriaBase::nuevo(tienda, 2000);
        let bloque = base
            .prefetch(user_id, "gusto rojo", 2000)
            .await
            .expect("prefetch");
        assert!(bloque.is_empty(), "la archivada no se recuerda: {bloque}");
    }

    // --- Sync ---

    #[tokio::test]
    async fn sync_guarda_candidatos_con_origen() {
        let tienda: Arc<dyn AgentPersistence> = Arc::new(TiendaPrueba::default());
        let user_id = Uuid::new_v4();
        let base = MemoriaBase::nuevo(tienda.clone(), 2000);
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
        let listado = tienda.memoria_listar(user_id).await.expect("listar");
        assert_eq!(listado.len(), 1);
    }

    // --- Curador ---

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
        let tienda = Arc::new(TiendaPrueba {
            sin_registro: true,
            ..TiendaPrueba::default()
        });
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

    // --- Tools ---

    fn contexto<'a>(tienda: &'a TiendaPrueba, user_id: Uuid) -> AgentToolContext<'a> {
        AgentToolContext {
            user_id,
            persistencia: tienda,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
        }
    }

    #[tokio::test]
    async fn tool_guardar_y_recordar() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        let ctx = contexto(&tienda, user_id);
        let r = ToolMemoriaGuardar
            .ejecutar(
                &ctx,
                json!({"clave": "color", "contenido": "prefiere el azul"}),
            )
            .await
            .expect("guarda");
        assert!(r.contenido.contains("color"));
        let r = ToolMemoriaRecordar
            .ejecutar(&ctx, json!({"consulta": "qué color prefiere"}))
            .await
            .expect("recuerda");
        assert!(r.contenido.contains("azul"));
    }

    #[tokio::test]
    async fn tool_guardar_rechaza_secretos() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        let ctx = contexto(&tienda, user_id);
        let err = ToolMemoriaGuardar
            .ejecutar(&ctx, json!({"clave": "k", "contenido": "mi api_key = abc"}))
            .await
            .expect_err("el secreto no se guarda");
        assert!(err.to_string().contains("credencial"));
        assert!(tienda.leer(user_id, "k").is_none());
    }

    #[tokio::test]
    async fn tool_borrar_elimina() {
        let tienda = TiendaPrueba::default();
        let user_id = Uuid::new_v4();
        tienda.sembrar(
            user_id,
            vec![MemoriaEntrada::nueva(
                "viejo".into(),
                "dato".into(),
                "t".into(),
            )],
        );
        let ctx = contexto(&tienda, user_id);
        ToolMemoriaBorrar
            .ejecutar(&ctx, json!({"clave": "viejo"}))
            .await
            .expect("borra");
        assert!(tienda.leer(user_id, "viejo").is_none());
    }
}
