/* [03-09-2026] Tool `programar_tarea` del núcleo (plan 318A-16, F6): el
 * agente crea tareas programadas desde la conversación, diciéndolo en
 * lenguaje natural ("revisa el repo cada lunes a las 9"), con paridad del
 * cron en lenguaje natural de hermes (`cron/`) y de grok (`schedule.ts`).
 *
 * La traducción NL→cron es una función PURA (`frase_a_cron`): sin proveedor
 * ni hora del sistema, así los tests son deterministas. Produce el cron v2
 * de 5 campos `M H * * DOW` que el scheduler del núcleo ya entiende, o los
 * intervalos v1 `cada{N}min|h|d`; si la frase no se entiende devuelve un
 * error EXPLÍCITO con los patrones soportados (nunca un falso "diario").
 *
 * El puerto de gestión ([`ProgramadorTareas`]) es opcional como el de
 * comandos: el runtime registra la tool SOLO cuando el consumidor lo
 * inyecta (fail-closed). La próxima ejecución la calcula el núcleo con
 * `cron::proxima_ejecucion` desde `Utc::now()` antes de crear.
 */

use crate::error::{Error, Result};
use crate::ports::{NuevaTareaProgramada, ProgramadorTareas, TareaProgramada};
use crate::scheduler;
use crate::tool::{AgentTool, AgentToolContext, AgentToolRegistry, AgentToolResult};
use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

/// Días de la semana para NL→cron (0=domingo..6=sábado, como cron).
const DIAS_SEMANA: &[(&str, u32)] = &[
    ("domingo", 0),
    ("lunes", 1),
    ("martes", 2),
    ("miercoles", 3),
    ("miércoles", 3),
    ("jueves", 4),
    ("viernes", 5),
    ("sabado", 6),
    ("sábado", 6),
];

/// Traduce una frase en lenguaje natural a una expresión cron soportada por
/// el scheduler del núcleo (v2 `M H * * DOW` o v1 `cada{N}min|h|d`).
///
/// Patrones soportados (normalización: minúsculas, sin puntuación):
/// - `cada lunes a las 9` / `los lunes a las 9:30` → `30 9 * * 1`
///   (día de la semana; hora por defecto 09:00 si se omite)
/// - `todos los dias a las 9` / `diario a las 9` → `0 9 * * *`
/// - `cada hora`, `cada 2 horas`, `cada 30 minutos`, `cada 3 dias` →
///   `cada1h`, `cada2h`, `cada30min`, `cada3d` (v1, relativo a la ejecución)
///
/// Cualquier otra frase devuelve [`Error::Validacion`] con los patrones
/// aceptados (fallback explícito del checklist F6).
pub fn frase_a_cron(frase: &str) -> Result<String> {
    let f = normalizar(frase);

    /* 1) Intervalos v1: "cada N unidad(es)" o "cada unidad" (N=1). */
    if let Some(resto) = f.strip_prefix("cada") {
        let resto = resto.trim_start();
        /* Sin unidad de tiempo a la vista → no es un intervalo reconocible. */
        if !resto.is_empty() {
            if let Some(cron) = intervalo(resto)? {
                return Ok(cron);
            }
            /* "cada lunes ..." no es intervalo: cae al análisis de día. */
        }
    }

    /* 2) Día de la semana / diario con hora opcional. */
    if let Some(dow) = dia_de_la_frase(&f) {
        let hora = hora_de_la_frase(&f);
        let (min, hor) = hora.unwrap_or((0, 9));
        return Ok(format!("{min} {hor} * * {dow}"));
    }
    if es_diario(&f) {
        let hora = hora_de_la_frase(&f);
        let (min, hor) = hora.unwrap_or((0, 9));
        return Ok(format!("{min} {hor} * * *"));
    }

    Err(Error::Validacion(format!(
        "No entendí la programación '{frase}'. Usa frases como: 'cada lunes a las 9', \
         'diario a las 9:30', 'cada hora', 'cada 2 horas', 'cada 30 minutos' o 'cada 3 dias'."
    )))
}

