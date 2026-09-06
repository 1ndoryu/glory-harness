//! [059A-S3] Auditoría y telemetría del turno: persistencia vía el puerto
//! `AgentPersistence` (R3: nunca SQL en el núcleo) y el evento de cierre
//! con agregados F0 antes de `Done`.

use super::*;

impl AgentRuntime {
    /// [059A-S3] Auditoría del turno (R3: siempre por el puerto, nunca SQL
    /// propio): guarda el registro del turno con estado "ok". Los 8
    /// parámetros son el contexto de sesión que el orquestador ya posee y que
    /// `TurnoPersistido` exige; agruparlos en un struct intermedio solo
    /// trasladaría el problema al llamador.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn persistir_turno(
        &self,
        estado: &EstadoTurno,
        user_id: Uuid,
        turno_id: Uuid,
        conversacion_id: Uuid,
        mensaje_usuario: &str,
        tokens_prompt_total: u32,
        tokens_complecion_total: u32,
        inicio: std::time::Instant,
    ) -> Result<()> {
        self.puertos
            .persistencia
            .guardar_turno(&TurnoPersistido {
                id: turno_id,
                conversacion_id,
                user_id,
                estado: "ok".into(),
                resumen: Some(mensajes_usuario_resumen(mensaje_usuario)),
                creado_en: chrono::Utc::now(),
                provider: Some(self.turno_config.provider.clone()),
                modelo: Some(self.turno_config.modelo.clone()),
                tokens_prompt: tokens_prompt_total,
                tokens_complecion: tokens_complecion_total,
                tools_ejecutadas: estado.tools_ejecutadas as u32,
                duracion_ms: inicio.elapsed().as_millis() as u64,
                error: None,
            })
            .await
    }

    /// [059A-S3] Persiste la respuesta del asistente y toca `actualizado_en` de
    /// la conversación (solo si hubo texto y la conversación es real; las
    /// tareas programadas pasan `conversacion_id = nil`).
    pub(crate) async fn persistir_respuesta_final(
        &self,
        estado: &EstadoTurno,
        conversacion_id: Uuid,
    ) -> Result<()> {
        if let Some(respuesta) = &estado.respuesta_final {
            if conversacion_id != Uuid::nil() {
                self.puertos
                    .persistencia
                    .guardar_mensaje(&MensajePersistido {
                        id: Uuid::new_v4(),
                        conversacion_id,
                        rol: "assistant".into(),
                        contenido: respuesta.clone(),
                        creado_en: chrono::Utc::now(),
                    })
                    .await?;
                self.puertos
                    .persistencia
                    .conversacion_tocar(conversacion_id)
                    .await?;
            }
        }
        Ok(())
    }

    /// [059A-S3] Telemetría del turno (no invasiva): emite el agregado con el
    /// motivo de cierre justo antes de `Done` y resetea el acumulador.
    pub(crate) async fn emitir_telemetria_y_done(
        &self,
        estado: &EstadoTurno,
        conversacion_id: Uuid,
        turno_id: Uuid,
        tx: &Sender<AgenteEvento>,
    ) {
        let compactaciones = self.contexto.lock().await.compactaciones();
        let motivo = motivo_cierre(
            estado.respuesta_final.is_some(),
            estado.respuesta_final.is_none() && !tx.is_closed(),
            tx.is_closed(),
        );
        let evento = {
            /* [318A-15 F0] El guard de la telemetría no debe cruzar un await (el
             * runtime exige futures Send): se construye y resetea el acumulador
             * en un bloque propio y se envía fuera. */
            let mut acumulador = self.telemetria();
            let e = construir_evento(conversacion_id, motivo, compactaciones, &acumulador);
            *acumulador = TelemetriaTurno::nuevo();
            e
        };
        let _ = tx.send(evento).await;
        let _ = tx.send(AgenteEvento::Done { turno_id }).await;
    }
}
