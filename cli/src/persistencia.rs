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
    AccionAuditable, MemoriaEntrada, MensajePersistido, SkillEntrada, TareaProgramadaPendiente,
    TurnoPersistido,
};
use glory_harness_core::{AgentPersistence, HarnessResult};

/// Estado durable de un `AgentPersistence` en memoria.
#[derive(Debug, Default)]
struct Estado {
    turnos: HashMap<Uuid, TurnoPersistido>,
    mensajes: HashMap<Uuid, Vec<MensajePersistido>>,
    acciones: Vec<AccionAuditable>,
    memoria: HashMap<Uuid, HashMap<String, String>>,
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
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.turnos.insert(turno.id, turno.clone());
        Ok(())
    }

    async fn finalizar_turno(
        &self,
        turno_id: Uuid,
        estado_final: &str,
        resumen: Option<&str>,
    ) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(turno) = estado.turnos.get_mut(&turno_id) {
            turno.estado = estado_final.to_string();
            turno.resumen = resumen.map(ToString::to_string);
        }
        Ok(())
    }

    async fn guardar_mensaje(&self, mensaje: &MensajePersistido) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado
            .mensajes
            .entry(mensaje.conversacion_id)
            .or_default()
            .push(mensaje.clone());
        Ok(())
    }

    async fn listar_mensajes(&self, conversacion_id: Uuid) -> HarnessResult<Vec<MensajePersistido>> {
        let estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut mensajes = estado.mensajes.get(&conversacion_id).cloned().unwrap_or_default();
        mensajes.sort_by_key(|m| m.creado_en);
        Ok(mensajes)
    }

    async fn conversacion_tocar(&self, conversacion_id: Uuid) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.conversacion_reciente.insert(conversacion_id, Utc::now());
        Ok(())
    }

    async fn registrar_accion(&self, accion: &AccionAuditable) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.acciones.push(accion.clone());
        Ok(())
    }

    async fn memoria_listar(&self, user_id: Uuid) -> HarnessResult<Vec<MemoriaEntrada>> {
        let estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado
            .memoria
            .get(&user_id)
            .map(|mapa| {
                mapa.iter()
                    .map(|(clave, contenido)| MemoriaEntrada {
                        clave: clave.clone(),
                        contenido: contenido.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    async fn memoria_upsert(&self, user_id: Uuid, entrada: &MemoriaEntrada) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado
            .memoria
            .entry(user_id)
            .or_default()
            .insert(entrada.clave.clone(), entrada.contenido.clone());
        Ok(())
    }

    async fn memoria_borrar(&self, user_id: Uuid, clave: &str) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mapa) = estado.memoria.get_mut(&user_id) {
            mapa.remove(clave);
        }
        Ok(())
    }

    async fn skills_listar(&self, user_id: Uuid) -> HarnessResult<Vec<SkillEntrada>> {
        let estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado.skills.get(&user_id).cloned().unwrap_or_default())
    }

    async fn tareas_recuperar_interrumpidas(&self) -> HarnessResult<u64> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // En memoria no hay heartbeats reales; nada que recuperar.
        estado.tareas_tomadas.clear();
        Ok(0)
    }

    async fn tareas_pendientes(&self, limite: u32) -> HarnessResult<Vec<TareaProgramadaPendiente>> {
        let estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        Ok(estado
            .tareas
            .values()
            .filter(|t| !estado.tareas_tomadas.contains(&t.id))
            .take(limite as usize)
            .cloned()
            .collect())
    }

    async fn tarea_tomar(&self, id: Uuid) -> HarnessResult<bool> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if estado.tareas_tomadas.contains(&id) {
            return Ok(false);
        }
        estado.tareas_tomadas.insert(id);
        Ok(true)
    }

    async fn tarea_finalizar(&self, id: Uuid, ok: bool, resumen: Option<&str>) -> HarnessResult<()> {
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut estado = self.estado.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        estado.tareas_tomadas.remove(&id);
        Ok(())
    }
}