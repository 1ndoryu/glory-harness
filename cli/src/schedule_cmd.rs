//! Subcomando `glory-harness schedule <list|create|remove|logs|run>`:
//! administra y ejecuta las tareas programadas del CLI standalone sobre la BD
//! durable (`%APPDATA%/glory-harness/glory-harness.db`, [B3-F8a]).
//! `PersistenciaSqlite` implementa AMBAS caras (CRUD [`ProgramadorTareas`] +
//! cola del scheduler en [`AgentPersistence`]), de modo que `create` en un
//! proceso y `run` en otro comparten las tareas; el `user_id` es estable
//! (tabla `config`). `run` ejecuta las vencidas como turnos del agente y
//! entrega el resumen en `tarea_logs` (núcleo `cron::ejecutar_lista`, claim
//! fence por tarea).
//!
//! [069A-5 F5] Extraído de `main.rs` (limite-lineas 532): el binario queda
//! como despachador fino; el dominio schedule vive aquí.
//! Resultado del subcomando: `Uso` = error de argumentos (exit 2).

use std::process::ExitCode;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use glory_harness::{run, PersistenciaSqlite};
use glory_harness::servicio::sesion_config::leer_gancho_pre_compact;
use glory_harness_core::ports::{TareaProgramada, TareaProgramadaPendiente};
use glory_harness_core::{AgentPersistence, HarnessError, ProgramadorTareas};
use uuid::Uuid;

use crate::extraer_opcion;

/// Resultado del subcomando `schedule`: `Uso` = error de argumentos (exit 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SalidaSchedule {
    Ok,
    Uso,
}

/// Abre la BD de la app y resuelve el usuario estable del CLI (se crea una
/// vez en `config` y se reutiliza: las tareas sobreviven a los procesos).
/// [069A-2] Delegación al helper compartido de `run` (misma tienda y usuario
/// que `chat`/`session`).
fn abrir_tiendas_schedule() -> Result<(Arc<PersistenciaSqlite>, Uuid), HarnessError> {
    run::abrir_tiendas_durables().map_err(HarnessError::Persistencia)
}

/// Filtro puro de vencimiento: pendientes con próxima pasada (las
/// desprogramadas —`proxima` ausente—, las `manual` (solo `run <id>`
/// explícito, nunca vencen solas) y las canceladas/completadas no tocan).
fn seleccionar_vencidas(
    tareas: &[TareaProgramada],
    ahora: DateTime<Utc>,
) -> Vec<TareaProgramadaPendiente> {
    tareas
        .iter()
        .filter(|t| {
            t.estado == "pendiente"
                && t.tipo != "manual"
                && t.proxima_ejecucion.map(|p| p <= ahora).unwrap_or(false)
        })
        .map(|t| TareaProgramadaPendiente {
            id: t.id,
            user_id: t.user_id,
            nombre: t.nombre.clone(),
            prompt: t.prompt.clone(),
            tipo: t.tipo.clone(),
            cron_expr: t.cron_expr.clone(),
            programacion: if t.programacion.is_empty() {
                None
            } else {
                Some(t.programacion.clone())
            },
        })
        .collect()
}

pub(crate) fn cmd_schedule(args: &[String]) -> ExitCode {
    let Some(accion) = args.first().map(String::as_str) else {
        eprintln!("uso: glory-harness schedule <list|create|remove|logs|run>");
        return ExitCode::from(2);
    };
    let (tiendas, user_id) = match abrir_tiendas_schedule() {
        Ok(par) => par,
        Err(e) => {
            eprintln!("glory-harness schedule: {e}");
            return ExitCode::from(1);
        }
    };

    let resultado = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt.block_on(cmd_schedule_impl(accion, args, &tiendas, user_id)),
        Err(e) => {
            eprintln!("glory-harness schedule: no se pudo iniciar el runtime tokio: {e}");
            return ExitCode::from(1);
        }
    };
    match resultado {
        Ok(SalidaSchedule::Ok) => ExitCode::SUCCESS,
        Ok(SalidaSchedule::Uso) => ExitCode::from(2),
        Err(e) => {
            eprintln!("glory-harness schedule: {e}");
            ExitCode::from(1)
        }
    }
}

