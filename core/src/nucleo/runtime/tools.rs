//! [059A-N S2] Split mecánico de `runtime.rs`: capa de llamada LLM y ejecución de
//! tools del bucle principal (`llm_llamada`, `ejecutar_tool`). Movimiento puro.

use super::*;

impl AgentRuntime {
    pub(crate) async fn llm_llamada(
        &self,
        mensajes: &[AiMessage],
        schemas: &[Value],
        on_token: &mut (dyn FnMut(&str) -> bool + Send),
        tx: &Sender<AgenteEvento>,
    ) -> Result<Vec<AiToolCall>> {
        let resultado = self
            .puertos
            .llm
            .enviar_chat_stream(
                mensajes.to_vec(),
                &self.turno_config.provider,
                &self.turno_config.modelo,
                AiChatOptions {
                    temperature: self.turno_config.temperatura,
                    max_tokens: self.turno_config.max_tokens,
                    reasoning_effort: self.turno_config.nivel_razonamiento.clone(),
                },
                schemas.to_vec(),
                on_token,
            )
            .await?;
        /* [02-09-2026] El resultado del stream lleva el provider/modelo REAL
         * tras resolver la cadena de fallback (enviar_chat_stream devuelve el
         * primer candidato que respondió, no el solicitado). Se propagan en el
         * evento Usage para que el front muestre qué modelo respondió de
         * verdad (puede saltar de commandcode a glory/deepseek, etc.). */
        let _ = tx
            .send(AgenteEvento::Usage {
                tokens_prompt: resultado.tokens_prompt,
                tokens_complecion: resultado.tokens_complecion,
                ocupacion_pct: None,
                provider: Some(resultado.provider.clone()),
                modelo: Some(resultado.modelo.clone()),
            })
            .await;
        Ok(resultado.tool_calls)
    }

    pub(crate) async fn ejecutar_tool(
        &self,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        tx: &Sender<AgenteEvento>,
    ) -> Result<crate::tool::AgentToolResult> {
        let ctx = AgentToolContext {
            user_id,
            persistencia: self.puertos.persistencia.as_ref(),
            web_search: self.puertos.web_search.as_deref(),
            web_fetch: self.puertos.web_fetch.as_deref(),
            /* [318A-10] `ai_provider` queda reservado para tools que generen
             * texto (ninguna agnóstica lo usa hoy); el runtime usa `llm`
             * directo para el loop. El consumidor puede implementar
             * `ProviderPort` sobre su propio servicio si una tool de dominio
             * lo necesita. */
            ai_provider: None,
            sandbox_archivos: self.registry.sandbox(),
            dominio: self.puertos.dominio.as_deref(),
            todo: self.registry.todo(),
            plan: self.plan_actual(),
        };
        let resultado = self
            .registry
            .ejecutar(&call.nombre, &ctx, call.argumentos.clone())
            .await
            .map_err(|error| {
                tracing::warn!(tool = %call.nombre, args = %call.argumentos, %error, "tool del agente falló");
                error
            })?;
        /* Auditoría de acción (sin secretos). */
        self.puertos.persistencia
            .registrar_accion(&AccionAuditable {
                turno_id,
                tool: call.nombre.clone(),
                ok: resultado.ok,
                resumen: resultado.resumen.clone(),
                argumentos_json: Some(call.argumentos.to_string()),
                /* [039A-1 04-09 H6] Se propaga el diff del cambio para que la
                 * UI lo repinte al recargar el historial. */
                diff: resultado.diff.clone(),
            })
            .await?;
        let _ = tx;
        Ok(resultado)
    }
}

