//! [059A-S3] Flujo de permiso de una tool del lote: veredicto F3,
//! rama no-ejecutada (ask/deny) y ejecución aprobada con timeout.

use super::*;

impl AgentRuntime {

    /// [059A-S3] Una tool del lote: emite `ToolStart`, resuelve el veredicto de
    /// permiso F3 y delega en la rama correspondiente (no-ejecutada vs
    /// ejecución real).
    pub(crate) async fn procesar_una_tool(
        &self,
        estado: &mut EstadoTurno,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        tx: &Sender<AgenteEvento>,
    ) -> Result<PasoTool> {
        let _ = tx
            .send(AgenteEvento::ToolStart {
                tool: call.nombre.clone(),
                argumentos: call.argumentos.clone(),
            })
            .await;

        /* [318A-15 F3] Permisos por tool (ask/allow/deny): política por tool
         * con herencia del modo (predeterminado → ask para efecto; meta →
         * deny para efecto; autonomo → allow) y override por conversación. El
         * SSE es unidireccional: `ask` emite `RequiereAprobacion` y omite la
         * ejecución (el LLM recibe el estado y pide confirmación); `deny`
         * (override o modo meta) deniega y NO se reintenta en el turno. La
         * decisión es pura (`decidir_permiso`); aquí solo se emite. */
        let permiso = self.registry.permiso_para_llamada(
            &call.nombre,
            &call.argumentos,
            &self.turno_config.modo,
        );
        let verdicto = decidir_permiso(permiso, estado.denegadas_en_turno.contains(&call.nombre));
        if verdicto != VerdictoPermiso::Ejecutar {
            self.manejar_verdicto_no_ejecutar(estado, call, verdicto, tx)
                .await?;
            return Ok(PasoTool::Continua);
        }
        self.ejecutar_tool_aprobada(estado, user_id, turno_id, call, tx)
            .await
    }