/// Cuerpo asíncrono del subcomando (extraído a `async fn` porque un bloque
/// `async move` en línea no admite anotación de tipo de retorno).
async fn cmd_schedule_impl(
    accion: &str,
    args: &[String],
    tiendas: &Arc<PersistenciaSqlite>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let programador: Arc<dyn ProgramadorTareas> = tiendas.clone();
    match accion {
        "list" | "listar" => accion_listar(programador, user_id).await,
        "create" | "crear" => accion_crear(args, programador, user_id).await,
        "remove" | "cancelar" => accion_remove(args, programador, user_id).await,
        "logs" => accion_logs(args, programador, user_id).await,
        "run" | "ejecutar" => accion_run(args, tiendas, user_id).await,
        otra => {
            eprintln!("schedule: acción desconocida '{otra}' (list|create|remove|logs|run)");
            Ok(SalidaSchedule::Uso)
        }
    }
}

/// `schedule run [--limite N]` o `schedule run <id>`: ejecuta las tareas
/// vencidas como turnos del agente y entrega el resumen en `tarea_logs`
/// ([B3-F8a]). Construye el harness sobre la misma BD (sin MCP en v1:
/// registry base; documentado como límite). Sin claves LLM los turnos fallan
/// con error presentable y la tarea queda pendiente con el fallo registrado
/// (reintento visible).
///
/// [119A-6 F2] `run <id>` ejecuta ESA tarea aunque no esté vencida (vía
/// explícita para `manual`, que nunca vence sola, y para forzar una
/// `una_vez`/recurrente). Sin id, corre la pasada de vencidas de siempre.
async fn accion_run(
    args: &[String],
    tiendas: &Arc<PersistenciaSqlite>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let persistencia: Arc<dyn AgentPersistence> = tiendas.clone();
    let programador: Arc<dyn ProgramadorTareas> = tiendas.clone();
    // Recupera ejecuciones interrumpidas (proceso muerto a mitad de turno)
    // antes de seleccionar: sin esto una tarea 'ejecutando' huérfana no
    // vuelve a tocar nunca (hermes `recover_abandoned`).
    let recuperadas = persistencia.tareas_recuperar_interrumpidas().await?;
    if recuperadas > 0 {
        println!("schedule run: {recuperadas} interrumpida(s) recuperada(s)");
    }
    let todas = programador.tareas_listar(user_id).await?;
    let id_explicito = match id_opt_desde_args(args) {
        Ok(id) => id,
        Err(salida) => return Ok(salida),
    };
    let vencidas: Vec<TareaProgramadaPendiente> = match id_explicito {
        Some(id) => {
            let Some(t) = todas.iter().find(|t| t.id == id) else {
                eprintln!("schedule run: no existe la tarea {id}");
                return Ok(SalidaSchedule::Uso);
            };
            if t.estado != "pendiente" {
                eprintln!(
                    "schedule run: la tarea {id} está '{}' (solo las pendientes se ejecutan)",
                    t.estado
                );
                return Ok(SalidaSchedule::Uso);
            }
            vec![TareaProgramadaPendiente {
                id: t.id,
                user_id: t.user_id,
                nombre: t.nombre.clone(),
                prompt: t.prompt.clone(),
                tipo: t.tipo.clone(),
                cron_expr: t.cron_expr.clone(),
                programacion: if t.programacion.is_empty() {
                    None
                } else {
                    Some(t.programacion.clone())
                },
            }]
        }
        None => {
            let limite = extraer_opcion(args, &["--limite", "--limit"])
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(10)
                .clamp(1, 100);
            let mut vencidas = seleccionar_vencidas(&todas, Utc::now());
            vencidas.truncate(limite as usize);
            vencidas
        }
    };
    if vencidas.is_empty() {
        println!("(sin tareas vencidas)");
        return Ok(SalidaSchedule::Ok);
    }
    let opciones = run::OpcionesRun {
        provider: extraer_opcion(args, &["--provider", "--proveedor"]),
        modelo: extraer_opcion(args, &["--modelo", "--model"]),
        dir: None,
        modo: None,
        razonamiento: None,
        max_ventana: None,
        gancho_pre_compact: leer_gancho_pre_compact(tiendas)
            .map_err(HarnessError::Persistencia)?,
        /* [069A-3] `schedule run` es desatendido (sin usuario ante la
         * consola): sin avisos aunque el flag exista en otro subcomando. */
        notificar: false,
        navegador: None,
    };
    let harness = run::construir_harness_con(
        &opciones,
        persistencia.clone(),
        programador.clone(),
        user_id,
    );
    for v in &vencidas {
        eprintln!("schedule run: ejecutando {} [{}] …", v.id, v.nombre);
    }
    let resumen = glory_harness_core::cron::ejecutar_lista(
        &harness.runtime,
        &persistencia,
        &programador,
        &vencidas,
    )
    .await?;
    println!(
        "schedule run: vencidas={} ejecutadas={} fallidas={} omitidas={} reprogramadas={}",
        resumen.pendientes,
        resumen.ejecutadas,
        resumen.fallidas,
        resumen.omitidas,
        resumen.reprogramadas,
    );
    Ok(SalidaSchedule::Ok)
}

