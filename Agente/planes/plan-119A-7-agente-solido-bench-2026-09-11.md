# Plan 119A-7 — Agente sólido: auditoría del núcleo vs referencias + bench propio

> ID: **119A-7** · Fecha: 2026-09-11 (profundizado con `supervisor-thinking`)
> · Estado: **activo (planificado, sin implementar)**.
> Origen: pide revisar qué falta antes de probar GH para que funcione bien —
> no la interfaz, sino el entorno donde trabajan las IAs (la inteligencia):
> comparar con todas las referencias de principio a fin y medir con
> benchmarks de agentes para corregir defectos.
> **Solo planificación: sin código tocado.**

## 0. Veredicto de arquitectura (skill `supervisor-thinking`)

**VIABLE CON RESERVAS.** Reservas que este plan ya incorpora: (a) «sólido»
sin número no es exigible → §6 lo define; (b) los runs autónomos de F2/F3
ejecutan comandos generados por el modelo en la máquina de desarrollo →
F0 impone jaula; (c) el modelo es no-determinista → §5 impone protocolo de
repetición; (d) el bench lo escribe quien escribe el agente → §5 impone
congelado anti-sobreajuste; (e) SWE-bench/Terminal-Bench completos son
sobreingeniería aquí (Windows, sin Docker, costo) → F3 los evalúa y el
núcleo exigible es el mini-bench propio.

## 1. Problema y no-goals

- **Problema:** se desconoce si el núcleo agente de GH (tools, permisos,
  modos, recuperación, contexto) sostiene trabajo real de principio a fin;
  los defectos se descubren hoy por reporte del usuario, no por medición.
- **No-goals:** evaluar MODELOS (se mide el harness con modelo fijo y
  registrado, no se comparan proveedores); perseguir SWE-bench completo;
  tocar UI; convertir el bench en puerta del gate (es costoso y fluctuante).

## 2. Punto de partida: hechos confirmados vs supuestos

- **Hechos (11-09):** referencias en `data/referencias-cli/` (`opencode`,
  `hermes-agent`, `grok-cli`, `claurst`, `vscode`); tools en
  `core/src/herramientas/` (`file_write`, `file_patch`, lectura, `comando`,
  `content_search`, `repo_map`, `skill`, `tareas`, `todo`, `web_search`,
  `web_fetch` con proveedor inyectado, `navegador`, `mcp`; permisos en
  `tool.rs`); sin infra de evaluación en el repo; runs vía CLI (`run`,
  `schedule run`, `web`, Tauri).
- **Supuestos a confirmar en F1:** qué cubre cada referencia que GH no;
  si el daemon temporiza `schedule run` o el tick es solo manual; si el
  modelo puede auto-gestionar tareas/memoria con las tools actuales.

## 3. Fases

- **F0 — Jaula y protocolo de medición (prerrequisito, sin modelo).**
  Directorio temporal por run fuera del árbol (se limpia solo), workspace
  raíz fijado ahí, sin secretos ni red salvo tarea que la exija, comandos
  destructivos fuera de la jaula prohibidos por construcción, runs
  supervisados. Protocolo: modelo fijado + versión registrada, topes
  (turnos, costo y tiempo por run) y repetición ×3 (pasa si ≥2).
  Verificación: un run de humo que demuestra que la jaula contiene
  (`file_write` fuera de la raíz falla, `comando` no sale del dir).
- **F1 — Matriz de contrato vs las 5 referencias (solo lectura).**
  Contrato GH (tools + calidad de sus descripciones, permisos/approvals,
  modos, skills, memoria/curador, subagentes, MCP, sandbox/límites, truncado,
  errores reintentables, compactación, rewind) cruzado con cada referencia;
  cada gap → tarea F4 o descarte con razón. Salida: tabla + top 10 por
  impacto. Sin código.
- **F2 — Flujo canónico end-to-end (11 pasos, pasa/falla por paso).**
  En jaula F0, con modelo real: 1 crear archivo, 2 leerlo, 3 modificarlo con
  patch, 4 buscar en el repo, 5 ejecutar comando y usar su salida, 6 test
  verde, 7 aprobación de acción con efecto, 8 rewind, 9 compactar sin perder
  lo esencial, 10 turno solo-lectura que no escribe, 11 error provocado
  (comando que falla) que el agente diagnostica sin inventar. Cada fallo =
  defecto con evidencia del turno. Criterio: 11/11 en ≥2 de 3 runs.
