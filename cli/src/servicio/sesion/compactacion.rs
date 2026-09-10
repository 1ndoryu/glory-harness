// Compactación por demanda del servicio ([109A-4 F3]).
//
// Vive aquí porque crece con el flujo de compactación (resumen persistido y su
// relectura) y no con la sesión: `sesion.rs` mantiene la orquestación del turno
// y este módulo el punto de compactación que hace que el siguiente turno
// arranque del resumen. Los mensajes nunca se borran.

use chrono::{DateTime, SecondsFormat, Utc};
use glory_harness_core::runtime::CompactarManual;
/* El trait trae `listar_mensajes`: la persistencia se usa por su interfaz, no
 * por el tipo SQLite concreto. */
use glory_harness_core::AgentPersistence;
use uuid::Uuid;

use super::{historial_desde_persistencia, Error, SesionComun};

impl SesionComun {
    /// [109A-4 F3] Compactación pedida por el usuario (`/compactar`).
    ///
    /// El historial se reconstruye desde la persistencia (igual que un turno) y
    /// se compacta con el runtime de la sesión: mismo gestor de contexto y
    /// mismos ganchos `PreCompact`/`PostCompact` que la pasada automática. Si
    /// compacta, el resumen queda PERSISTIDO como punto de compactación de la
    /// conversación y los turnos siguientes arrancan de él; los mensajes no se
    /// borran (historial visible y rewind intactos). Si no hay material, no se
    /// escribe nada y el resultado lo explica con `motivo`.
    pub async fn compactar_conversacion(
        &self,
        conversacion_id: Uuid,
        instruccion: Option<String>,
    ) -> Result<CompactarManual, Error> {
        let mensajes = self
            .persistencia
            .listar_mensajes(conversacion_id)
            .await
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let historial = historial_desde_persistencia(mensajes);
        let instruccion = instruccion
            .map(|texto| texto.trim().to_owned())
            .filter(|texto| !texto.is_empty());
        let resultado = self
            .runtime
            .compactar_manual(&historial, instruccion.as_deref())
            .await;
        if let Some(resumen) = resultado.resumen.as_deref() {
            let guardado = self
                .persistencia
                .conversacion_compactar(
                    self.user_id,
                    conversacion_id,
                    /* Misma precisión que `PersistenciaSqlite` usa para los
                     * mensajes: comparar nano segundos contra segundos haría
                     * perder los mensajes del mismo segundo. */
                    &Utc::now().to_rfc3339_opts(SecondsFormat::Secs, true),
                    resumen,
                )
                .map_err(|e| Error::Persistencia(e.to_string()))?;
            if !guardado {
                return Err(Error::Sesion(
                    "la conversación no existe o no es del usuario".into(),
                ));
            }
        }
        Ok(resultado)
    }

    /// [109A-4 F3] Punto de compactación vigente, resuelto a (instante,
    /// resumen). Una marca de tiempo ilegible se ignora con aviso en vez de
    /// romper el turno: enviar el historial completo es la degradación segura.
    pub(super) fn punto_de_compactacion(
        &self,
        conversacion_id: Uuid,
    ) -> Result<Option<(DateTime<Utc>, String)>, Error> {
        let punto = self
            .persistencia
            .conversacion_compactacion(self.user_id, conversacion_id)
            .map_err(|e| Error::Persistencia(e.to_string()))?;
        let Some(punto) = punto else {
            return Ok(None);
        };
        match DateTime::parse_from_rfc3339(&punto.compactado_en) {
            Ok(cuando) => Ok(Some((cuando.with_timezone(&Utc), punto.resumen))),
            Err(e) => {
                tracing::warn!(error = %e, "compactación con fecha inválida; se envía el historial completo");
                Ok(None)
            }
        }
    }
}
