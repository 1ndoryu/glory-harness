//! Persistencia en memoria para el binario `glory-harness` standalone (Fase 3).
//!
//! El CLI/daemon no tiene base de datos propia: implementa [`AgentPersistence`]
//! sobre estructuras en memoria (`HashMap` tras un `Mutex`). Esto permite usar
//! el runtime del núcleo sin acoplar el binario a SQLx ni a las tablas de task
//! (R5 del plan 318A-13: el núcleo no sabe qué persistencia usa el consumidor).
//!
//! Es una implementación mínima de escucha: conserva el estado mientras el
//! proceso vive, suficiente para `run` one-shot y para el daemon en memoria.
//! Un consumidor real (task) usa su propia implementación con repositorios.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use uuid::Uuid;

use glory_harness_core::ports::{
    AccionAuditable, AmbitoMemoria, LogTareaEjecucion, MemoriaEntrada, MensajePersistido,
    NuevaTareaProgramada, ProgramadorTareas, SkillEntrada, TareaProgramada,
    TareaProgramadaPendiente, TurnoPersistido,
};
use glory_harness_core::{AgentPersistence, HarnessResult};

/// Estado durable de un `AgentPersistence` en memoria.
#[derive(Debug, Default)]
struct Estado {
    turnos: HashMap<Uuid, TurnoPersistido>,
    mensajes: HashMap<Uuid, Vec<MensajePersistido>>,
    acciones: Vec<AccionAuditable>,
    /// [109A-2] Los recuerdos se indexan por `(usuario, ámbito)`: la memoria
    /// de un proyecto nunca se mezcla con la global ni con la de otro.
    memoria: HashMap<(Uuid, AmbitoMemoria), HashMap<String, MemoriaEntrada>>,
    skills: HashMap<Uuid, Vec<SkillEntrada>>,
    tareas: HashMap<Uuid, TareaProgramadaPendiente>,
    tareas_tomadas: std::collections::HashSet<Uuid>,
    conversacion_reciente: HashMap<Uuid, DateTime<Utc>>,
}

/// Implementación en memoria de [`AgentPersistence`]. `Clone` comparte el
/// estado (Arc<Mutex> interno), así que puede vivir en vários puertos a la vez.
#[derive(Debug, Clone, Default)]
pub struct PersistenciaMemoria {
    estado: Arc<Mutex<Estado>>,
}

impl PersistenciaMemoria {
    #[must_use]
    pub fn nuevo() -> Self {
        Self::default()
    }

    /// Carga un conjunto de skills estáticas (para dar contexto útil en el
    /// CLI/daemon sin BD). Sustituye a la tabla `agente_skills` de task.
    pub fn con_skills_base(&self, user_id: Uuid) -> &Self {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.skills.entry(user_id).or_insert_with(|| {
            vec![SkillEntrada {
                id: Uuid::new_v4(),
                nombre: "resumen".into(),
                descripcion: "Resume en 3 viñetas".into(),
                instrucciones: "Al terminar, resume tu respuesta en 3 viñetas concisas.".into(),
                activa: true,
            }]
        });
        self
    }
}