- **F3 — Bench: evaluar los públicos, construir el propio.**
  Evaluar SWE-bench / Terminal-Bench contra las restricciones (Windows, sin
  Docker, costo, red): lo inviable se descarta con razón escrita, no se
  persigue. Núcleo: mini-bench versionado (12–20 tareas deterministas sobre
  fixtures, sin red) en 4 familias —operaciones de archivo, búsqueda y patch
  preciso, comandos y diagnóstico, disciplina (aprobaciones, solo-lectura,
  recuperación de error)— graduadas por asserts + comandos, con arnés
  headless que puntúa por tarea. **Anti-sobreajuste:** las tareas se
  congelan antes de F4; F4 no puede editar tareas, solo el agente; se
  reserva un 25% como conjunto ciego que solo se corre al final.
- **F4 — Corrección y re-medición.**
  Top de F1/F2/F3 por impacto; fixes en `core` (contrato/tools), nunca
  parches en el arnés para pasar; segunda pasada de flujo + bench con
  antes/después numérico en la completada.
- **F5 — Regresión periódica manual (solo si F4 cierra).**
  Subconjunto corto (~5 tareas) de cadencia manual; nunca puerta del gate.

## 4. SOLID y dónde vive cada cosa

- El arnés (runner, graduadores, fixtures) es herramienta de medición, no
  producto: vive fuera de `core`/`cli` como módulo propio de evaluación
  (ubicación exacta a decidir en F3; si solo hay scripts + fixtures, sin
  crate nuevo — YAGNI).
- Los fixes van al contrato (`core`: tools, permisos, truncado, errores) o
  al consumidor (`cli`), nunca al arnés para maquillar el número.
- Graduadores puros por tarea (una responsabilidad cada uno); el runner solo
  orquesta, puntúa y registra (modelo, versión, costo, seed si existe).

## 5. Eficiencia, costo y riesgos con mitigación

- Mini-bench antes que benchmarks públicos: una tarea propia cuesta
  céntimos y minutos; SWE-bench completo cuesta ordenes más y exige Docker.
- **Costo:** cada número publicado lleva su costo (modelo + llamadas);
  topes F0 cortan runs desbocados.
- **No-determinismo:** protocolo ×3 del F0; un defecto que aparece 1/3 se
  registra como fluctuante, no como sólido.
- **Seguridad (runs autónomos):** jaula F0 + supervisión; si una tarea
  necesita red o escritura fuera, se rediseña o se cae.
- **Sobreajuste:** congelado + conjunto ciego (§3 F3); quien fija puede
  proponer tareas nuevas, nunca editar las congeladas.

## 6. Criterios de aceptación («sólido» = números)

- Flujo F2: 11/11 pasos en ≥2 de 3 runs, con modelo y costo registrados.
- Bench: puntuación publicada antes/después; el conjunto ciego solo se abre
  una vez y su número acompaña al principal.
- Cada defecto F4 lleva reproducción mínima + fix + test; sin eso no cierra.

## 7. Gate, evidencia y documentación

- Gate canónico `sentinel check 119A-7 --stages
  scripts/quality/stages.json` PASS en fases con código; `cargo test` +
  clippy del arnés; el bench NO entra al gate.
- Evidencia en `Agente/completados/` (matriz F1, tabla 11 pasos, tabla
  bench antes/después con modelo y costo); prevención en
  `Agente/prevencion/` si aparece un modo de fallo repetible (jaula,
  fluctuación, sobreajuste); roadmap se actualiza al cerrar cada fase.

## 8. Riesgos abiertos

Modelo base que cambia bajo los pies (fijar versión mitiga, no elimina);
fixtures que se vuelven obsoletos; referencias que divergen de su upstream.

## 9. Estado y siguiente paso

- F0-jaula cerrada 11-09 (commit `32bf6b6`, gate PASS 446 ok).
- F1 evidencia completada 11-09 (§11); pendiente gate F1 + commit.
- Siguiente: F0-protocolo ×3 + F2 flujo 11 pasos — BLOQUEADO por
  credencial de modelo (`LlavesProveedor::from_env` no consume
  `GEMINI_API_KEY`; resto de keys ausentes).

## 10. Reto F0/F1 (11-09, verificado contra el código)

Sin código tocado; correcciones al plan antes de arrancar:

1. **§2 inventario impreciso.** Las tools viven en
   `archivo/{tools_archivo.rs,content_search.rs}`, `comando.rs`,
   `repo_map.rs`, `skill.rs`, `tareas.rs`, `todo.rs`, `tools_web.rs`,
   `mcp.rs`, `tool.rs` (registro+permisos) y dir `navegador/`. F1 debe
   partir de este inventario, no del listado aproximado de §2.