/// Normaliza la frase: minúsculas; la puntuación (salvo `:` de la hora,
/// que `parse_hora` necesita) pasa a espacios.
fn normalizar(frase: &str) -> String {
    frase
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() || c == ':' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Intenta parsear `cada N unidad` / `cada unidad`. `Ok(None)` si el resto no
/// parece una unidad de tiempo (p. ej. arranca con un día de la semana).
fn intervalo(resto: &str) -> Result<Option<String>> {
    let palabras: Vec<&str> = resto.split_whitespace().collect();
    let (numero, unidad, resto_tras_unidad) = match palabras.as_slice() {
        /* [059A-S7] Sin expect en producción: el guard ya validó el parse, así
         * que unwrap_or(1) solo extrae el valor sin poder panickear (nunca
         * cae al fallback tras un guard Ok). */
        [num, unidad, rest @ ..] if num.parse::<u32>().is_ok() => {
            (num.parse::<u32>().ok().unwrap_or(1), *unidad, rest)
        }
        [unidad, rest @ ..] => (1u32, *unidad, rest),
        _ => return Ok(None),
    };
    /* Si tras la unidad queda material (p. ej. "cada 2 horas y media"), el
     * subconjunto v1 no lo cubre: fallback explícito. */
    if !resto_tras_unidad.is_empty() {
        return Ok(None);
    }
    let sufijo = match unidad {
        "minuto" | "minutos" | "min" | "mins" => "min",
        "hora" | "horas" | "h" => "h",
        "dia" | "dias" | "día" | "días" | "d" => "d",
        _ => return Ok(None),
    };
    Ok(Some(format!("cada{numero}{sufijo}")))
}

/// Día de la semana si la frase menciona uno (`None` si no).
fn dia_de_la_frase(f: &str) -> Option<u32> {
    for (nombre, dow) in DIAS_SEMANA {
        if f.split_whitespace().any(|p| p == *nombre) {
            return Some(*dow);
        }
    }
    None
}

/// ¿La frase pide "todos los días" / "diario" (sin día concreto)?
fn es_diario(f: &str) -> bool {
    f.contains("diario")
        || f.contains("todos los dias")
        || f.contains("cada dia")
        || f.contains("cada día")
        || f.contains("todos los días")
}

/// Hora `(minuto, hora)` si la frase contiene "a las HH[:MM]".
fn hora_de_la_frase(f: &str) -> Option<(u32, u32)> {
    let palabras: Vec<&str> = f.split_whitespace().collect();
    for (i, p) in palabras.iter().enumerate() {
        if (*p == "a" || *p == "las") && i + 1 < palabras.len() {
            if let Some((min, hor)) = parse_hora(palabras[i + 1]) {
                return Some((min, hor));
            }
        }
        if let Some((min, hor)) = parse_hora(p) {
            /* "9:30" suelto también cuenta (p. ej. "revisa a las 9:30"). */
            if p.contains(':') {
                return Some((min, hor));
            }
        }
    }
    None
}

/// `"9"` → (0, 9); `"9:30"` → (30, 9). Valida rangos cron (0-23 / 0-59).
fn parse_hora(tok: &str) -> Option<(u32, u32)> {
    let (h, m) = match tok.split_once(':') {
        Some((h, m)) => (h, m),
        None => (tok, "0"),
    };
    let hor: u32 = h.parse().ok()?;
    let min: u32 = m.parse().ok()?;
    if hor > 23 || min > 59 {
        return None;
    }
    Some((min, hor))
}

/// Formatea una tarea para el modelo (listar/logs). Sin secretos: el prompt
/// de la tarea se recorta a 80 caracteres para no saturar el contexto.
fn tarea_a_texto(t: &TareaProgramada) -> String {
    let prompt = if t.prompt.chars().count() > 80 {
        let recortado: String = t.prompt.chars().take(80).collect();
        format!("{recortado}…")
    } else {
        t.prompt.clone()
    };
    let proxima = t
        .proxima_ejecucion
        .map(|p| p.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "—".to_string());
    format!(
        "{} [{}] cron='{}' próximo='{proxima}' estado={} — \"{}\"",
        t.nombre,
        t.tipo,
        t.cron_expr.as_deref().unwrap_or("—"),
        t.estado,
        prompt
    )
}

/// Tool `programar_tarea`: crear/listar/cancelar/logs sobre el puerto
/// [`ProgramadorTareas`]. Solo el agente principal: los subagentes no la ven
/// (whitelist de perfiles, verificado por test).
pub struct ToolProgramarTarea {
    programador: Arc<dyn ProgramadorTareas>,
}

impl ToolProgramarTarea {
    #[must_use]
    pub fn nuevo(programador: Arc<dyn ProgramadorTareas>) -> Self {
        Self { programador }
    }
}

#[async_trait]
impl AgentTool for ToolProgramarTarea {
    fn id(&self) -> &'static str {
        "programar_tarea"
    }
    fn descripcion(&self) -> &'static str {
        "Programa una tarea del agente para que se ejecute sola en el futuro.\
\nQUÉ HACE: crea, lista, cancela o consulta los registros de una tarea programada.\
\nCUÁNDO USARLA: cuando el usuario pide algo recurrente o diferido en lenguaje natural,\
\np. ej. 'revisa el repo cada lunes a las 9' (se traduce a cron automáticamente).\
\nFORMATO DE SALIDA: al crear devuelve el id y la próxima ejecución; al listar, una línea por tarea.\
\nERRORES: frase de programación no entendida (te dice los patrones válidos), tarea inexistente."
    }
    fn schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "accion": {
                    "type": "string",
                    "enum": ["crear", "listar", "cancelar", "logs"],
                    "description": "Operación: crear (nueva tarea), listar (mis tareas), cancelar (desprogramar), logs (últimas ejecuciones de una tarea)"
                },
                "nombre": {
                    "type": "string",
                    "description": "Nombre corto de la tarea (crear); sin espacios raros, p. ej. 'revisar-repo'"
                },
                "prompt": {
                    "type": "string",
                    "description": "Instrucción que se ejecutará cuando toque (crear); descripción de qué hace"
                },
                "cuando": {
                    "type": "string",
                    "description": "Programación en lenguaje natural (crear): 'cada lunes a las 9', 'diario a las 9:30', 'cada hora', 'cada 30 minutos'"
                },
                "id": {
                    "type": "string",
                    "description": "UUID de la tarea (cancelar/logs); se obtiene al crearla o al listar"
                }
            },
            "required": ["accion"]
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
        let accion = argumentos
            .get("accion")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Argumentos(
                    "programar_tarea: accion requerida (crear|listar|cancelar|logs)".into(),
                )
            })?;
        let programador: &dyn ProgramadorTareas = self.programador.as_ref();
        match accion {
            "crear" => crear(programador, ctx.user_id, &argumentos).await,
            "listar" => listar(programador, ctx.user_id).await,
            "cancelar" => cancelar(programador, ctx.user_id, &argumentos).await,
            "logs" => logs(programador, ctx.user_id, &argumentos).await,
            otra => Err(Error::Argumentos(format!(
                "programar_tarea: accion desconocida '{otra}' (crear|listar|cancelar|logs)"
            ))),
        }
    }
}

