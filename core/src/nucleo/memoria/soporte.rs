//! Soporte de tests de memoria: tienda observable en memoria
//! (el mock de contrato no observa escrituras).
//!
//! Solo existe en compilación de tests (`#[cfg(test)]` en `mod.rs`).

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::ports::{
    AccionAuditable, AgentPersistence, MemoriaEntrada, MensajePersistido, SkillEntrada,
    TareaProgramadaPendiente, TurnoPersistido,
};

#[derive(Default)]
pub(crate) struct TiendaPrueba {
    memoria: Mutex<HashMap<Uuid, HashMap<String, MemoriaEntrada>>>,
    skills: Mutex<HashMap<Uuid, Vec<SkillEntrada>>>,
    /// Simula una tienda sin `skills_registrar` (legacy): la promoción
    /// deja nota en vez de romper la pasada.
    pub(crate) sin_registro: bool,
}

impl TiendaPrueba {
    /// Tienda legacy sin `skills_registrar` (la promoción deja nota).
    pub(crate) fn sin_registro() -> Self {
        Self {
            sin_registro: true,
            ..Self::default()
        }
    }

    pub(crate) fn sembrar(&self, user_id: Uuid, entradas: Vec<MemoriaEntrada>) {
        let mut mapa = self.memoria.lock().unwrap_or_else(|p| p.into_inner());
        let slot = mapa.entry(user_id).or_default();
        for e in entradas {
            slot.insert(e.clave.clone(), e);
        }
    }

    pub(crate) fn leer(&self, user_id: Uuid, clave: &str) -> Option<MemoriaEntrada> {
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
    async fn tarea_reprogramar(&self, _: Uuid, _: Uuid, _: Option<DateTime<Utc>>) -> Result<()> {
        Ok(())
    }
}

pub(crate) fn entrada_vieja(clave: &str, contenido: &str, dias: i64, usos: u32) -> MemoriaEntrada {
    let mut e = MemoriaEntrada::nueva(clave.into(), contenido.into(), "t".into());
    e.actualizada_en = Utc::now() - chrono::Duration::days(dias);
    e.usos = usos;
    e
}
