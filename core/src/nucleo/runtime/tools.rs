//! [059A-N S2] Split mecánico de `runtime.rs`: capa de llamada LLM y ejecución de
//! tools del bucle principal (`llm_llamada`, `ejecutar_tool`). Movimiento puro.

use super::*;
use crate::nucleo::llm::SalidasVivo;

/// [129A-2] Emisión throttled de texto en vivo hacia `tx` (el `Token` por
/// delta que faltaba: antes el único `Token` salía completo al final).
/// `try_send` nunca bloquea ni cancela: si el canal está lleno el fragmento
/// vuelve al búfer y se reintenta en el siguiente delta; lo que quede sin
/// enviar viaja en la cola del cierre (`gestionar_respuesta_final`), así que
/// `enviados` dice exactamente qué prefijo del contenido total ya salió.
pub(crate) struct EmisorVivo {
    bufer: String,
    ultimo_envio: std::time::Instant,
    /// Bytes del contenido total ya entregados como `Token` en vivo.
    pub(crate) enviados: usize,
}

/// Intervalo mínimo entre eventos `Token` en vivo (~25/s, como hermes).
const INTERVALO_VIVO_MS: u128 = 40;
/// Tamaño que fuerza un envío aunque no haya pasado el intervalo.
const UMBRAL_VIVO_CHARS: usize = 240;

impl EmisorVivo {
    pub(crate) fn nuevo() -> Self {
        Self {
            bufer: String::new(),
            ultimo_envio: std::time::Instant::now(),
            enviados: 0,
        }
    }

    pub(crate) fn empujar(&mut self, fragmento: &str, tx: &Sender<AgenteEvento>) {
        self.bufer.push_str(fragmento);
        if self.bufer.len() >= UMBRAL_VIVO_CHARS
            || self.ultimo_envio.elapsed().as_millis() >= INTERVALO_VIVO_MS
        {
            let texto = std::mem::take(&mut self.bufer);
            let n = texto.len();
            match tx.try_send(AgenteEvento::Token { texto }) {
                Ok(()) => {
                    self.enviados += n;
                    self.ultimo_envio = std::time::Instant::now();
                }
                /* Canal lleno o consumidor ido: el fragmento vuelve al búfer
                 * (vacío tras el `take`) para reintentarlo o enviarlo al
                 * cierre; NUNCA se pierde texto por emitir en vivo. */
                Err(tokio::sync::mpsc::error::TrySendError::Full(valor)) => {
                    /* Canal lleno: el `Token` no enviado vuelve al búfer
                     * (vacío tras el `take`) para reintentarlo o enviarlo al
                     * cierre; NUNCA se pierde texto por emitir en vivo. El
                     * `if let` cubre el único variant posible (siempre se
                     * envía `Token`); otro se soltaría sin más. */
                    if let AgenteEvento::Token { texto } = valor {
                        self.bufer = texto;
                    }
                }
                /* Consumidor ido: se suelta (el cierre también fallará en
                 * silencio con su `let _`; el turno ya terminó para la UI). */
                Err(tokio::sync::mpsc::error::TrySendError::Closed(_)) => {}
            }
        }
    }
}

impl AgentRuntime {
    pub(crate) async fn llm_llamada(
        &self,
        mensajes: &[AiMessage],
        schemas: &[Value],
        on_token: &mut (dyn FnMut(&str) -> bool + Send),
        on_razonamiento: &mut (dyn FnMut(&str) + Send),
        tx: &Sender<AgenteEvento>,
        en_vivo: bool,
    ) -> Result<(Vec<AiToolCall>, String, usize)> {
        /* [129A-2] El turno principal (`en_vivo`) emite el texto como `Token`
         * throttled durante el stream; cierre, wrap-up e hijos van en batch
         * como siempre (`vivo` queda vacío y `enviados` es 0). El tercer
         * elemento dice qué prefijo del contenido total ya salió en vivo para
         * que el cierre solo envíe la cola (sin duplicados). */
        let mut vivo = EmisorVivo::nuevo();
        let mut con_vivo = |fragmento: &str| -> bool {
            if en_vivo {
                vivo.empujar(fragmento, tx);
            }
            on_token(fragmento)
        };
        let salidas = SalidasVivo {
            token: &mut con_vivo,
            razonamiento: on_razonamiento,
        };
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
                salidas,
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
        /* [129A-1] El pensamiento no fluye por `on_token`: se emite una vez,
         * completo, para pintarlo como summary. Sin razonamiento no hay
         * evento y nada cambia en la UI. */
        if !resultado.razonamiento.trim().is_empty() {
            let _ = tx
                .send(AgenteEvento::Razonamiento {
                    texto: resultado.razonamiento.clone(),
                })
                .await;
        }
        Ok((resultado.tool_calls, resultado.razonamiento, vivo.enviados))
    }