/// Crea la tarea: NL→cron puro, próxima ejecución calculada por el scheduler
/// (agnóstica) y alta en el puerto. `Err` claro si la frase no se entiende.
async fn crear(
    programador: &dyn ProgramadorTareas,
    user_id: Uuid,
    argumentos: &Value,
) -> Result<AgentToolResult> {
    let nombre = argumentos
        .get("nombre")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| Error::Argumentos("programar_tarea: nombre requerido para crear".into()))?;
    let prompt = argumentos
        .get("prompt")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .ok_or_else(|| Error::Argumentos("programar_tarea: prompt requerido para crear".into()))?;
    let cuando = argumentos
        .get("cuando")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .ok_or_else(|| {
            Error::Argumentos(
                "programar_tarea: 'cuando' requerido (p. ej. 'cada lunes a las 9')".into(),
            )
        })?;

    let cron = frase_a_cron(cuando)?;
    let proxima = scheduler::proxima_ejecucion(&cron, Utc::now())?;
    let nueva = NuevaTareaProgramada {
        user_id,
        nombre: nombre.to_string(),
        prompt: prompt.to_string(),
        tipo: "recurrente".into(),
        cron_expr: cron.clone(),
        proxima_ejecucion: proxima,
    };
    let id = programador.tarea_crear(&nueva).await?;
    Ok(AgentToolResult::ok(
        format!(
            "Tarea programada creada: {nombre} (id {id}) — cron '{cron}' — próxima ejecución {}.",
            proxima.format("%Y-%m-%d %H:%M UTC")
        ),
        format!("programar_tarea: creó '{nombre}' con cron {cron}"),
    ))
}