/// `schedule list`: imprime las tareas programadas del usuario estable.
async fn accion_listar(
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let tareas = programador.tareas_listar(user_id).await?;
    if tareas.is_empty() {
        println!("(sin tareas programadas)");
        return Ok(SalidaSchedule::Ok);
    }
    for t in &tareas {
        let proxima = t
            .proxima_ejecucion
            .map(|p| p.format("%Y-%m-%d %H:%M UTC").to_string())
            .unwrap_or_else(|| "—".into());
        /* [119A-6 F2] La programación canónica manda; en filas legacy (sin
         * ella) se muestra el espejo `cron_expr` heredado. La política F3 se
         * muestra (dormida: no actúa hasta F3). */
        let programa = if t.programacion.is_empty() {
            format!("cron='{}'", t.cron_expr.as_deref().unwrap_or("—"))
        } else {
            format!(
                "programacion='{}' zona={} aviso={} reintentos={}",
                t.programacion, t.zona_horaria, t.notificacion, t.reintentos
            )
        };
        println!(
            "{} [{}] {programa} próximo='{proxima}' estado={} \"{}\"",
            t.id, t.nombre, t.estado, t.prompt
        );
    }
    Ok(SalidaSchedule::Ok)
}

/// `schedule create`: valida argumentos, resuelve la programación a un
/// `ScheduleTarea` canónico tz-aware y registra la tarea.
///
/// Formas (en orden de precedencia):
/// - `--programacion "diario:0 9@Europe/Madrid"`: texto canónico directo
///   (clases: manual, una_vez, intervalo, diario, entre_semana, semanal,
///   cron; la zona viaja dentro del texto).
/// - `--en <RFC3339>`: una sola vez en ese instante (+ `--zona` opcional).
/// - `--cuando "<NL>"`: lenguaje natural heredado (`frase_a_cron`) +
///   `--zona` opcional (defecto UTC).
async fn accion_crear(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    use glory_harness_core::ports::NuevaTareaProgramada;
    use glory_harness_core::schedule::ScheduleTarea;
    use glory_harness_core::tareas::frase_a_cron;

    let nombre = extraer_opcion(args, &["--nombre", "--name"]);
    let prompt = extraer_opcion(args, &["--prompt", "--mensaje"]);
    let (Some(nombre), Some(prompt)) = (nombre, prompt) else {
        eprintln!("uso: glory-harness schedule create --nombre <n> --prompt <p> (--programacion <canónico> | --en <RFC3339> | --cuando \"cada lunes a las 9\") [--zona <IANA>]");
        return Ok(SalidaSchedule::Uso);
    };
    let zona = extraer_opcion(args, &["--zona", "--zone"]).unwrap_or_else(|| "UTC".into());
    let schedule: ScheduleTarea =
        if let Some(texto) = extraer_opcion(args, &["--programacion"]) {
            match ScheduleTarea::parse(&texto) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("schedule create: {e}");
                    return Ok(SalidaSchedule::Uso);
                }
            }
        } else if let Some(instante) = extraer_opcion(args, &["--en", "--at"]) {
            match ScheduleTarea::nueva(
                glory_harness_core::schedule::ClaseTarea::UnaVez,
                &instante,
                &zona,
            ) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("schedule create: {e}");
                    return Ok(SalidaSchedule::Uso);
                }
            }
        } else if let Some(cuando) = extraer_opcion(args, &["--cuando", "--cron"]) {
            let cron = match frase_a_cron(&cuando) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("schedule create: {e}");
                    return Ok(SalidaSchedule::Uso);
                }
            };
            let clase = if cron.starts_with("cada") {
                glory_harness_core::schedule::ClaseTarea::Intervalo
            } else {
                glory_harness_core::schedule::ClaseTarea::Cron
            };
            match ScheduleTarea::nueva(clase, &cron, &zona) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("schedule create: {e}");
                    return Ok(SalidaSchedule::Uso);
                }
            }
        } else {
            eprintln!("uso: glory-harness schedule create --nombre <n> --prompt <p> (--programacion <canónico> | --en <RFC3339> | --cuando \"cada lunes a las 9\") [--zona <IANA>]");
            return Ok(SalidaSchedule::Uso);
        };
    /* Espejo legible heredado: `manual`→"manual", `una_vez`→"una_vez", el
     * resto→"recurrente". `manual` nunca vence sola (ver
     * `seleccionar_vencidas`); se guarda `ahora` como próxima placeholder. */
    let tipo = if schedule.clase == glory_harness_core::schedule::ClaseTarea::Manual {
        "manual"
    } else if schedule.clase == glory_harness_core::schedule::ClaseTarea::UnaVez {
        "una_vez"
    } else {
        "recurrente"
    };
    let proxima = match schedule.proxima(chrono::Utc::now()) {
        Ok(Some(p)) => p,
        Ok(None) => chrono::Utc::now(),
        Err(e) => {
            eprintln!("schedule create: {e}");
            return Ok(SalidaSchedule::Uso);
        }
    };
    let id = programador
        .tarea_crear(&NuevaTareaProgramada {
            user_id,
            nombre,
            prompt,
            tipo: tipo.into(),
            cron_expr: schedule.expresion.clone(),
            programacion: schedule.texto(),
            zona_horaria: schedule.zona_horaria.name().into(),
            proxima_ejecucion: proxima,
            notificacion: None,
            reintentos: None,
        })
        .await?;
    println!(
        "tarea creada: {id} — programacion '{}' — próxima ejecución {}",
        schedule.texto(),
        proxima.format("%Y-%m-%d %H:%M UTC")
    );
    Ok(SalidaSchedule::Ok)
}