    /// [059A-S3] Wrap-up por límite de pasos: una última llamada SIN tools pide
    /// el resumen de cierre (hecho / pendiente / siguiente paso) cuando el
    /// turno agotó `max_turns` sin respuesta final. Vive aquí (no en
    /// `turno/mod.rs`) para no superar el tope de 500 líneas efectivas por
    /// servicio. Si el cierre también queda vacío (proveedor caído), el turno
    /// queda sin respuesta y el consumidor decide reintentar.
    pub(crate) async fn cierre_wrap_up(
        &self,
        estado: &mut super::turno::EstadoTurno,
        tx: &Sender<AgenteEvento>,
    ) -> Result<()> {
        if estado.respuesta_final.is_some() || tx.is_closed() {
            return Ok(());
        }
        let mut mensajes_cierre = estado.mensajes.clone();
        mensajes_cierre.push(AiMessage::texto("system", wrap_up_instruccion()));
        let mut ultimo_contenido = String::new();
        let mut on_token = |texto: &str| -> bool {
            ultimo_contenido.push_str(texto);
            !tx.is_closed()
        };
        /* [129A-2] El cierre va en batch (sin vivo): un `Token` al final. */
        let mut sin_razonamiento_vivo = |_: &str| {};
        let resultado = self
            .llm_llamada(&mensajes_cierre, &[], &mut on_token, &mut sin_razonamiento_vivo, tx, false)
            .await?;
        /* [129A-1] El cierre también razona: se conserva con el resto del
         * turno (el evento en vivo ya salió por `tx`). */
        estado.guardar_razonamiento(resultado.1);
        if resultado.0.is_empty() {
            let _ = tx
                .send(AgenteEvento::Token {
                    texto: ultimo_contenido.clone(),
                })
                .await;
            if !ultimo_contenido.trim().is_empty() {
                estado.respuesta_final = Some(ultimo_contenido);
            }
        }
        Ok(())
    }

    pub(crate) async fn ejecutar_tool(
        &self,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        tx: &Sender<AgenteEvento>,
    ) -> Result<crate::tool::AgentToolResult> {
        /* [Bloque 3, F4] Hook PreToolUse (bloqueable, claurst exit 2): un hook
         * que pide bloquear veta la tool ANTES de lanzarla; el modelo recibe
         * un resultado de tool denegada (nunca se ejecuta nada). Sin hooks
         * configurados es un no-op que no cambia ningún comportamiento. */
        if self
            .disparar_hook(
                EventoHook::PreToolUse,
                serde_json::json!({
                    "tool": call.nombre.clone(),
                    "tool_input": call.argumentos,
                }),
            )
            .await
        {
            self.telemetria().registrar_denegacion();
            return Ok(crate::tool::AgentToolResult {
                ok: false,
                contenido: format!(
                    "[{} BLOQUEADA POR HOOK] Un hook de política vetó esta tool antes de ejecutarla; NO la reintentes.",
                    call.nombre
                ),
                resumen: "bloqueada_por_hook".into(),
                diff: None,
                evento_extra: None,
            });
        }
        let ctx = AgentToolContext {
            user_id,
            /* [109A-2] El ámbito viaja en el ctx para que las tools de memoria
             * no puedan olvidarlo: lo fija el turno, no la tool. */
            ambito_memoria: self.turno_config.ambito_memoria,
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
            navegador: self.puertos.navegador.as_deref(),
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
        self.puertos
            .persistencia
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
        /* [Bloque 3, F4] Hook informativo PostToolUse con el resultado real
         * (éxito o fallo manejado; un Err del runner no llega aquí). */
        let _ = self
            .disparar_hook(
                EventoHook::PostToolUse,
                serde_json::json!({
                    "tool": call.nombre.clone(),
                    "ok": resultado.ok,
                    "resumen": resultado.resumen.clone(),
                }),
            )
            .await;
        Ok(resultado)
    }
}

#[cfg(test)]
mod pruebas_emisor_vivo {
    /* [129A-2 F3] Verificación real de la lógica viva sin red: umbral de
     * tamaño, throttle temporal y rebuffer ante canal lleno (backpressure). */
    use super::*;
    use tokio::sync::mpsc;

    #[tokio::test]
    async fn envia_al_superar_umbral() {
        let (tx, mut rx) = mpsc::channel::<AgenteEvento>(16);
        let mut vivo = EmisorVivo::nuevo();
        vivo.empujar(&"a".repeat(300), &tx);
        assert_eq!(vivo.enviados, 300);
        match rx.recv().await {
            Some(AgenteEvento::Token { texto }) => assert_eq!(texto.len(), 300),
            otro => panic!("se esperaba Token, llegó {otro:?}"),
        }
    }

    #[tokio::test]
    async fn acumula_y_envia_por_intervalo() {
        let (tx, mut rx) = mpsc::channel::<AgenteEvento>(16);
        let mut vivo = EmisorVivo::nuevo();
        vivo.empujar("hola", &tx);
        assert_eq!(vivo.enviados, 0, "fragmento pequeño: aún no debe emitir");
        assert!(rx.try_recv().is_err());
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        vivo.empujar(" mundo", &tx);
        match rx.recv().await {
            Some(AgenteEvento::Token { texto }) => assert_eq!(texto, "hola mundo"),
            otro => panic!("se esperaba Token acumulado, llegó {otro:?}"),
        }
        assert_eq!(vivo.enviados, 10);
    }

    #[tokio::test]
    async fn reintenta_tras_canal_lleno_sin_perder_texto() {
        let (tx, mut rx) = mpsc::channel::<AgenteEvento>(1);
        tx.send(AgenteEvento::Token {
            texto: "ocupante".to_string(),
        })
        .await
        .expect("canal con hueco");
        let mut vivo = EmisorVivo::nuevo();
        vivo.empujar(&"y".repeat(300), &tx);
        assert_eq!(vivo.enviados, 0, "canal lleno: nada emitido");
        assert!(rx.recv().await.is_some(), "drena el ocupante");
        vivo.empujar("z", &tx);
        match rx.recv().await {
            Some(AgenteEvento::Token { texto }) => {
                assert_eq!(texto.len(), 301);
                assert!(texto.ends_with('z'));
            }
            otro => panic!("se esperaba Token reemitido, llegó {otro:?}"),
        }
        assert_eq!(vivo.enviados, 301);
    }
}