#[async_trait]
impl AgentPersistence for PersistenciaMemoria {
    async fn guardar_turno(&self, turno: &TurnoPersistido) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.turnos.insert(turno.id, turno.clone());
        Ok(())
    }

    async fn finalizar_turno(
        &self,
        turno_id: Uuid,
        estado_final: &str,
        resumen: Option<&str>,
    ) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(turno) = estado.turnos.get_mut(&turno_id) {
            turno.estado = estado_final.to_string();
            turno.resumen = resumen.map(ToString::to_string);
        }
        Ok(())
    }

    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado
            .mensajes
            .entry(mensaje.conversacion_id)
            .or_default()
            .push(mensaje.clone());
        Ok(())
    }

    async fn listar_mensajes(
        &self,
        conversacion_id: Uuid,
    ) -> HarnessResult<Vec<MensajePersistido>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut mensajes = estado
            .mensajes
            .get(&conversacion_id)
            .cloned()
            .unwrap_or_default();
        mensajes.sort_by_key(|m| m.creado_en);
        Ok(mensajes)
    }

    async fn conversacion_tocar(&self, conversacion_id: Uuid) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado
            .conversacion_reciente
            .insert(conversacion_id, Utc::now());
        Ok(())
    }

    async fn registrar_accion(&self, accion: &AccionAuditable) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.acciones.push(accion.clone());
        Ok(())
    }

    async fn memoria_listar(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
    ) -> HarnessResult<Vec<MemoriaEntrada>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado
            .memoria
            .get(&(user_id, ambito))
            .map(|mapa| mapa.values().cloned().collect())
            .unwrap_or_default())
    }

    async fn memoria_upsert(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
        entrada: &MemoriaEntrada,
    ) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado
            .memoria
            .entry((user_id, ambito))
            .or_default()
            .insert(entrada.clave.clone(), entrada.clone());
        Ok(())
    }

    async fn memoria_borrar(
        &self,
        user_id: Uuid,
        ambito: AmbitoMemoria,
        clave: &str,
    ) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mapa) = estado.memoria.get_mut(&(user_id, ambito)) {
            mapa.remove(clave);
        }
        Ok(())
    }

    async fn memoria_ambitos(&self, user_id: Uuid) -> HarnessResult<Vec<AmbitoMemoria>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut ambitos: Vec<AmbitoMemoria> = estado
            .memoria
            .keys()
            .filter(|(u, _)| *u == user_id)
            .map(|(_, a)| *a)
            .collect();
        if !ambitos.contains(&AmbitoMemoria::Global) {
            ambitos.push(AmbitoMemoria::Global);
        }
        ambitos.sort_by_key(|a| (a.proyecto_id().is_some(), a.proyecto_id()));
        Ok(ambitos)
    }

    async fn skills_listar(&self, user_id: Uuid) -> HarnessResult<Vec<SkillEntrada>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado.skills.get(&user_id).cloned().unwrap_or_default())
    }

    async fn skills_registrar(&self, user_id: Uuid, skill: &SkillEntrada) -> HarnessResult<()> {
        // [069A-4] Paridad con la tienda sqlite (alta o sustitución por nombre).
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let lista = estado.skills.entry(user_id).or_default();
        if let Some(previa) = lista.iter_mut().find(|s| s.nombre == skill.nombre) {
            *previa = skill.clone();
        } else {
            lista.push(skill.clone());
        }
        Ok(())
    }

    async fn tareas_recuperar_interrumpidas(&self) -> HarnessResult<u64> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // En memoria no hay heartbeats reales; nada que recuperar.
        estado.tareas_tomadas.clear();
        Ok(0)
    }

    async fn tareas_pendientes(&self, limite: u32) -> HarnessResult<Vec<TareaProgramadaPendiente>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado
            .tareas
            .values()
            .filter(|t| !estado.tareas_tomadas.contains(&t.id))
            .take(limite as usize)
            .cloned()
            .collect())
    }

    async fn tarea_tomar(&self, id: Uuid) -> HarnessResult<bool> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if estado.tareas_tomadas.contains(&id) {
            return Ok(false);
        }
        estado.tareas_tomadas.insert(id);
        Ok(true)
    }

    async fn tarea_finalizar(
        &self,
        id: Uuid,
        ok: bool,
        resumen: Option<&str>,
    ) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.tareas_tomadas.remove(&id);
        if !ok {
            // En memoria, una tarea fallida se reintenta (se deja pendiente).
            estado.tareas_tomadas.remove(&id);
        }
        tracing::debug!(tarea = %id, ok, resumen = resumen.unwrap_or(""), "tarea finalizada (memoria)");
        Ok(())
    }

    async fn tarea_reprogramar(
        &self,
        id: Uuid,
        _user_id: Uuid,
        _proxima: Option<DateTime<Utc>>,
    ) -> HarnessResult<()> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.tareas_tomadas.remove(&id);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// [318A-16 F6] ProgramadorTareas en memoria (subcomando `schedule` + tool
