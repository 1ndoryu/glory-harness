//! [059A-S2/S3] Bucle del turno: orquestador `ejecutar_turno` y sus
//! helpers de fase (compacción, guardas de finalización, lote de tools,
//! wrap-up). Tipos de estado del turno (`EstadoTurno`, `PasoIteracion`,
//! `PasoTool`) y sub-módulos por responsabilidad:
//!  - `auditoria`: persistencia del turno y telemetría final.
//!  - `permisos`: flujo de permiso/veredicto y ejecución de una tool.

use super::*;

mod auditoria;
mod permisos;

/// [059A-S3] Estado mutable de un turno (extraído de `ejecutar_turno` para
/// acotar las firmas de los helpers de fase).
pub(crate) struct EstadoTurno {
    mensajes: Vec<AiMessage>,
    /* [Bloque 3, F1] Cola de respuestas previas del asistente (del historial
     * del consumidor) para el detector de repetición de las guardas. Solo
     * contenido de texto real; tool_calls/Null no cuentan. */
    respuestas_asistente: Vec<String>,
    /* Un solo reintento por turno tras respuesta vacía. */
    ya_reintentado: bool,
    /* [318A-15 F3] Tools denegadas en este turno (por política o por negación
     * del usuario): si el modelo las vuelve a proponer en el MISMO turno, no se
     * re-emite el evento ni se le vuelve a explicar — se le devuelve
     * "denegada" para que cambie de plan (no reintento automático). */
    denegadas_en_turno: std::collections::HashSet<String>,
    tools_ejecutadas: usize,
    /* [29-08-2026] Persistencia de la conversación (Fase 4): la respuesta
     * final del asistente se guarda al terminar el turno para que recargar
     * conserve el historial completo (el mensaje del usuario lo persiste el
     * consumidor antes de llamar). */
    respuesta_final: Option<String>,
}

impl EstadoTurno {
    /// [059A-S3] Ensambla el arranque del turno: system + historial + mensaje
    /// del usuario, y precarga la cola de respuestas previas del asistente.
    fn nuevo(prompt_sistema: String, historial: Vec<AiMessage>, mensaje_usuario: String) -> Self {
        let mut mensajes: Vec<AiMessage> = Vec::new();
        mensajes.push(AiMessage::texto("system", prompt_sistema));
        let respuestas_asistente: Vec<String> = historial
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| match &m.content {
                Value::String(t) if !t.trim().is_empty() => Some(t.clone()),
                _ => None,
            })
            .collect();
        mensajes.extend(historial);
        mensajes.push(AiMessage::texto("user", mensaje_usuario));
        EstadoTurno {
            mensajes,
            respuestas_asistente,
            ya_reintentado: false,
            denegadas_en_turno: std::collections::HashSet::new(),
            tools_ejecutadas: 0,
            respuesta_final: None,
        }
    }
}

/// [059A-S3] Resultado de un paso del bucle para que `ejecutar_turno` decida
/// el control (continue/break) sin duplicar la lógica de cada fase.
enum PasoIteracion {
    /// Respuesta final del modelo (texto): terminar el turno con normalidad.
    FinalizarTurno,
    /// Respuesta vacía y queda reintento: seguir el bucle con aviso de sistema.
    ReintentarVacio,
    /// La tool `ask_user` cortó el turno (la pregunta queda pendiente).
    TurnoCortadoPorPregunta,
    /// El SSE se cerró: no seguir ejecutando tools ni consumiendo tokens.
    ConexionCerrada,
    /// Hubo tool_calls procesadas: continuar a la siguiente iteración.
    ProcesarTools,
}

/// [059A-S3] Resultado de procesar una tool individual del lote.
pub(crate) enum PasoTool {
    /// Seguir con la siguiente tool del lote.
    Continua,
    /// El SSE se cerró a mitad de la ejecución.
    CerrarConexion,
    /// `ask_user` emitió la pregunta: el turno termina aquí.
    PreguntaUsuario,
}

