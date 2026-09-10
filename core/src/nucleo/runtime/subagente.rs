//! [059A-N S2] Split mecánico de `runtime.rs`: sesiones hijas efímeras
//! (`ejecutar_subagente_desde_llamada`, `ejecutar_subagente`). Movimiento puro.
//! [059A-S3] Refactor: el bucle hijo delega el riesgo por perfil y el
//! veredicto de permiso en helpers (misma semántica, funciones < 100 ef).

use super::*;

impl AgentRuntime {
    /// [318A-15 F4] Intercepta la tool `task` (paridad opencode/claurst):
    /// valida argumentos y delega en `ejecutar_subagente`. El resumen
    /// acotado del hijo se convierte en el `contenido` del resultado de
    /// tool que ve el modelo padre.
    pub(crate) async fn ejecutar_subagente_desde_llamada(
        &self,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        tx: &Sender<AgenteEvento>,
    ) -> Result<crate::tool::AgentToolResult> {
        /* Contrato del plan 318A-15 F4: `task { agente, objetivo, contexto?,
         * max_pasos? }`. `agente` = perfil; `objetivo` = tarea; `contexto`
         * opcional se anexa al objetivo; `max_pasos` opcional acota el
         * presupuesto del perfil (1..=16). */
        let perfil_id = call
            .argumentos
            .get("agente")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                crate::error::Error::Validacion("task: falta el parámetro 'agente'".into())
            })?;
        let objetivo = call
            .argumentos
            .get("objetivo")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                crate::error::Error::Validacion("task: falta el parámetro 'objetivo'".into())
            })?;
        let contexto = call
            .argumentos
            .get("contexto")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty());
        let max_pasos: Option<usize> = match call.argumentos.get("max_pasos") {
            None => None,
            Some(v) => Some(
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(|| {
                        crate::error::Error::Validacion(
                            "task: 'max_pasos' debe ser un entero".into(),
                        )
                    })?,
            ),
        };
        let Some(perfil) = perfil_subagente(perfil_id) else {
            return Ok(crate::tool::AgentToolResult {
                ok: false,
                contenido: format!(
                    "Agente de subagente desconocido '{perfil_id}'. Disponibles: {}",
                    perfiles_disponibles().join(", ")
                ),
                resumen: "agente_desconocido".into(),
                diff: None,
                evento_extra: None,
            });
        };
        let mut instruccion = objetivo.to_string();
        if let Some(contexto) = contexto {
            instruccion.push_str("\n\nContexto adicional del delegador:\n");
            instruccion.push_str(contexto);
        }
        let presupuesto = presupuesto_efectivo(perfil.presupuesto_pasos, max_pasos)?;
        let resultado = self
            .ejecutar_subagente(perfil, instruccion, presupuesto, user_id, turno_id, tx)
            .await?;
        Ok(crate::subagente::enmarcar_resultado_para_padre(resultado))
    }

    /// [318A-15 F4] Sesión hija efímera con aislamiento real de la
    /// conversación del padre (paridad opencode `task` / claurst
    /// `agent_tool.rs`): system prompt propio del perfil, whitelist de
    /// tools, presupuesto de pasos con wrap-up parcial y profundidad
    /// máxima 1. El hijo hereda la política F3 (mismo registro y overrides
    /// compartidos) y NUNCA escribe en persistencia: devuelve solo un
    /// resumen acotado.
    async fn ejecutar_subagente(
        &self,
        perfil: PerfilSubagente,
        instruccion: String,
        presupuesto_pasos: usize,
        user_id: Uuid,
        /* El turno del padre: las acciones del hijo se auditan contra él
         * para trazabilidad (no contra un UUID efímero). */
        turno_id: Uuid,
        tx: &Sender<AgenteEvento>,
    ) -> Result<crate::subagente::ResultadoSubagente> {
        /* Tope de concurrentes del proceso (default 2): el contador es global
         * porque el daemon corre un runtime por conversación. Saturado → se
         * rechaza la delegación (el padre recibe el motivo y decide); no hay
         * cola. */
        if !concurrencia_permitida(SUBAGENTES_EN_CURSO.load(std::sync::atomic::Ordering::SeqCst)) {
            return Ok(crate::subagente::ResultadoSubagente {
                ok: false,
                resumen: format!(
                    "tope de subagentes concurrentes alcanzado ({CONCURRENTES_MAX_SUBAGENTES}): espera a que termine uno antes de delegar"
                ),
                parcial: false,
                pasos_usados: 0,
            });
        }
        SUBAGENTES_EN_CURSO.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _guardia_concurrencia = GuardiaConcurrencia;

        /* Fail-closed: profundidad máxima 1 (el schema del hijo ya excluye
         * `task`, esto cubre llamadas directas). */
        if !profundidad_permitida(
            self.profundidad_subagente
                .load(std::sync::atomic::Ordering::SeqCst),
        ) {
            return Ok(crate::subagente::ResultadoSubagente {
                ok: false,
                resumen: "profundidad máxima de subagentes alcanzada (1): no se puede delegar dentro de un subagente"
                    .into(),
                parcial: false,
                pasos_usados: 0,
            });
        }
        self.profundidad_subagente
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let _guardia = GuardiaProfundidad(&self.profundidad_subagente);

        /* [Bloque 3, F4] Hook `SubagentStart` (informativo). */
        self.hook_subagente_inicio(&perfil, &instruccion, presupuesto_pasos)
            .await;
        let _ = tx
            .send(AgenteEvento::SubagenteInicio {
                perfil: perfil.id.to_string(),
                instruccion: instruccion.clone(),
            })
            .await;

        let mut mensajes = vec![
            AiMessage::texto("system", perfil.instruccion_sistema),
            AiMessage::texto("user", instruccion),
        ];
        let schemas = schema_hijo(&self.registry, &perfil, &self.modo_efectivo());

        let mut texto_final = String::new();
        let mut pasos = 0usize;
        let mut parcial_final = false;
        let mut denegadas_hijo: HashSet<String> = HashSet::new();
        /* [059A-S3] Cada iteración es un paso acotado por presupuesto; la
         * lógica (llamada LLM + procesado de tool_calls del hijo) vive en
         * `paso_subagente` para mantener el bucle por debajo de 100 ef. */
        while pasos < presupuesto_pasos {
            pasos += 1;
            if let Some(final_hijo) = self
                .paso_subagente(
                    &mut mensajes,
                    &perfil,
                    &schemas,
                    user_id,
                    turno_id,
                    &mut denegadas_hijo,
                    tx,
                )
                .await?
            {
                texto_final = final_hijo;
                break;
            }
        }

        /* Presupuesto agotado sin respuesta final: cierre estructurado
         * parcial (mismo contrato del wrap-up de F5) en vez de cortar. */
        if texto_final.is_empty() {
            parcial_final = true;
            /* [318A-15 F0] Telemetría: subagente cerrado como parcial. */
            self.telemetria().registrar_subagente_parcial();
            texto_final = self.cierre_parcial_subagente(&mut mensajes, tx).await?;
        }

        /* [Bloque 3, F4] Hook `SubagentStop` (informativo): resultado acotado
         * de la sesión hija (resumen + si cerró parcial por presupuesto). Sin
         * hooks configurados es un no-op. */
        let resumen = crate::subagente::resumen_acotado(&texto_final);
        let ok = !resumen.is_empty();
        /* [Bloque 3, F4] Hook `SubagentStop` (informativo). */
        self.hook_subagente_fin(&perfil, &resumen, ok, parcial_final, pasos)
            .await;
        let _ = tx
            .send(AgenteEvento::SubagenteFin {
                resumen: resumen.clone(),
                ok,
                parcial: parcial_final,
            })
            .await;
        Ok(crate::subagente::ResultadoSubagente {
            ok,
            resumen,
            parcial: parcial_final,
            pasos_usados: pasos,
        })
    }

    /// [Bloque 3, F4] Hook `SubagentStart` (informativo): observa el arranque
    /// de la sesión hija con perfil, objetivo y presupuesto. Sin hooks
    /// configurados es un no-op.
    async fn hook_subagente_inicio(
        &self,
        perfil: &PerfilSubagente,
        instruccion: &str,
        presupuesto_pasos: usize,
    ) {
        let _ = self
            .disparar_hook(
                EventoHook::SubagentStart,
                serde_json::json!({
                    "agente": perfil.id.to_string(),
                    "objetivo": instruccion,
                    "max_pasos": presupuesto_pasos,
                }),
            )
            .await;
    }

    /// [Bloque 3, F4] Hook `SubagentStop` (informativo): resultado acotado de
    /// la sesión hija (resumen + si cerró parcial por presupuesto). Sin hooks
    /// configurados es un no-op.
    async fn hook_subagente_fin(
        &self,
        perfil: &PerfilSubagente,
        resumen: &str,
        ok: bool,
        parcial: bool,
        pasos: usize,
    ) {
        let _ = self
            .disparar_hook(
                EventoHook::SubagentStop,
                serde_json::json!({
                    "agente": perfil.id.to_string(),
                    "resumen": resumen,
                    "ok": ok,
                    "parcial": parcial,
                    "pasos": pasos,
                }),
            )
            .await;
    }

    /// [059A-S3] Un paso del bucle hijo: pide un avance al LLM y, si responde
    /// con tool_calls, procesa cada una (riesgo por perfil F3 + veredicto de
    /// permiso compartido) empujando los mensajes al historial del hijo.
    /// Devuelve `Some(texto)` cuando el hijo concluyó (respuesta sin tools).
    #[allow(clippy::too_many_arguments)]
    async fn paso_subagente(
        &self,
        mensajes: &mut Vec<AiMessage>,
        perfil: &PerfilSubagente,
        schemas: &[Value],
        user_id: Uuid,
        turno_id: Uuid,
        denegadas_hijo: &mut HashSet<String>,
        tx: &Sender<AgenteEvento>,
    ) -> Result<Option<String>> {
        let mut parcial = String::new();
        let mut on_token = |t: &str| {
            parcial.push_str(t);
            true
        };
        let llamadas = self
            .llm_llamada(mensajes, schemas, &mut on_token, tx)
            .await?;
        if llamadas.is_empty() {
            /* Respuesta final del hijo: es el resumen que volverá al padre. */
            return Ok(Some(parcial));
        }
        for call in llamadas {
            if let Some(aviso) = aviso_comando_excede_perfil(perfil, &call) {
                empujar_tool_call(mensajes, &call);
                let mut tool_msg = AiMessage::texto("tool", aviso);
                tool_msg.tool_call_id = Some(call.id.clone());
                mensajes.push(tool_msg);
                continue;
            }
            let permiso = self.registry.permiso_para_llamada(
                &call.nombre,
                &call.argumentos,
                &self.modo_efectivo(),
            );
            let verdicto = decidir_permiso(permiso, denegadas_hijo.contains(&call.nombre));
            let mensaje_tool = self
                .mensaje_verdicto_subagente(user_id, turno_id, &call, verdicto, denegadas_hijo, tx)
                .await?;
            empujar_tool_call(mensajes, &call);
            let mut tool_msg = AiMessage::texto("tool", mensaje_tool);
            tool_msg.tool_call_id = Some(call.id.clone());
            mensajes.push(tool_msg);
        }
        Ok(None)
    }

    /// [059A-S3] Ejecuta una llamada de tool del hijo según el veredicto F3 o
    /// devuelve el mensaje de denegación/pendiente correspondiente. Efectos
    /// laterales acotados: eventos SSE, telemetría y el registro local de
    /// denegadas del hijo (evita repetir la misma tool).
    async fn mensaje_verdicto_subagente(
        &self,
        user_id: Uuid,
        turno_id: Uuid,
        call: &AiToolCall,
        verdicto: VerdictoPermiso,
        denegadas_hijo: &mut HashSet<String>,
        tx: &Sender<AgenteEvento>,
    ) -> Result<String> {
        let resultado = match verdicto {
            VerdictoPermiso::Ejecutar => {
                let _ = tx
                    .send(AgenteEvento::ToolStart {
                        tool: call.nombre.clone(),
                        argumentos: call.argumentos.clone(),
                    })
                    .await;
                let t0_ejecucion = std::time::Instant::now();
                let resultado = self.ejecutar_tool(user_id, turno_id, call, tx).await?;
                let _ = tx
                    .send(AgenteEvento::ToolResult {
                        tool: call.nombre.clone(),
                        ok: resultado.ok,
                        resumen: resultado.resumen.clone(),
                        diff: resultado.diff.clone(),
                    })
                    .await;
                /* [318A-15 F0] Telemetría del hijo: las tools del
                 * subagente cuentan en el acumulador del turno. */
                self.telemetria().registrar_uso(
                    &call.nombre,
                    resultado.ok,
                    t0_ejecucion.elapsed().as_millis() as u64,
                );
                resultado.contenido
            }
            VerdictoPermiso::Preguntar => {
                let _ = tx
                    .send(AgenteEvento::RequiereAprobacion {
                        tool: call.nombre.clone(),
                        argumentos: call.argumentos.clone(),
                    })
                    .await;
                denegadas_hijo.insert(call.nombre.clone());
                format!(
                    "[{} REQUIERE APROBACIÓN DEL USUARIO] No se ejecutó; pide confirmación y espera.",
                    call.nombre
                )
            }
            VerdictoPermiso::RepetidoPregunta => format!(
                "[{} REQUIERE APROBACIÓN DEL USUARIO (repetido)] Sigue pendiente: no insistas.",
                call.nombre
            ),
            VerdictoPermiso::Denegar => {
                denegadas_hijo.insert(call.nombre.clone());
                let _ = tx
                    .send(AgenteEvento::PermisoDenegado {
                        tool: call.nombre.clone(),
                        motivo: "denegada_por_usuario".into(),
                    })
                    .await;
                self.telemetria().registrar_denegacion();
                format!(
                    "[{} DENEGADA] NO la reintentes; cambia de plan.",
                    call.nombre
                )
            }
            VerdictoPermiso::RepetidoDenegado => format!(
                "[{} DENEGADA — repetida] Ya se te indicó; no la vuelvas a proponer.",
                call.nombre
            ),
        };
        Ok(resultado)
    }

    /// [059A-S3] Wrap-up parcial del hijo: pide una última respuesta acotada
    /// al LLM cuando el presupuesto se agota sin conclusión (contrato F5).
    async fn cierre_parcial_subagente(
        &self,
        mensajes: &mut Vec<AiMessage>,
        tx: &Sender<AgenteEvento>,
    ) -> Result<String> {
        mensajes.push(AiMessage::texto("system", wrap_up_instruccion()));
        let mut parcial = String::new();
        let mut on_token = |t: &str| {
            parcial.push_str(t);
            true
        };
        let _ = self.llm_llamada(mensajes, &[], &mut on_token, tx).await?;
        Ok(parcial)
    }
}

/// [059A-S3] Si el comando supera el riesgo máximo del perfil (F3: p. ej.
/// `explorar` = solo Seguro), devuelve el aviso que el hijo debe ver; en caso
/// contrario `None`. El tope del perfil manda sobre cualquier regla F1.
fn aviso_comando_excede_perfil(perfil: &PerfilSubagente, call: &AiToolCall) -> Option<String> {
    if call.nombre != "comando" {
        return None;
    }
    let maximo = perfil.comandos_max_riesgo?;
    let nivel = crate::bash_clasificar::clasificar_comando(
        call.argumentos
            .get("comando")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(""),
    );
    if nivel > maximo {
        Some(format!(
            "[comando DENEGADA por perfil] riesgo {} supera el máximo del perfil '{}' ({}); NO la reintentes con ese comando.",
            nivel.clave(),
            perfil.id,
            maximo.clave()
        ))
    } else {
        None
    }
}

/// [059A-S3] Empuja al historial el mensaje `assistant` con la `tool_call`
/// del hijo (idem al patrón del runtime padre). Extraído para no duplicar
/// el ensamblado entre las ramas del bucle.
fn empujar_tool_call(mensajes: &mut Vec<AiMessage>, call: &AiToolCall) {
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
}
