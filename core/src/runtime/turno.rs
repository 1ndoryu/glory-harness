//! [059A-N S2] Split mecánico de `runtime.rs`: bucle del turno principal
//! (`ejecutar_turno`). Movimiento puro — sin cambios de lógica.
//!
//! `impl AgentRuntime` en submódulo: los campos privados del runtime viven en el
//! módulo padre (`crate::runtime`), accesibles desde este hijo (privacidad por
//! módulo). La política de permisos (`decidir_permiso`) y los helpers del prompt
//! siguen en `mod.rs`.

use super::*;

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
        let mut mensajes: Vec<AiMessage> = Vec::new();
        mensajes.push(AiMessage::texto("system", self.prompt_sistema()));
        /* [Bloque 3, F1] Cola de respuestas previas del asistente (del
         * historial del consumidor) para el detector de repetición de las
         * guardas. Solo contenido de texto real; tool_calls/Null no cuentan. */
        let mut respuestas_asistente: Vec<String> = historial
            .iter()
            .filter(|m| m.role == "assistant")
            .filter_map(|m| match &m.content {
                Value::String(t) if !t.trim().is_empty() => Some(t.clone()),
                _ => None,
            })
            .collect();
        /* Un solo reintento por turno tras respuesta vacía. */
        let mut ya_reintentado = false;
        mensajes.extend(historial);
        mensajes.push(AiMessage::texto("user", mensaje_usuario.clone()));

        let tokens_prompt_total = 0u32;
        let tokens_complecion_total = 0u32;
        let mut tools_ejecutadas = 0usize;
        /* [29-08-2026] Persistencia de la conversación (Fase 4): la respuesta
         * final del asistente se guarda al terminar el turno para que recargar
         * conserve el historial completo (el mensaje del usuario lo persiste el
         * consumidor antes de llamar). */
        let mut respuesta_final: Option<String> = None;
        /* [318A-15 F3] Tools denegadas en este turno (por política o por
         * negación del usuario): si el modelo las vuelve a proponer en el
         * MISMO turno, no se re-emite el evento ni se le vuelve a explicar —
         * se le devuelve "denegada" para que cambie de plan (no reintento
         * automático). */
        let mut denegadas_en_turno: std::collections::HashSet<String> = std::collections::HashSet::new();
        /* [318A-16 F5] Modo plan: propuesta fresca por turno. El turno
         * anterior dejó su propuesta legible (`plan_actual`); al empezar uno
         * nuevo en modo plan se sustituye (la UI decidió aplicar o
         * descartar entre turnos). Fuera de modo plan, `None`. */
        {
            let mut plan = self.plan_actual.lock().unwrap_or_else(|p| p.into_inner());
            *plan = if self.turno_config.modo == "plan" {
                Some(Arc::new(std::sync::RwLock::new(
                    crate::plan::PlanPropuesto::default(),
                )))
            } else {
                None
            };
        }

        'turnos: for _turno in 0..self.turno_config.max_turns {
            /* [01-09-2026] Fase 4: cancelación real — si el cliente cortó el
             * SSE (receiver dropeado), el sender está cerrado y no se sigue
             * ejecutando tools ni consumiendo tokens. */
            if tx.is_closed() {
                break;
            }
            /* Contexto: preparar (compactar si hace falta) ANTES de cada llamada. */
            let (mensajes_prep, metricas) = {
                let mut cm = self.contexto.lock().await;
                /* [318A-15 F6] Compactación dirigida: `resumen_llm=None` usa el
                 * fallback B determinista (la variante A es activable por el
                 * consumidor vía `preparar_con`; `resumir_con_llm=false` por
                 * defecto según §8.4 del plan). Con una tool en curso no se
                 * compacta salvo ocupación degenerada (ventana de seguridad). */
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
            mensajes = mensajes_prep;

            let mut ids = self.registry.ids();
            if !self.turno_config.permitir_busqueda_web {
                ids.retain(|id| *id != "web_search");
            }
            if !self.turno_config.permitir_recordatorios {
                ids.retain(|id| *id != "crear_recordatorio");
            }
            let ids_ref: Vec<&str> = ids;
            /* [318A-15 F3] `schemas_openai` aplica el deny silencioso
             * (override de la conversación o modo meta): la tool denegada no
             * aparece en el schema del modelo. */
            let schemas = self.registry.schemas_openai(Some(&ids_ref), &self.turno_config.modo);
            /* [318A-7] Desglose de contexto: emitir el desglose de la ventana
             * (system, tools, mensajes, resultados, reserva de salida) para que
             * el front muestre la barra de uso con secciones. */
            let desglose = DesgloseContexto::calcular(&mensajes, &schemas, &self.turno_config.contexto);
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
                /* [01-09-2026] Fase 4: `on_token` devuelve false para abortar
                 * el stream LLM en cuanto el cliente corta el SSE. */
                let mut on_token = |texto: &str| -> bool {
                    ultimo_contenido.push_str(texto);
                    !tx.is_closed()
                };
                self.llm_llamada(&mensajes, &schemas, &mut on_token, tx).await?
            };

            if tool_calls.is_empty() {
                /* [Bloque 3, F1] Guardas de turno en la finalización: una
                 * respuesta repetida casi verbatim lleva aviso anexado (el
                 * usuario ve que el modelo sabe que se repite); una respuesta
                 * vacía permite UN reintento con aviso de sistema (sin
                 * re-ejecutar tools). Ambas desactivables via `set_guardas`. */
                let guardas = *self.guardas.lock().unwrap_or_else(|p| p.into_inner());
                if texto_vacio(&ultimo_contenido) && decidir_reintento_vacio(&guardas, ya_reintentado)
                {
                    ya_reintentado = true;
                    mensajes.push(AiMessage::texto("system", aviso_vacio()));
                    continue;
                }
                if !texto_vacio(&ultimo_contenido) {
                    if let Some(aviso) = aviso_por_repeticion(
                        &ultimo_contenido,
                        &respuestas_asistente,
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
                    respuesta_final = Some(ultimo_contenido.clone());
                    respuestas_asistente.push(ultimo_contenido);
                }
                break;
            }

            /* Ejecutar cada tool propuesta (secuencial, con timeout). */
            for call in &tool_calls {
                let _ = tx
                    .send(AgenteEvento::ToolStart {
                        tool: call.nombre.clone(),
                        argumentos: call.argumentos.clone(),
                    })
                    .await;

                /* [318A-15 F3] Permisos por tool (ask/allow/deny): política
                 * por tool con herencia del modo (predeterminado → ask para
                 * efecto; meta → deny para efecto; autonomo → allow) y
                 * override por conversación. El SSE es unidireccional:
                 * `ask` emite `RequiereAprobacion` y omite la ejecución (el
                 * LLM recibe el estado y pide confirmación); `deny` (override
                 * o modo meta) deniega y NO se reintenta en el turno. La
                 * decisión es pura (`decidir_permiso`); aquí solo se emite. */
                let permiso = self.registry.permiso_para_llamada(
                    &call.nombre,
                    &call.argumentos,
                    &self.turno_config.modo,
                );
                let verdicto = decidir_permiso(permiso, denegadas_en_turno.contains(&call.nombre));
                if verdicto != VerdictoPermiso::Ejecutar {
                    /* Assistant con la tool_call: obligatorio antes del tool
                     * (contrato OpenAI; sin él el proveedor responde 400). */
                    mensajes.push(AiMessage {
                        role: "assistant".into(),
                        content: serde_json::Value::Null,
                        tool_calls: Some(vec![AiToolCall {
                            id: call.id.clone(),
                            nombre: call.nombre.clone(),
                            argumentos: call.argumentos.clone(),
                        }]),
                        tool_call_id: None,
                    });
                    /* [318A-10 02-09-2026] El tool DEBE llevar el mismo
                     * tool_call_id que la tool_call del assistant previo
                     * (contrato OpenAI). */
                    let primera_vez = denegadas_en_turno.insert(call.nombre.clone());
                    let (eventos, mensaje_tool, resumen) = match verdicto {
                        VerdictoPermiso::Preguntar => {
                            /* [318A-16 F2] Canal explícito: cada `ask` registra
                             * una petición con `id` y la emite para que la UI
                             * responda (Rechazar / Permitir / Permitir
                             * siempre). `RequiereAprobacion` se conserva por
                             * compatibilidad con los consumidores previos. */
                            let id = Uuid::new_v4().to_string();
                            let clasificacion =
                                self.registry.clasificar_llamada(&call.nombre, &call.argumentos);
                            self.registry.registrar_peticion(crate::aprobacion::PeticionAprobacion::nueva(
                                &id,
                                call.nombre.clone(),
                                call.argumentos.clone(),
                                clasificacion.clone(),
                            ));
                            (
                                vec![
                                    AgenteEvento::PeticionAprobacion {
                                        id,
                                        tool: call.nombre.clone(),
                                        argumentos: call.argumentos.clone(),
                                        clasificacion,
                                    },
                                    AgenteEvento::RequiereAprobacion {
                                        tool: call.nombre.clone(),
                                        argumentos: call.argumentos.clone(),
                                    },
                                ],
                                format!(
                                    "[{} REQUIERE APROBACIÓN DEL USUARIO] La acción no se ejecutó; explica al usuario qué se hará y pide confirmación.",
                                    call.nombre
                                ),
                                "requiere_aprobacion".to_string(),
                            )
                        }
                        VerdictoPermiso::RepetidoPregunta => (
                            vec![],
                            format!(
                                "[{} REQUIERE APROBACIÓN DEL USUARIO (repetido)] Sigue pendiente de aprobación: no insistas, explica y espera la confirmación del usuario.",
                                call.nombre
                            ),
                            "requiere_aprobacion".to_string(),
                        ),
                        VerdictoPermiso::Denegar | VerdictoPermiso::RepetidoDenegado => {
                            /* deny: silencioso de schema (arriba) + fail-closed
                             * si aun así se propone (override cambiado a mitad
                             * de turno, modo meta, etc.). Motivo para la UI. */
                            let motivo = if self.registry.tiene_efecto(&call.nombre)
                                && self.turno_config.modo == "meta"
                            {
                                "denegada_por_politica".to_string()
                            } else {
                                "denegada_por_usuario".to_string()
                            };
                            let eventos = if verdicto == VerdictoPermiso::Denegar {
                                vec![AgenteEvento::PermisoDenegado {
                                    tool: call.nombre.clone(),
                                    motivo: motivo.clone(),
                                }]
                            } else {
                                vec![]
                            };
                            let mensaje = if primera_vez {
                                format!(
                                    "[{} DENEGADA ({})] El usuario no autorizó esta acción; NO la reintentes. Continúa con otras herramientas o explica el plan alternativo.",
                                    call.nombre, motivo
                                )
                            } else {
                                format!(
                                    "[{} DENEGADA ({}) — repetida] Ya se te indicó que esta tool está denegada; NO la vuelvas a proponer en este turno.",
                                    call.nombre, motivo
                                )
                            };
                            (eventos, mensaje, "permiso_denegado".to_string())
                        }
                        VerdictoPermiso::Ejecutar => unreachable!("filtrado arriba"),
                    };
                    if !eventos.is_empty() {
                        /* [318A-15 F0] Telemetría: acción bloqueada emitida. */
                        self.telemetria().registrar_denegacion();
                        for ev in eventos {
                            let _ = tx.send(ev).await;
                        }
                    }
                    let _ = tx
                        .send(AgenteEvento::ToolResult {
                            tool: call.nombre.clone(),
                            ok: false,
                            resumen,
                            diff: None,
                        })
                        .await;
                    let mut tool_msg = AiMessage::texto("tool", mensaje_tool);
                    tool_msg.tool_call_id = Some(call.id.clone());
                    mensajes.push(tool_msg);
                    continue;
                }

                /* [318A-15 F4] tool `task`: sesión hija efímera (ver
                 * `ejecutar_subagente`). El resto de tools van con timeout. */
                /* [318A-15 F6] Ventana de seguridad: marcar la tool en curso
                 * durante la ejecución para que el siguiente `preparar_con` no
                 * compacte en medio de un tool_call largo. Un panic que aborta
                 * el turno deja el flag en true, pero el runtime del turno se
                 * descarta igualmente (la siguiente conversación crea uno
                 * nuevo). */
                self.tool_en_curso
                    .store(true, std::sync::atomic::Ordering::Relaxed);
                let t0_ejecucion = std::time::Instant::now();
                let resultado: Result<crate::tool::AgentToolResult> =
                    if call.nombre == "task" {
                        self.ejecutar_subagente_desde_llamada(user_id, turno_id, call, tx)
                            .await
                    } else if call.nombre == "ask_user" {
                        /* [Bloque 3, F1] Pregunta al usuario en medio del
                         * turno: valida, registra pendiente, emite `Pregunta`
                         * y el turno termina (abajo, `break 'turnos`). */
                        procesar_pregunta(&self.registry, call, tx).await
                    } else {
                        tokio::time::timeout(
                            self.turno_config.timeout_tool,
                            self.ejecutar_tool(user_id, turno_id, call, tx),
                        )
                        .await
                        .unwrap_or_else(|_| {
                            Err(crate::error::Error::Validacion(format!(
                                "Timeout: la tool '{}' tardó más de {}s",
                                call.nombre,
                                self.turno_config.timeout_tool.as_secs()
                            )))
                        })
                    };
                self.tool_en_curso
                    .store(false, std::sync::atomic::Ordering::Relaxed);
                let (ok, contenido, resumen, diff) = match resultado {
                    Ok(r) => (r.ok, r.contenido.clone(), r.resumen.clone(), r.diff.clone()),
                    Err(error) => (false, format!("Error: {error}"), "error".to_string(), None),
                };
                tools_ejecutadas += 1;
                /* [318A-15 F0] Telemetría: uso/fallo/duración de la tool
                 * (el timeout cuenta como fallo; `task` registra la
                 * delegación aquí y las tools del hijo en su propio bucle). */
                self.telemetria().registrar_uso(
                    &call.nombre,
                    ok,
                    t0_ejecucion.elapsed().as_millis() as u64,
                );
                let _ = tx
                    .send(AgenteEvento::ToolResult {
                        tool: call.nombre.clone(),
                        ok,
                        resumen: resumen.clone(),
                        diff: diff.clone(),
                    })
                    .await;
                /* Cancelación real: si el SSE se cortó a mitad de la ejecución
                 * de tools, no seguimos con el resto de tool_calls. */
                if tx.is_closed() {
                    break;
                }
                /* [Bloque 3, F1] `ask_user` termina el turno: el evento
                 * `Pregunta` ya se emitió y la respuesta del usuario llega
                 * como su siguiente mensaje. No se persiste respuesta final
                 * (la pregunta queda pendiente en el registro). */
                if call.nombre == "ask_user" {
                    break 'turnos;
                }
                /* El resultado vuelve al LLM como mensaje de tool (contrato
                 * OpenAI: assistant con tool_calls + tool con tool_call_id). */
                mensajes.push(AiMessage {
                    role: "assistant".into(),
                    content: serde_json::Value::Null,
                    tool_calls: Some(vec![AiToolCall {
                        id: call.id.clone(),
                        nombre: call.nombre.clone(),
                        argumentos: call.argumentos.clone(),
                    }]),
                    tool_call_id: None,
                });
                mensajes.push(AiMessage {
                    role: "tool".into(),
                    content: serde_json::Value::String(format!(
                        "[resultado de {}{}]\n{contenido}",
                        call.nombre,
                        if ok { "" } else { " (ERROR)" }
                    )),
                    tool_calls: None,
                    tool_call_id: Some(call.id.clone()),
                });
            }
        }

        /* [318A-15 F5] Límite de pasos con wrap-up: si el turno agotó
         * `max_turns` sin respuesta final y el cliente sigue conectado, no se
         * corta en seco: una última llamada SIN tools pide el resumen de
         * cierre (hecho / pendiente / siguiente paso). Si el cierre también
         * queda vacío (proveedor caído), el turno queda sin respuesta y el
         * consumidor decide reintentar (mismo contrato que hoy). */
        if respuesta_final.is_none() && !tx.is_closed() {
            let mut mensajes_cierre = mensajes.clone();
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
                    respuesta_final = Some(ultimo_contenido);
                }
            }
        }

        /* Auditoría del turno (R3: siempre por el puerto, nunca SQL propio). */
        self.persistencia
            .guardar_turno(&TurnoPersistido {
                id: turno_id,
                conversacion_id,
                user_id,
                estado: "ok".into(),
                resumen: Some(mensajes_usuario_resumen(&mensaje_usuario)),
                creado_en: chrono::Utc::now(),
                provider: Some(self.turno_config.provider.clone()),
                modelo: Some(self.turno_config.modelo.clone()),
                tokens_prompt: tokens_prompt_total,
                tokens_complecion: tokens_complecion_total,
                tools_ejecutadas: tools_ejecutadas as u32,
                duracion_ms: inicio.elapsed().as_millis() as u64,
                error: None,
            })
            .await?;

        /* [29-08-2026] Persistir la respuesta del asistente (si el proveedor
         * devolvió texto) y tocar `actualizado_en` de la conversación. Si no
         * hubo respuesta (fallo retryable), el turno ya quedó como
         * pendiente/fallido y el usuario reintenta: no se escribe nada falso.
         * Las tareas programadas pasan `conversacion_id = nil`: no se
         * persiste nada. */
        if let Some(respuesta) = &respuesta_final {
            if conversacion_id != Uuid::nil() {
                self.persistencia
                    .guardar_mensaje(&MensajePersistido {
                        id: Uuid::new_v4(),
                        conversacion_id,
                        rol: "assistant".into(),
                        contenido: respuesta.clone(),
                        creado_en: chrono::Utc::now(),
                    })
                    .await?;
                self.persistencia.conversacion_tocar(conversacion_id).await?;
            }
        }

        /* [318A-15 F0] Telemetría del turno (no invasiva): agregados ya
         * observados durante la ejecución, emitidos justo antes de `Done` y
         * reseteados para el siguiente turno. Los turnos fallidos no llegan
         * aquí (emiten `Error` con motivo y retryable). */
        {
            let compactaciones = self.contexto.lock().await.compactaciones();
            let motivo = motivo_cierre(
                respuesta_final.is_some(),
                respuesta_final.is_none() && !tx.is_closed(),
                tx.is_closed(),
            );
            let evento = {
                /* [318A-15 F0] El guard de la telemetría no debe cruzar un
                 * await (el runtime exige futures Send): se construye y
                 * resetea el acumulador en un bloque propio y se envía fuera. */
                let mut acumulador = self.telemetria();
                let e = construir_evento(conversacion_id, motivo, compactaciones, &acumulador);
                *acumulador = TelemetriaTurno::nuevo();
                e
            };
            let _ = tx.send(evento).await;
        }
        let _ = tx.send(AgenteEvento::Done { turno_id }).await;
        Ok(())
    }
}