// `programar_tarea` del CLI standalone).
// ---------------------------------------------------------------------------

/// Estado del [`ProgramadorMemoria`]: tareas por usuario + logs por tarea.
#[derive(Debug, Default)]
struct EstadoProgramador {
    tareas: Vec<TareaProgramada>,
    logs: HashMap<Uuid, Vec<LogTareaEjecucion>>,
}

/// Implementación en memoria del puerto [`ProgramadorTareas`] para el binario
/// standalone (misma filosofía que [`PersistenciaMemoria`]: vive mientras el
/// proceso corre). El worker de producción corre en el consumidor (PT ya lo
/// tiene con cron + heartbeat); aquí la cara CRUD existe para `schedule` y
/// para que la tool `programar_tarea` se registre en las sesiones del CLI.
#[derive(Debug, Clone, Default)]
pub struct ProgramadorMemoria {
    estado: Arc<Mutex<EstadoProgramador>>,
}

impl ProgramadorMemoria {
    #[must_use]
    pub fn nuevo() -> Self {
        Self::default()
    }
}

#[async_trait]
impl ProgramadorTareas for ProgramadorMemoria {
    async fn tarea_crear(&self, nueva: &NuevaTareaProgramada) -> HarnessResult<Uuid> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let id = Uuid::new_v4();
        estado.tareas.push(TareaProgramada {
            id,
            user_id: nueva.user_id,
            nombre: nueva.nombre.clone(),
            prompt: nueva.prompt.clone(),
            tipo: nueva.tipo.clone(),
            cron_expr: Some(nueva.cron_expr.clone()),
            proxima_ejecucion: Some(nueva.proxima_ejecucion),
            estado: "pendiente".into(),
            creado_en: Utc::now(),
        });
        Ok(id)
    }

    async fn tareas_listar(&self, user_id: Uuid) -> HarnessResult<Vec<TareaProgramada>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado
            .tareas
            .iter()
            .filter(|t| t.user_id == user_id)
            .cloned()
            .collect())
    }

    async fn tarea_cancelar(&self, id: Uuid, user_id: Uuid) -> HarnessResult<bool> {
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(pos) = estado
            .tareas
            .iter()
            .position(|t| t.id == id && t.user_id == user_id)
        else {
            return Ok(false);
        };
        estado.tareas[pos].estado = "cancelada".into();
        Ok(true)
    }

    async fn tarea_logs(
        &self,
        id: Uuid,
        user_id: Uuid,
        limite: u32,
    ) -> HarnessResult<Vec<LogTareaEjecucion>> {
        let estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let es_suya = estado
            .tareas
            .iter()
            .any(|t| t.id == id && t.user_id == user_id);
        if !es_suya {
            return Ok(Vec::new());
        }
        let mut logs = estado.logs.get(&id).cloned().unwrap_or_default();
        logs.sort_by_key(|l| l.ejecutada_en);
        logs.truncate(limite as usize);
        Ok(logs)
    }

    async fn tarea_registrar_log(
        &self,
        id: Uuid,
        user_id: Uuid,
        ok: bool,
        resumen: &str,
    ) -> HarnessResult<()> {
        // [B3-F8a] Entrega en RAM: misma guarda de ownership que `tarea_logs`
        // (la ajena se ignora sin error para no abortar la pasada).
        let mut estado = self
            .estado
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let es_suya = estado
            .tareas
            .iter()
            .any(|t| t.id == id && t.user_id == user_id);
        if !es_suya {
            return Ok(());
        }
        estado.logs.entry(id).or_default().push(LogTareaEjecucion {
            id: Uuid::new_v4(),
            tarea_id: id,
            ok,
            resumen: resumen.to_string(),
            ejecutada_en: Utc::now(),
        });
        Ok(())
    }
}