2. **Compactación YA existe** (no es gap): automática por ocupación
   (`nucleo/context.rs`, anti-thrash, piso 512K), manual `/compactar`
   (`turno/mod.rs:460`), hooks `PreCompact` vetables (`hooks.rs:84`).
   F2-paso 9 mide su calidad, no su existencia.
3. **Rewind a nivel archivo YA existe pero SIN exponer**: `historial.rs`
   (`tomar_checkpoint`/`revertir_ultimo`, pila LIFO acotada, fail-closed
   fuera del sandbox) solo lo usa `aplicar_plan_con_checkpoint`
   (`plan.rs:169`) + tests. F2-paso 8 debe fijar ANTES el driver
   (¿tool `deshacer`? ¿comando CLI? ¿vía shell enjaulado?) — candidato
   F4 ya visible: exponer undo al modelo.
4. **Supuesto §2 resuelto: el tick es solo manual.** `daemon.rs` es un
   daemon de sesiones TCP NDJSON en loopback, NO un loop de
   scheduler; `schedule run`/`ciclo_scheduler` solo corren a mano
   (el loop único en daemon es 119A-6 F3, pendiente). F0/F2 no pueden
   asumir disparos periódicos.
5. **Jaula F0 implementada (11-09, tarde)**: `EjecutorCliente::en_raiz`
   (`cli/src/infra/ejecutor.rs`) fija el cwd de arranque de cada hijo
   (síncrono + fondo); `run` lo cablea al workspace (`--dir` o cwd,
   `run.rs`). Humo: `en_raiz_arranca_los_comandos_en_la_jaula`
   (tempdir + sonda `cd`/`pwd`). Límite documentado en el propio
   módulo: el shell puede hacer `cd` fuera (sin namespaces en
   Windows); la contención total = cwd fijado + clasificación de
   riesgo + aprobación + supervisión. `nuevo()` sin raíz queda solo
   para diagnósticos sin run (listado de tools, sesiones daemon).
6. **Aprobaciones existen**: flujo plan→aprobar con checkpoint
   (`plan.rs` e2e `e2e_aprobar_con_checkpoint_y_undo`). F2-paso 7 lo
   ejerce, no lo construye.

Veredicto del reto: el plan sigue **VIABLE**; F0/F1 arrancan con los
puntos 1–6 incorporados. Sin cambios en fases, gate ni DoD.

**SIGUIENTE ACCIÓN:** arrancar F0 (jaula + protocolo) y luego F1 (matriz,
solo lectura). **AUTORIZADO PARA EJECUTAR** el ciclo local (investigar,
editar, probar, gate, commit) cuando se arranque; nunca deploy ni
escrituras fuera de la jaula; SSH prohibido siempre.

## 11. F1 matriz contrato (11-09, solo lectura, sin código tocado)

Ejes §2 × 5 referencias. Rutas de evidencia bajo
`data/referencias-cli/<ref>/` (intocable).

