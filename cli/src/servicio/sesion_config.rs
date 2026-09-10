//! Resolución de configuración persistida para construir una sesión.

use std::path::PathBuf;

use glory_harness_core::hooks::ComandoGancho;

use crate::{OpcionesRun, PersistenciaSqlite, VENTANA_MINIMA};

use super::sesion::{Error, OpcionesSesion};

const CLAVE_GANCHO_PRE_COMPACT: &str = "gancho_pre_compact";
const VENTANA_DEFAULT: u32 = 150_000;

pub(super) fn resolver_opciones(
    persistencia: &PersistenciaSqlite,
    opciones: &OpcionesSesion,
) -> Result<OpcionesRun, Error> {
    let leer = |clave| {
        persistencia
            .config_leer(clave)
            .map_err(|e| Error::Persistencia(e.to_string()))
    };
    let dir = match opciones.dir.clone() {
        Some(dir) => Some(dir),
        None => leer("workspace")?.filter(|d| !d.trim().is_empty()),
    };
    let provider = match opciones.provider.clone() {
        Some(p) => Some(p),
        None => leer("proveedor")?,
    };
    let modelo = match opciones.modelo.clone() {
        Some(m) => Some(m),
        None => leer("modelo")?,
    };
    let modo = match opciones.modo.clone() {
        Some(m) => Some(m),
        None => leer("modo")?,
    };
    let razonamiento = match opciones.razonamiento.clone() {
        Some(valor) => Some(valor),
        None => leer("nivelRazonamiento")?,
    };
    Ok(OpcionesRun {
        provider,
        modelo,
        dir: dir.map(PathBuf::from),
        modo,
        razonamiento,
        max_ventana: leer_max_ventana(persistencia)?,
        gancho_pre_compact: leer_gancho_pre_compact(persistencia)
            .map_err(Error::Persistencia)?,
        notificar: false,
        navegador: opciones.navegador.clone(),
    })
}

pub(super) fn leer_max_ventana(persistencia: &PersistenciaSqlite) -> Result<Option<u32>, Error> {
    match persistencia
        .config_leer("contexto_max_ventana")
        .map_err(|e| Error::Persistencia(e.to_string()))?
    {
        Some(txt) => match txt.trim().parse::<u32>() {
            Ok(v) if v >= VENTANA_MINIMA => Ok(Some(v)),
            _ => Ok(Some(VENTANA_DEFAULT)),
        },
        None => Ok(Some(VENTANA_DEFAULT)),
    }
}

pub fn leer_gancho_pre_compact(
    persistencia: &PersistenciaSqlite,
) -> Result<Option<ComandoGancho>, String> {
    let Some(raw) = persistencia
        .config_leer(CLAVE_GANCHO_PRE_COMPACT)
        .map_err(|e| e.to_string())?
    else {
        return Ok(None);
    };
    match serde_json::from_str::<ComandoGancho>(&raw) {
        Ok(config) if !config.comando.trim().is_empty() => Ok(Some(config)),
        Ok(_) => {
            tracing::warn!(
                clave = CLAVE_GANCHO_PRE_COMPACT,
                "gancho pre-compact vacío; se ignora"
            );
            Ok(None)
        }
        Err(error) => {
            tracing::warn!(
                clave = CLAVE_GANCHO_PRE_COMPACT,
                %error,
                "gancho pre-compact inválido; se ignora"
            );
            Ok(None)
        }
    }
}
