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
use glory_harness_core::ports::{TareaProgramada, TareaProgramadaPendiente};
use glory_harness_core::{AgentPersistence, HarnessError, ProgramadorTareas};
use uuid::Uuid;

use crate::extraer_opcion;

/// Resultado del subcomando `schedule`: `Uso` = error de argumentos (exit 2).
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
/// desprogramadas —`proxima` ausente— y las canceladas/completadas no tocan).
fn seleccionar_vencidas(
    tareas: &[TareaProgramada],
    ahora: DateTime<Utc>,
) -> Vec<TareaProgramadaPendiente> {
    tareas
        .iter()
        .filter(|t| {
            t.estado == "pendiente" && t.proxima_ejecucion.map(|p| p <= ahora).unwrap_or(false)
        })
        .map(|t| TareaProgramadaPendiente {
            id: t.id,
            user_id: t.user_id,
            nombre: t.nombre.clone(),
            prompt: t.prompt.clone(),
            tipo: t.tipo.clone(),
            cron_expr: t.cron_expr.clone(),
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

/// `schedule run [--limite N]`: ejecuta las tareas vencidas como turnos del
/// agente y entrega el resumen en `tarea_logs` ([B3-F8a]). Construye el
/// harness sobre la misma BD (sin MCP en v1: registry base; documentado como
/// límite). Sin claves LLM los turnos fallan con error presentable y la
/// tarea queda pendiente con el fallo registrado (reintento visible).
async fn accion_run(
    args: &[String],
    tiendas: &Arc<PersistenciaSqlite>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    let limite = extraer_opcion(args, &["--limite", "--limit"])
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(10)
        .clamp(1, 100);
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
    let mut vencidas = seleccionar_vencidas(&todas, Utc::now());
    vencidas.truncate(limite as usize);
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
        println!(
            "{} [{}] cron='{}' próximo='{proxima}' estado={} \"{}\"",
            t.id,
            t.nombre,
            t.cron_expr.as_deref().unwrap_or("—"),
            t.estado,
            t.prompt
        );
    }
    Ok(SalidaSchedule::Ok)
}

/// `schedule create`: valida argumentos, traduce NL→cron y registra la tarea.
async fn accion_crear(
    args: &[String],
    programador: Arc<dyn ProgramadorTareas>,
    user_id: Uuid,
) -> Result<SalidaSchedule, HarnessError> {
    use glory_harness_core::ports::NuevaTareaProgramada;
    use glory_harness_core::tareas::frase_a_cron;

    let nombre = extraer_opcion(args, &["--nombre", "--name"]);
    let prompt = extraer_opcion(args, &["--prompt", "--mensaje"]);
    let cuando = extraer_opcion(args, &["--cuando", "--cron", "--programacion"]);
    let (Some(nombre), Some(prompt), Some(cuando)) = (nombre, prompt, cuando) else {
        eprintln!("uso: glory-harness schedule create --nombre <n> --prompt <p> --cuando \"cada lunes a las 9\"");
        return Ok(SalidaSchedule::Uso);
    };
    let cron = match frase_a_cron(&cuando) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("schedule create: {e}");
            return Ok(SalidaSchedule::Uso);
        }
    };
    let proxima = glory_harness_core::scheduler::proxima_ejecucion(&cron, chrono::Utc::now())?;
    let id = programador
        .tarea_crear(&NuevaTareaProgramada {
            user_id,
            nombre,
            prompt,
            tipo: "recurrente".into(),
            cron_expr: cron.clone(),
            proxima_ejecucion: proxima,
        })
        .await?;
    println!(
        "tarea creada: {id} — cron '{cron}' — próxima ejecución {}",
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
            proxima_ejecucion: proxima,
            estado: estado.into(),
            creado_en: Utc::now(),
        }
    }

    #[test]
    fn vencidas_solo_pendientes_con_proxima_pasada() {
        let ahora = Utc::now();
        let pasado = ahora - chrono::Duration::hours(1);
        let futuro = ahora + chrono::Duration::hours(1);
        let tareas = vec![
            tarea("pendiente", Some(pasado)),
            tarea("pendiente", Some(futuro)),
            tarea("pendiente", None),
            tarea("cancelada", Some(pasado)),
            tarea("completada", Some(pasado)),
        ];
        let vencidas = seleccionar_vencidas(&tareas, ahora);
        assert_eq!(vencidas.len(), 1, "solo la pendiente vencida toca");
        assert_eq!(vencidas[0].id, tareas[0].id);
    }
}