**Herramientas.** Glory: registro+permisos en `tool.rs`, `mcp.rs`,
`skill.rs`, `navegador/`, planes/todo/tareas (53 `impl AgentTool`).
opencode `packages/opencode/src/tool/` (bash,read,edit,write,glob,
grep,list,patch,todowrite,todoread,webfetch,websearch,task,skill) con
descripción+límite+truncado. grok `src/tools/` (bash,file,grep,
schedule,computer). claurst `src-rust/crates/tools/src/` (41 ficheros:
agent_tool,apply_patch,ask_user,batch_edit,brief,bundled_skills,
computer_use,config_tool,cron,enter/exit_plan_mode,file_*,
formatter,glob,goal_complete,grep,lsp_tool,mcp_auth,mcp_resources,
monitor_tool,notebook_edit,powershell,pty_bash,remote_trigger,
repl_tool,send_message,skill_tool,sleep,synthetic_output,tasks,
team_tool,todo_write,tool_search,web_fetch,web_search,worktree).
hermes `tools/*.py` con auto-descubrimiento (`registry.py`,
`discover_builtin_tools`, `check_fn` reachability, todo toolset
debe nombrar la tool para exponerla). vscode `LanguageModelToolsService`
+ toolsets por modelo (`chatToolPicker.ts`, `toolSetsContribution`).
Pareja candidata F4: `monitor_tool`, `lsp_tool`, `worktree` (glory YA
tiene `ask_user`: `contrato/evento.rs:88`, `cli/src/ui/chat.rs:457).

**Delegación/subagentes — GAP GRANDE.** opencode `task` (subagentes).
grok `agent/delegations.ts` + hooks `SubagentStart/Stop`, hijos en modo
ask (`agent.ts:1212`). hermes `delegate_tool.py` (roles leaf/
orchestrator, `max_spawn_depth` 2, `max_concurrent_children` 3,
completions async, handoff de procesos al padre). claurst `team_tool.rs`
+ `send_message.rs` + `remote_trigger.rs`. Glory: nada equivalente.

**Permisos/allowlist — GAP PARCIAL.** opencode allow/ask/deny +
denylist de paths en sandbox. hermes `approvals suggest/test/--apply`
(mina session DB → propone `command_allowlist`, dry-run del veredicto
contra guards reales: blocklist, deny rules, dangerous-patterns,
yolo/off). vscode include/exclude de toolsets (`chatActions.ts:1607`).
Glory tiene clasificación+aprobación pero sin allowlist persistente,
sin dry-run, sin minería de aprobaciones pasadas.

**Modos — GAP GRANDE.** grok `AgentMode agent|plan|ask`
(`types/index.ts:244`). vscode `ChatModeKind Ask|Edit|Agent`
(`common/constants.js`). claurst `enter/exit_plan_mode.rs`. opencode
modos build/plan. Glory: sin modos.

**Sandbox.** Glory F0: cwd fijado (ver §10.5). grok `SandboxMode
off|shuru` (`utils/settings.ts:16`) + `workspace-trust.ts`; `BashTool`
con cwd propio, fondo máx 8 con logPath (`tools/bash.ts:11-12`).
opencode sandbox = timeout/workdir/denylist. hermes backends
local/docker/ssh/modal/daytona/singularity
(`tools/terminal_tool_backends.py`, `environments/`). Pareja F4:
perfiles de sandbox; backends remotos descartados (CLI local).

**Skills — GAP (verificar `skill.rs`).** opencode `skills/` + frontmatter
+ `skills.sh`. grok `utils/skills.ts`. hermes `skills/` +
`agent/curator*.py` + gate `write_approval` (staging en
`pending/{memory,skills}/` para review). claurst `bundled_skills.rs` +
`skill_tool.rs`. Glory `skill.rs` existe: F1 debe medir cobertura vs
esto, no asumir ausencia.

**Memoria — PARIDAD PARCIAL.** Glory tiene memoria. hermes MEMORY.md +
USER.md + `write_approval` + curator; opencode MEMORY.md. Pareja F4:
gate de escritura + curaduría, no el almacén.

**Compactación — PARIDAD.** Glory auto+manual+PreCompact (§10.2). grok
`agent/compaction.ts` + hooks Pre/PostCompact; hermes compression +
curator. F2-paso 9 mide calidad.

**Rewind — GAP PARCIAL (ya candidato §10.3).** Ninguna referencia
expone undo de archivos al modelo como primitiva con checkpoint;
glory lo tiene interno. F4: exponer driver.

**Cron — PARIDAD.** Glory `ScheduleTarea` (119A-6). grok
`tools/schedule.ts` (`ScheduleManager`). hermes `cronjob` tool + `cron/`.
claurst `cron.rs`.

**MCP — VERIFICAR (`mcp.rs` existe).** grok `buildMcpToolSet` +
`mcp/runtime`. hermes cliente MCP + catálogo. claurst crate `mcp/`.
opencode servidores MCP. Medir si `mcp.rs` glory es cliente completo
o solo tipos.

**Hooks — GAP PARCIAL.** Glory tiene hooks + PreCompact. grok:
SessionStart/End, UserPromptSubmit, TaskCreated/Completed,
SubagentStart/Stop, Stop(Failure), Notification. hermes gateway hooks
always-registered. Pareja F4: ampliar catálogo.

**Salida/truncado — VERIFICAR.** opencode truncado con marcas;
grok `MAX_TAIL_BYTES` 8192. Medir qué hace glory con salidas largas.

**Top-10 por impacto (candidatos F4, a confirmar en F2/F3):**
1 modos ask/plan/agent; 2 delegación task/subagentes; 3 skills +
curaduría + gate escritura; 4 allowlist/denylist + dry-run + minería;
5 MCP (si `mcp.rs` incompleto); 6 rewind expuesto; 7 hooks ampliados;
8 truncado con marcas (si falta); 9 sesiones multi + conteo tokens
(verificar vs `SessionStore` grok); 10 monitor/lsp/worktree
(menor; `ask_user` ya existe). Descartado: backends remotos, telemetría, skins.