async fn listar(programador: &dyn ProgramadorTareas, user_id: Uuid) -> Result<AgentToolResult> {
    let tareas = programador.tareas_listar(user_id).await?;
    if tareas.is_empty() {
        return Ok(AgentToolResult::ok(
            "No hay tareas programadas para este usuario.",
            "programar_tarea: listar (0)",
        ));
    }
    let mut lineas: Vec<String> = Vec::with_capacity(tareas.len());
    for t in &tareas {
        lineas.push(tarea_a_texto(t));
    }
    Ok(AgentToolResult::ok(
        format!(
            "Tareas programadas ({}):\n{}",
            tareas.len(),
            lineas.join("\n")
        ),
        format!("programar_tarea: listar ({})", tareas.len()),
    ))
}

async fn cancelar(
    programador: &dyn ProgramadorTareas,
    user_id: Uuid,
    argumentos: &Value,
) -> Result<AgentToolResult> {
    let id = id_de_argumentos(argumentos)?;
    match programador.tarea_cancelar(id, user_id).await? {
        true => Ok(AgentToolResult::ok(
            format!("Tarea {id} cancelada (desprogramada)."),
            format!("programar_tarea: canceló {id}"),
        )),
        false => Err(Error::NoEncontrado(format!(
            "programar_tarea: no existe la tarea {id} para este usuario"
        ))),
    }
}

async fn logs(
    programador: &dyn ProgramadorTareas,
    user_id: Uuid,
    argumentos: &Value,
) -> Result<AgentToolResult> {
    let id = id_de_argumentos(argumentos)?;
    let registros = programador.tarea_logs(id, user_id, 10).await?;
    if registros.is_empty() {
        return Ok(AgentToolResult::ok(
            format!("La tarea {id} no tiene ejecuciones registradas."),
            "programar_tarea: logs (0)",
        ));
    }
    let mut lineas: Vec<String> = Vec::with_capacity(registros.len());
    for r in &registros {
        let estado = if r.ok { "ok" } else { "fallo" };
        let resumen = if r.resumen.chars().count() > 100 {
            let recortado: String = r.resumen.chars().take(100).collect();
            format!("{recortado}…")
        } else {
            r.resumen.clone()
        };
        lineas.push(format!(
            "{} [{estado}] {}",
            r.ejecutada_en.format("%Y-%m-%d %H:%M UTC"),
            resumen
        ));
    }
    Ok(AgentToolResult::ok(
        format!(
            "Ejecuciones de {id} (últimas {}):\n{}",
            registros.len(),
            lineas.join("\n")
        ),
        format!("programar_tarea: logs ({})", registros.len()),
    ))
}

fn id_de_argumentos(argumentos: &Value) -> Result<Uuid> {
    argumentos
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::Argumentos("programar_tarea: id requerido (cancelar/logs)".into()))
        .and_then(|s| {
            Uuid::parse_str(s)
                .map_err(|_| Error::Argumentos(format!("programar_tarea: id inválido '{s}'")))
        })
}