impl AgentRuntime {
    /// Ejecuta un turno completo del agente: sistema + historial + mensaje del
    /// usuario → loop de tools → respuesta final. Emite eventos al `tx`.
    pub async fn ejecutar_turno(
        &self,
        user_id: Uuid,
        turno_id: Uuid,
        conversacion_id: Uuid,
        historial: Vec<AiMessage>,
        mensaje_usuario: String,
        tx: &Sender<AgenteEvento>,
    ) -> Result<()> {
        let inicio = std::time::Instant::now();
        let mut estado = EstadoTurno::nuevo(
            self.prompt_sistema(),
            historial,
            mensaje_usuario.clone(),
        );
        /* [318A-16 F5] Modo plan: propuesta fresca por turno. El turno
         * anterior dejó su propuesta legible (`plan_actual`); al empezar uno
         * nuevo en modo plan se sustituye (la UI decidió aplicar o descartar
         * entre turnos). Fuera de modo plan, `None`. */
        self.resetear_plan_para_modo();

        let tokens_prompt_total = 0u32;
        let tokens_complecion_total = 0u32;

        'turnos: for _turno in 0..self.turno_config.max_turns {
            /* [01-09-2026] Fase 4: cancelación real — si el cliente cortó el
             * SSE (receiver dropeado), el sender está cerrado y no se sigue
             * ejecutando tools ni consumiendo tokens. */
            if tx.is_closed() {
                break;
            }
            let paso = self
                .un_paso_de_iteracion(
                    &mut estado,
                    user_id,
                    turno_id,
                    tokens_prompt_total,
                    tokens_complecion_total,
                    tx,
                )
                .await?;
            match paso {
                PasoIteracion::FinalizarTurno | PasoIteracion::TurnoCortadoPorPregunta => {
                    break 'turnos;
                }
                PasoIteracion::ReintentarVacio
                | PasoIteracion::ProcesarTools
                | PasoIteracion::ConexionCerrada => {}
            }
        }

        /* [318A-15 F5] Límite de pasos con wrap-up: si el turno agotó
         * `max_turns` sin respuesta final y el cliente sigue conectado, no se
         * corta en seco: una última llamada SIN tools pide el resumen de cierre
         * (hecho / pendiente / siguiente paso). Si el cierre también queda
         * vacío (proveedor caído), el turno queda sin respuesta y el consumidor
         * decide reintentar (mismo contrato que hoy). */
        self.cierre_wrap_up(&mut estado, tx).await?;

        /* Auditoría del turno (R3: siempre por el puerto, nunca SQL propio). */
        self.persistir_turno(
            &estado,
            user_id,
            turno_id,
            conversacion_id,
            &mensaje_usuario,
            tokens_prompt_total,
            tokens_complecion_total,
            inicio,
        )
        .await?;

        /* [29-08-2026] Persistir la respuesta del asistente (si el proveedor
         * devolvió texto) y tocar `actualizado_en` de la conversación. Si no
         * hubo respuesta (fallo retryable), el turno ya quedó como
         * pendiente/fallido y el usuario reintenta: no se escribe nada falso.
         * Las tareas programadas pasan `conversacion_id = nil`: no se persiste
         * nada. */
        self.persistir_respuesta_final(&estado, conversacion_id)
            .await?;