    /// [059A-S3] Veredicto distinto de Ejecutar: empuja el par
    /// assistant(tool_call)/tool con el mensaje según el veredicto, registra la
    /// denegación y emite los eventos (petición de aprobación o denegación).
    async fn manejar_verdicto_no_ejecutar(
        &self,
        estado: &mut EstadoTurno,
        call: &AiToolCall,
        verdicto: VerdictoPermiso,
        tx: &Sender<AgenteEvento>,
    ) -> Result<()> {
        self.empujar_tool_call_asistente(estado, call);
        /* [318A-10 02-09-2026] El tool DEBE llevar el mismo tool_call_id que la
         * tool_call del assistant previo (contrato OpenAI). */
        let primera_vez = estado.denegadas_en_turno.insert(call.nombre.clone());
        let (eventos, mensaje_tool, resumen) = match verdicto {
            VerdictoPermiso::Preguntar => {
                /* [318A-16 F2] Canal explícito: cada `ask` registra una
                 * petición con `id` y la emite para que la UI responda
                 * (Rechazar / Permitir / Permitir siempre). `RequiereAprobacion`
                 * se conserva por compatibilidad con los consumidores previos. */
                let id = Uuid::new_v4().to_string();
                let clasificacion =
                    self.registry.clasificar_llamada(&call.nombre, &call.argumentos);
                self.registry
                    .registrar_peticion(crate::aprobacion::PeticionAprobacion::nueva(
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
                /* deny: silencioso de schema (arriba) + fail-closed si aun así
                 * se propone (override cambiado a mitad de turno, modo meta,
                 * etc.). Motivo para la UI. */
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
        self.empujar_mensaje_tool_denegado(estado, call, &mensaje_tool);
        Ok(())
    }

    /// [059A-S3] Assistant con la tool_call: obligatorio antes del tool
    /// (contrato OpenAI; sin él el proveedor responde 400).
    fn empujar_tool_call_asistente(&self, estado: &mut EstadoTurno, call: &AiToolCall) {
        estado.mensajes.push(AiMessage {
            role: "assistant".into(),
            content: serde_json::Value::Null,
            tool_calls: Some(vec![AiToolCall {
                id: call.id.clone(),
                nombre: call.nombre.clone(),
                argumentos: call.argumentos.clone(),
            }]),
            tool_call_id: None,
        });
    }

    /// [059A-S3] Empuja el mensaje de rol `tool` (con su tool_call_id) que
    /// cierra la tool_call denegada en el historial.
    fn empujar_mensaje_tool_denegado(
        &self,
        estado: &mut EstadoTurno,
        call: &AiToolCall,
        mensaje_tool: &str,
    ) {
        let mut tool_msg = AiMessage::texto("tool", mensaje_tool);
        tool_msg.tool_call_id = Some(call.id.clone());
        estado.mensajes.push(tool_msg);
    }

    /// [059A-S3] Tool con veredicto Ejecutar: la lanza con timeout (excepto
    /// `task` → subagente y `ask_user` → pregunta), registra uso/telemetría y
    /// empuja assistant(tool_call)/tool al historial. Devuelve el control de
    /// flujo (conexión cortada o `ask_user` terminan el turno).
    async fn ejecutar_tool_aprobada(
        &self,
        estado: &mut EstadoTurno,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        tx: &Sender<AgenteEvento>,
    ) -> Result<PasoTool> {
        /* [318A-15 F4] tool `task`: sesión hija efímera (ver
         * `ejecutar_subagente`). El resto de tools van con timeout. */
        /* [318A-15 F6] Ventana de seguridad: marcar la tool en curso durante la
         * ejecución para que el siguiente `preparar_con` no compacte en medio
         * de un tool_call largo. Un panic que aborta el turno deja el flag en
         * true, pero el runtime del turno se descarta igualmente (la siguiente
         * conversación crea uno nuevo). */
        self.tool_en_curso
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let t0_ejecucion = std::time::Instant::now();
        let resultado: Result<crate::tool::AgentToolResult> = if call.nombre == "task" {
            self.ejecutar_subagente_desde_llamada(user_id, turno_id, call, tx)
                .await
        } else if call.nombre == "ask_user" {
            /* [Bloque 3, F1] Pregunta al usuario en medio del turno: valida,
             * registra pendiente, emite `Pregunta` y el turno termina. */
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
        estado.tools_ejecutadas += 1;
        /* [318A-15 F0] Telemetría: uso/fallo/duración de la tool (el timeout
         * cuenta como fallo; `task` registra la delegación aquí y las tools del
         * hijo en su propio bucle). */
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
        /* Cancelación real: si el SSE se cortó a mitad de la ejecución de
         * tools, no seguimos con el resto de tool_calls. */
        if tx.is_closed() {
            return Ok(PasoTool::CerrarConexion);
        }
        /* [Bloque 3, F1] `ask_user` termina el turno: la respuesta del usuario
         * llega como su siguiente mensaje. No se persiste respuesta final. */
        if call.nombre == "ask_user" {
            return Ok(PasoTool::PreguntaUsuario);
        }
        /* El resultado vuelve al LLM como mensaje de tool (contrato OpenAI:
         * assistant con tool_calls + tool con tool_call_id). */
        estado.mensajes.push(AiMessage {
            role: "assistant".into(),
            content: serde_json::Value::Null,
            tool_calls: Some(vec![AiToolCall {
                id: call.id.clone(),
                nombre: call.nombre.clone(),
                argumentos: call.argumentos.clone(),
            }]),
            tool_call_id: None,
        });
        estado.mensajes.push(AiMessage {
            role: "tool".into(),
            content: serde_json::Value::String(format!(
                "[resultado de {}{}]\n{contenido}",
                call.nombre,
                if ok { "" } else { " (ERROR)" }
            )),
            tool_calls: None,
            tool_call_id: Some(call.id.clone()),
        });
        Ok(PasoTool::Continua)
    }

}