/// Lee el id de tarea de `args` (posicional o `--id`) o imprime el uso.
fn id_desde_args(args: &[String], comando: &str) -> Result<Uuid, SalidaSchedule> {
    let id_texto: String = match args.get(1) {
        Some(v) => v.clone(),
        None => match extraer_opcion(args, &["--id"]) {
            Some(v) => v,
            None => {
                eprintln!("uso: glory-harness schedule {comando} <id>");
                return Err(SalidaSchedule::Uso);
            }
        },
    };
    match Uuid::parse_str(&id_texto) {
        Ok(id) => Ok(id),
        Err(_) => {
            eprintln!("schedule {comando}: id inválido '{id_texto}'");
            Err(SalidaSchedule::Uso)
        }
    }
}

/// Id opcional para `schedule run [<id>]`: `Ok(Some)` si el posicional o
/// `--id` parsea como UUID; `Ok(None)` si no hay id (pasada de vencidas);
/// `Err(Uso)` si hay un posicional no-flag que no es UUID (typo visible, no
/// pasada silenciosa).
fn id_opt_desde_args(args: &[String]) -> Result<Option<Uuid>, SalidaSchedule> {
    if let Some(v) = args.get(1) {
        if !v.starts_with("--") {
            match Uuid::parse_str(v) {
                Ok(id) => return Ok(Some(id)),
                Err(_) => {
                    eprintln!("schedule run: id inválido '{v}' (o usa --limite N)");
                    return Err(SalidaSchedule::Uso);
                }
            }
        }
    }
    Ok(extraer_opcion(args, &["--id"]).and_then(|v| Uuid::parse_str(&v).ok()))
}

