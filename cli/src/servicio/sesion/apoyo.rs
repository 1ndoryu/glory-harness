// Apoyo del servicio de sesión: helpers puros/libres que no tocan el estado
// del turno (identidad estable del usuario, conteo de claves por proveedor y
// título automático). Viven aquí para que `sesion.rs` mantenga la
// orquestación del turno bajo el techo de 500 líneas efectivas.

use glory_harness_core::llm::LlavesProveedor;
use uuid::Uuid;

use super::{Error, ProveedorConteo};
use crate::PersistenciaSqlite;

pub(super) fn usuario_estable(persistencia: &PersistenciaSqlite) -> Result<Uuid, Error> {
    match persistencia
        .config_leer("user_id")
        .map_err(|e| Error::Persistencia(e.to_string()))?
    {
        Some(guardado) => Uuid::parse_str(guardado.trim())
            .map_err(|_| Error::Configuracion("user_id guardado corrupto".into())),
        None => {
            let nuevo = Uuid::new_v4();
            persistencia
                .config_guardar("user_id", &nuevo.to_string())
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            Ok(nuevo)
        }
    }
}

pub(super) fn conteos(llaves: &LlavesProveedor) -> Vec<ProveedorConteo> {
    vec![
        ProveedorConteo {
            nombre: "cerebras".into(),
            claves: llaves.cerebras.len(),
        },
        ProveedorConteo {
            nombre: "groq".into(),
            claves: llaves.groq.len(),
        },
        ProveedorConteo {
            nombre: "deepseek".into(),
            claves: llaves.deepseek.len(),
        },
        ProveedorConteo {
            nombre: "glory".into(),
            claves: llaves.glory.len(),
        },
        ProveedorConteo {
            nombre: "commandcode".into(),
            claves: llaves.commandcode.len(),
        },
    ]
}

/// [039A-1 04-09 H5] Nombre breve de conversación desde el primer mensaje del
/// usuario: primeras ~4 palabras (o ~42 caracteres), una sola línea, sin
/// prefijos de modo (`[META: …]`). Si no hay palabras, "Conversación".
pub(super) fn titulo_auto_desde_mensaje(mensaje: &str) -> String {
    let limpio = mensaje.trim().lines().next().unwrap_or("").trim();
    let sin_meta = limpio
        .strip_prefix("[META:")
        .and_then(|resto| resto.find(']').map(|i| &resto[i + 1..]))
        .unwrap_or(limpio)
        .trim();
    if sin_meta.is_empty() {
        return "Conversación".into();
    }
    let palabras: Vec<&str> = sin_meta.split_whitespace().collect();
    let mut out = String::new();
    for (i, p) in palabras.iter().enumerate() {
        if i == 4 {
            break;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(p);
        if out.chars().count() >= 42 {
            break;
        }
    }
    if out.is_empty() {
        "Conversación".into()
    } else {
        out
    }
}