        /* [318A-15 F0] Telemetría del turno (no invasiva): agregados ya
         * observados durante la ejecución, emitidos justo antes de `Done` y
         * reseteados para el siguiente turno. Los turnos fallidos no llegan
         * aquí (emiten `Error` con motivo y retryable). */
        self.emitir_telemetria_y_done(&estado, conversacion_id, turno_id, tx)
            .await;
        Ok(())
    }

    /// [059A-S3] Modo plan: cada turno arranca con una propuesta fresca en
    /// `plan_actual` (o `None` fuera de modo plan).
    fn resetear_plan_para_modo(&self) {
        let mut plan = self.plan_actual.lock().unwrap_or_else(|p| p.into_inner());
        *plan = if self.turno_config.modo == "plan" {
            Some(Arc::new(std::sync::RwLock::new(
                crate::plan::PlanPropuesto::default(),
            )))
        } else {
            None
        };
    }

    /// [059A-S3] Una iteración del bucle de turno: compactar contexto ANTES de
    /// la llamada, pedir el avance al LLM y procesar el resultado (respuesta
    /// final con guardas, o el lote de tools propuestas).
    async fn un_paso_de_iteracion(
        &self,
        estado: &mut EstadoTurno,
        user_id: Uuid,
        turno_id: Uuid,
        tokens_prompt_total: u32,
        tokens_complecion_total: u32,
        tx: &Sender<AgenteEvento>,
    ) -> Result<PasoIteracion> {
        /* Contexto: preparar (compactar si hace falta) ANTES de cada llamada. */
        let actuales = std::mem::take(&mut estado.mensajes);
        estado.mensajes = self.preparar_contexto_iteracion(actuales, tx).await?;

        let mut ids = self.registry.ids();
        if !self.turno_config.permitir_busqueda_web {
            ids.retain(|id| *id != "web_search");
        }
        if !self.turno_config.permitir_recordatorios {
            ids.retain(|id| *id != "crear_recordatorio");
        }
        let ids_ref: Vec<&str> = ids;
        /* [318A-15 F3] `schemas_openai` aplica el deny silencioso (override de
         * la conversación o modo meta): la tool denegada no aparece en el
         * schema del modelo. */
        let schemas = self
            .registry
            .schemas_openai(Some(&ids_ref), &self.turno_config.modo);
        /* [318A-7] Desglose de contexto: emitir el desglose de la ventana
         * (system, tools, mensajes, resultados, reserva de salida) para que el
         * front muestre la barra de uso con secciones. */
        let desglose =
            DesgloseContexto::calcular(&estado.mensajes, &schemas, &self.turno_config.contexto);
        let _ = tx
            .send(AgenteEvento::ContextoDetalle {
                max_ventana: desglose.max_ventana,
                reserva_salida: desglose.reserva_salida,
                system_instrucciones: desglose.system_instrucciones,
                definiciones_tools: desglose.definiciones_tools,
                mensajes: desglose.mensajes,
                resultados_tools: desglose.resultados_tools,
                total_entrada: desglose.total_entrada,
                ocupacion_pct: desglose.ocupacion_pct,
            })
            .await;

        let mut ultimo_contenido = String::new();
        let tool_calls = {
            /* [01-09-2026] Fase 4: `on_token` devuelve false para abortar el
             * stream LLM en cuanto el cliente corta el SSE. */
            let mut on_token = |texto: &str| -> bool {
                ultimo_contenido.push_str(texto);
                !tx.is_closed()
            };
            self.llm_llamada(&estado.mensajes, &schemas, &mut on_token, tx)
                .await?
        };

        if tool_calls.is_empty() {
            return self
                .gestionar_respuesta_final(
                    estado,
                    ultimo_contenido,
                    tokens_prompt_total,
                    tokens_complecion_total,
                    tx,
                )
                .await;
        }

        /* Ejecutar cada tool propuesta (secuencial, con timeout). */
        for call in &tool_calls {
            let paso_tool = self
                .procesar_una_tool(estado, user_id, turno_id, call, tx)
                .await?;
            match paso_tool {
                PasoTool::Continua => {}
                /* Cancelación real: si el SSE se cortó a mitad de la ejecución
                 * de tools, no seguimos con el resto de tool_calls. */
                PasoTool::CerrarConexion => return Ok(PasoIteracion::ConexionCerrada),
                /* [Bloque 3, F1] `ask_user` termina el turno: el evento
                 * `Pregunta` ya se emitió y la respuesta del usuario llega como
                 * su siguiente mensaje. No se persiste respuesta final (la
                 * pregunta queda pendiente en el registro). */
                PasoTool::PreguntaUsuario => return Ok(PasoIteracion::TurnoCortadoPorPregunta),
            }
        }
        Ok(PasoIteracion::ProcesarTools)
    }

    /// [059A-S3] Compacción del contexto previa a cada llamada del turno.
    /// Devuelve la lista preparada y emite `Usage` con la ocupación cuando hubo
    /// compactación (la variante LLM del resumen sigue el default del plan:
    /// `resumir_con_llm=false`, fallback determinista; activable por consumidor
    /// vía `preparar_con`). Con una tool en curso no se compacta salvo
    /// ocupación degenerada (ventana de seguridad).
    async fn preparar_contexto_iteracion(
        &self,
        mensajes: Vec<AiMessage>,
        tx: &Sender<AgenteEvento>,
    ) -> Result<Vec<AiMessage>> {
        let (mensajes_prep, metricas) = {
            let mut cm = self.contexto.lock().await;
            let resultado = cm.preparar_con(
                &mensajes,
                0,
                None,
                self.tool_en_curso.load(std::sync::atomic::Ordering::Relaxed),
            );
            (resultado.mensajes, resultado.metricas)
        };
        if let Some(m) = &metricas {
            let _ = tx
                .send(AgenteEvento::Usage {
                    tokens_prompt: 0,
                    tokens_complecion: 0,
                    ocupacion_pct: Some(m.occupancy_pct),
                    provider: None,
                    modelo: None,
                })
                .await;
            tracing::info!(before = m.tokens_before, after = m.tokens_after, savings = %m.savings_pct, "compactación de contexto");
        }
        Ok(mensajes_prep)
    }

    /// [059A-S3] El modelo respondió sin tool_calls: guardas de turno en la
    /// finalización. Una respuesta repetida casi verbatim lleva aviso anexado
    /// (el usuario ve que el modelo sabe que se repite); una respuesta vacía
    /// permite UN reintento con aviso de sistema (sin re-ejecutar tools).
    /// Ambas desactivables vía `set_guardas`.
    async fn gestionar_respuesta_final(
        &self,
        estado: &mut EstadoTurno,
        mut ultimo_contenido: String,
        tokens_prompt_total: u32,
        tokens_complecion_total: u32,
        tx: &Sender<AgenteEvento>,
    ) -> Result<PasoIteracion> {
        let guardas = *self.guardas.lock().unwrap_or_else(|p| p.into_inner());
        if texto_vacio(&ultimo_contenido) && decidir_reintento_vacio(&guardas, estado.ya_reintentado)
        {
            estado.ya_reintentado = true;
            estado
                .mensajes
                .push(AiMessage::texto("system", aviso_vacio()));
            return Ok(PasoIteracion::ReintentarVacio);
        }
        if !texto_vacio(&ultimo_contenido) {
            if let Some(aviso) = aviso_por_repeticion(
                &ultimo_contenido,
                &estado.respuestas_asistente,
                guardas.umbral_repeticion,
            ) {
                ultimo_contenido.push('\n');
                ultimo_contenido.push_str(&aviso);
            }
        }
        let _ = tx
            .send(AgenteEvento::Token {
                texto: ultimo_contenido.clone(),
            })
            .await;
        let _ = tx
            .send(AgenteEvento::Usage {
                tokens_prompt: tokens_prompt_total,
                tokens_complecion: tokens_complecion_total,
                ocupacion_pct: None,
                provider: None,
                modelo: None,
            })
            .await;
        if !ultimo_contenido.trim().is_empty() {
            estado.respuesta_final = Some(ultimo_contenido.clone());
            estado.respuestas_asistente.push(ultimo_contenido);
        }
        Ok(PasoIteracion::FinalizarTurno)
    }

    /// [059A-S3] Wrap-up por límite de pasos: una última llamada SIN tools pide
    /// el resumen de cierre (hecho / pendiente / siguiente paso) cuando el
    /// turno agotó `max_turns` sin respuesta final. Si el cierre también queda
    /// vacío (proveedor caído), el turno queda sin respuesta y el consumidor
    /// decide reintentar.
    async fn cierre_wrap_up(
        &self,
        estado: &mut EstadoTurno,
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
        let resultado = self
            .llm_llamada(&mensajes_cierre, &[], &mut on_token, tx)
            .await?;
        if resultado.is_empty() {
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
}