/// `schedule remove`: cancela la tarea indicada.
async fn accion_remove(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let id = match id_desde_args(args, "remove") {
        Ok(id) => id,
        Err(salida) => return Ok(salida),
    };
    if programador.tarea_cancelar(id, user_id).await? {
        println!("tarea {id} cancelada");
        Ok(SalidaSchedule::Ok)
    } else {
        eprintln!("schedule remove: no existe la tarea {id}");
        Ok(SalidaSchedule::Uso)
    }
}

/// `schedule logs`: imprime las últimas ejecuciones de la tarea indicada.
async fn accion_logs(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let id = match id_desde_args(args, "logs") {
        Ok(id) => id,
        Err(salida) => return Ok(salida),
    };
    let registros = programador.tarea_logs(id, user_id, 10).await?;
    if registros.is_empty() {
        println!("(la tarea {id} no tiene ejecuciones registradas)");
        return Ok(SalidaSchedule::Ok);
    }
    for r in &registros {
        let estado = if r.ok { "ok" } else { "fallo" };
        println!(
            "{} [{estado}] {}",
            r.ejecutada_en.format("%Y-%m-%d %H:%M UTC"),
            r.resumen
        );
    }
    Ok(SalidaSchedule::Ok)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tarea(estado: &str, proxima: Option<DateTime<Utc>>) -> TareaProgramada {
        TareaProgramada {
            id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            nombre: "t".into(),
            prompt: "p".into(),
            tipo: "recurrente".into(),
            cron_expr: Some("0 9 * * *".into()),
            programacion: "cron:0 9 * * *@UTC".into(),
            zona_horaria: "UTC".into(),
            proxima_ejecucion: proxima,
            estado: estado.into(),
            creado_en: Utc::now(),
            notificacion: "fallos".into(),
            reintentos: 0,
        }
    }

    #[test]
    fn vencidas_solo_pendientes_con_proxima_pasada() {
        let ahora = Utc::now();
        let pasado = ahora - chrono::Duration::hours(1);
        let futuro = ahora + chrono::Duration::hours(1);
        let mut manual = tarea("pendiente", Some(pasado));
        manual.tipo = "manual".into();
        manual.programacion = "manual@UTC".into();
        let tareas = vec![
            tarea("pendiente", Some(pasado)),
            tarea("pendiente", Some(futuro)),
            tarea("pendiente", None),
            tarea("cancelada", Some(pasado)),
            tarea("completada", Some(pasado)),
            manual,
        ];
        let vencidas = seleccionar_vencidas(&tareas, ahora);
        assert_eq!(vencidas.len(), 1, "solo la pendiente vencida toca");
        assert_eq!(vencidas[0].id, tareas[0].id);
        assert_eq!(
            vencidas[0].programacion.as_deref(),
            Some("cron:0 9 * * *@UTC"),
            "la programacion canónica viaja al ejecutor"
        );
    }

    #[test]
    fn id_opt_solo_uuid_posicional_o_flag() {
        let id = Uuid::new_v4();
        assert_eq!(
            id_opt_desde_args(&["run".into(), id.to_string()])
                .expect("uuid válido"),
            Some(id)
        );
        assert_eq!(
            id_opt_desde_args(&["run".into(), "--limite".into(), "5".into()])
                .expect("flags no son id"),
            None
        );
        assert!(
            id_opt_desde_args(&["run".into(), "no-es-uuid".into()]).is_err(),
            "un posicional no-UUID es error de uso, no pasada silenciosa"
        );
    }
}