/// Registra la tool SOLO cuando el consumidor inyecta el puerto (fail-closed,
/// patrón `EjecutorComando` de F3).
pub fn registrar_tool_programar_tarea(
    registry: &mut AgentToolRegistry,
    programador: Arc<dyn ProgramadorTareas>,
) {
    registry.registrar(Box::new(ToolProgramarTarea::nuevo(programador)));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock del puerto: registra altas/cancelaciones y devuelve el id pedido.
    struct ProgramadorMock {
        creadas: std::sync::Mutex<Vec<NuevaTareaProgramada>>,
        canceladas: std::sync::Mutex<Vec<Uuid>>,
        entregas: std::sync::Mutex<Vec<(Uuid, bool, String)>>,
        contador: AtomicUsize,
    }

    impl ProgramadorMock {
        fn nuevo() -> Self {
            Self {
                creadas: std::sync::Mutex::new(Vec::new()),
                canceladas: std::sync::Mutex::new(Vec::new()),
                entregas: std::sync::Mutex::new(Vec::new()),
                contador: AtomicUsize::new(0),
            }
        }
    }

    #[async_trait]
    impl ProgramadorTareas for ProgramadorMock {
        async fn tarea_crear(&self, nueva: &NuevaTareaProgramada) -> Result<Uuid> {
            self.creadas.lock().expect("lock").push(nueva.clone());
            let n = self.contador.fetch_add(1, Ordering::SeqCst) as u64;
            Ok(Uuid::from_u64_pair(0, n + 1))
        }
        async fn tareas_listar(&self, _user_id: Uuid) -> Result<Vec<TareaProgramada>> {
            Ok(self
                .creadas
                .lock()
                .expect("lock")
                .iter()
                .map(|n| TareaProgramada {
                    id: Uuid::new_v4(),
                    user_id: n.user_id,
                    nombre: n.nombre.clone(),
                    prompt: n.prompt.clone(),
                    tipo: n.tipo.clone(),
                    cron_expr: Some(n.cron_expr.clone()),
                    proxima_ejecucion: Some(n.proxima_ejecucion),
                    estado: "pendiente".into(),
                    creado_en: n.proxima_ejecucion,
                })
                .collect())
        }
        async fn tarea_cancelar(&self, id: Uuid, _user_id: Uuid) -> Result<bool> {
            self.canceladas.lock().expect("lock").push(id);
            Ok(true)
        }
        async fn tarea_logs(
            &self,
            _id: Uuid,
            _user_id: Uuid,
            _limite: u32,
        ) -> Result<Vec<crate::ports::LogTareaEjecucion>> {
            Ok(Vec::new())
        }
        async fn tarea_registrar_log(
            &self,
            id: Uuid,
            _user_id: Uuid,
            ok: bool,
            resumen: &str,
        ) -> Result<()> {
            self.entregas
                .lock()
                .expect("lock")
                .push((id, ok, resumen.to_string()));
            Ok(())
        }
    }

    fn ctx_con_programador(_programador: Arc<dyn ProgramadorTareas>) -> AgentToolContext<'static> {
        let persistencia: &'static crate::contrato_tests::PersistenciaMock =
            Box::leak(Box::new(crate::contrato_tests::PersistenciaMock::default()));
        AgentToolContext {
            ambito_memoria: crate::ports::AmbitoMemoria::Global,
            user_id: Uuid::new_v4(),
            persistencia,
            web_fetch: None,
            web_search: None,
            ai_provider: None,
            sandbox_archivos: None,
            dominio: None,
            todo: None,
            plan: None,
            navegador: None,
        }
    }

    /* --- NL→cron puro (determinista, sin reloj ni proveedor) --- */

    #[test]
    fn nl_lunes_a_las_9() {
        assert_eq!(
            frase_a_cron("cada lunes a las 9").expect("válido"),
            "0 9 * * 1"
        );
        assert_eq!(
            frase_a_cron("los lunes a las 9").expect("válido"),
            "0 9 * * 1"
        );
        assert_eq!(
            frase_a_cron("todos los lunes a las 9:30").expect("válido"),
            "30 9 * * 1"
        );
    }

    #[test]
    fn nl_sin_hora_usa_default_9() {
        assert_eq!(frase_a_cron("cada lunes").expect("válido"), "0 9 * * 1");
        assert_eq!(
            frase_a_cron("revisa el repo cada viernes").expect("válido"),
            "0 9 * * 5"
        );
    }

    #[test]
    fn nl_diario_y_resto_de_semana() {
        assert_eq!(frase_a_cron("diario a las 9").expect("válido"), "0 9 * * *");
        assert_eq!(
            frase_a_cron("todos los dias a las 8:15").expect("válido"),
            "15 8 * * *"
        );
        assert_eq!(
            frase_a_cron("cada domingo a las 9").expect("válido"),
            "0 9 * * 0"
        );
    }

    #[test]
    fn nl_intervalos_v1() {
        assert_eq!(frase_a_cron("cada hora").expect("válido"), "cada1h");
        assert_eq!(frase_a_cron("cada 2 horas").expect("válido"), "cada2h");
        assert_eq!(
            frase_a_cron("cada 30 minutos").expect("válido"),
            "cada30min"
        );
        assert_eq!(frase_a_cron("cada 3 dias").expect("válido"), "cada3d");
    }

    #[test]
    fn nl_frases_invalidas_dan_error_explicito() {
        for frase in [
            "cuando quieras",
            "en un rato",
            "cada vez que se pueda",
            "",
            "a las 25:99",
        ] {
            let err = frase_a_cron(frase).expect_err("debe fallar");
            let msg = err.to_string();
            assert!(
                msg.contains("No entendí") || msg.contains("válido"),
                "error claro para '{frase}': {msg}"
            );
            assert!(
                msg.contains("cada lunes a las 9"),
                "el error lista los patrones soportados: {msg}"
            );
        }
    }

    /* --- Tool sobre el puerto (E2E determinista del checklist) --- */

    #[tokio::test]
    async fn e2e_cada_lunes_crea_tarea_con_cron_correcto() {
        let mock = Arc::new(ProgramadorMock::nuevo());
        let ctx = ctx_con_programador(mock.clone());
        let tool = ToolProgramarTarea::nuevo(mock.clone());

        let r = tool
            .ejecutar(
                &ctx,
                json!({
                    "accion": "crear",
                    "nombre": "revisar-repo",
                    "prompt": "Revisa el repo en busca de cambios pendientes",
                    "cuando": "cada lunes a las 9"
                }),
            )
            .await
            .expect("crear ok");
        assert!(r.ok);
        assert!(
            r.contenido.contains("cron '0 9 * * 1'"),
            "el cron traducido aparece en la respuesta: {}",
            r.contenido
        );

        let creadas = mock.creadas.lock().expect("lock");
        assert_eq!(creadas.len(), 1);
        let nueva = &creadas[0];
        assert_eq!(nueva.nombre, "revisar-repo");
        assert_eq!(nueva.cron_expr, "0 9 * * 1");
        assert_eq!(nueva.tipo, "recurrente");
        assert!(
            nueva.proxima_ejecucion > Utc::now(),
            "la próxima ejecución queda en el futuro"
        );
    }

    #[tokio::test]
    async fn tool_rechaza_frase_no_entendida_sin_crear() {
        let mock = Arc::new(ProgramadorMock::nuevo());
        let ctx = ctx_con_programador(mock.clone());
        let tool = ToolProgramarTarea::nuevo(mock.clone());

        let err = tool
            .ejecutar(
                &ctx,
                json!({
                    "accion": "crear",
                    "nombre": "raro",
                    "prompt": "x",
                    "cuando": "cuando el sol salga dos veces"
                }),
            )
            .await
            .expect_err("frase inválida falla");
        assert!(err.to_string().contains("No entendí"));
        let creadas = mock.creadas.lock().expect("lock");
        assert!(
            creadas.is_empty(),
            "una frase no entendida nunca crea una tarea (sin falsos positivos)"
        );
    }

    #[tokio::test]
    async fn tool_lista_cancela_y_valida_id() {
        let mock = Arc::new(ProgramadorMock::nuevo());
        let ctx = ctx_con_programador(mock.clone());
        let tool = ToolProgramarTarea::nuevo(mock.clone());

        tool.ejecutar(
            &ctx,
            json!({
                "accion": "crear",
                "nombre": "backup",
                "prompt": "Haz backup",
                "cuando": "cada 2 horas"
            }),
        )
        .await
        .expect("crear");

        let listado = tool
            .ejecutar(&ctx, json!({"accion": "listar"}))
            .await
            .expect("listar");
        assert!(listado.contenido.contains("backup"));
        assert!(listado.contenido.contains("cada2h"));

        let cancelado = tool
            .ejecutar(
                &ctx,
                json!({"accion": "cancelar", "id": "00000000-0000-0000-0000-000000000001"}),
            )
            .await
            .expect("cancelar");
        assert!(cancelado.contenido.contains("cancelada"));

        let err = tool
            .ejecutar(&ctx, json!({"accion": "cancelar", "id": "no-es-un-uuid"}))
            .await
            .expect_err("id inválido");
        assert!(err.to_string().contains("id inválido"));

        let err = tool
            .ejecutar(
                &ctx,
                json!({"accion": "crear", "nombre": "x", "prompt": "p"}),
            )
            .await
            .expect_err("falta cuando");
        assert!(err.to_string().contains("cuando"));
    }

    #[test]
    fn registro_fail_closed_solo_con_puerto() {
        let mut registry = AgentToolRegistry::new();
        assert!(
            !registry.ids().contains(&"programar_tarea"),
            "sin puerto la tool NO existe (fail-closed)"
        );
        registrar_tool_programar_tarea(&mut registry, Arc::new(ProgramadorMock::nuevo()));
        assert!(registry.ids().contains(&"programar_tarea"));
        assert!(
            registry.tiene_efecto("programar_tarea"),
            "crear/cancelar son efectos"
        );
    }
}
