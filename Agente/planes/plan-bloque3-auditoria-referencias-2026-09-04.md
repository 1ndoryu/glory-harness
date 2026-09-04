# Plan: Auditoría de referencias — Bloque 3 de mejoras (Glory Harness + IA de Tasks)

- **Fecha:** 04-09-2026 · **Área:** glory-harness + PROYECTO TASKS (IA de tasks)
- **Base:** planes `plan-mejora-agente-2026-09-03.md` (318A-15, F0–F6 ✅) y
  `plan-mejora-agente-2-2026-09-03.md` (318A-16, F1–F6 ✅), comparativa
  `Agente/documentacion/comparativa-opencode-agente-2026-09-03.md`
- **Referencias locales:** `data/referencias-cli/` (claurst, hermes-agent, grok-cli,
  opencode, vscode — solo lectura). **Cerradas (no clonadas):** Claude Code
  (cubierto por el spec clean-room de claurst `spec/`), Meta/Codex, Gemini CLI, Cursor, Aider.
- **Estado:** auditoría inicial hecha (04-09). Este documento es la **hoja de
  trabajo**: checklists por referencia para marcar `[x]` y anotar, y al final el
  **Bloque 3 recomendado** (aún NO ejecutado).
- **Progreso Bloque 3:** Fase 1 ✅ completa (`37d6d83` + `318A-17 (B3-F1)` —
  guardas puras, `web_fetch` con proveedor HTTP del CLI, `ask_user` con evento
  `Pregunta` y guardas aplicadas en el runtime). Fases 2–8 pendientes.
- **IDs sugeridos** para las fases del Bloque 3: `049A-N` (verificar contra
  `Agente/completados/` y `roadmap` antes de asignar).

---

## 0. Cómo usar este documento

1. Cada §2–§7 es un **checklist de auditoría** con casillas `[ ]`. Se revisa la
   referencia (paths de evidencia incluidos), se marca lo que **vale la pena
   replicar** y se anota en la columna/comentario la decisión:
   - **[YA]** ya existe en GH/PT (con path) → no duplicar.
   - **[REPLICAR]** hueco real → entra en la lista del Bloque 3.
   - **[EVALUAR]** útil pero requiere decisión (alcance, arquitectura, cuenta).
   - **[NO]** fuera de alcance con razón (anotar la razón).
2. Los checklists están pensados para **re-recorrerse** cuando cambie el código de
   una referencia (los clones son `--depth 1`; actualizar con `git fetch` si se
   quiere la última versión).
3. Al final (§9) está el **Bloque 3 recomendado**: la lista priorizada de mejoras
   que sale de esta auditoría. Cuando el usuario lo apruebe, cada fase se convierte
   en una sección ejecutable con su checklist propio (patrón de los planes 1 y 2).

---

## 1. Línea base — qué tiene hoy Glory Harness (verificado 04-09)

Fuentes: `glory-harness/core/src/*.rs`, `glory-harness/cli/src/*.rs`.

| Área | Estado real | Paths |
|---|---|---|
| Prompt por capas `[ENTORNO]`/`[REGLAS]` protegidas en compactación | ✅ | `core/src/context.rs` |
| Reglas v2: categorías + patrones, fail-closed, wildcard | ✅ | `core/src/regla.rs` |
| Permisos ask/allow/deny por tool, overrides, deny sin reintento, schema oculto | ✅ | `core/src/permiso.rs` |
| Aprobación 3 botones + "permitir siempre" por categoría (UI Tasks) | ✅ | `core/src/aprobacion.rs`, frontend PT |
| Tool `comando`: clasificador de riesgo, timeout/fondo/truncado | ✅ | `core/src/comando.rs`, `bash_clasificar.rs`, `cli/src/ejecutor.rs` |
| `file_read` por rangos, `file_write`/`file_patch` con unicidad de snippet | ✅ | `core/src/tools_archivo.rs`, `sandbox.rs`, `diff.rs` |
| Todo tool (plan visible) | ✅ | `core/src/todo.rs` |
| Subagentes: tool `task`, perfiles, presupuesto, profundidad 1 | ✅ | `core/src/subagente.rs` |
| Modo plan: propuesta con diff → aprobar → aplicar una vez | ✅ | `core/src/plan.rs`, CLI `/plan` |
| Tareas programadas: cron v2, NL→cron, tool, `schedule` CLI | ✅ | `core/src/scheduler.rs`, `tareas.rs` |
| Compactación dirigida + fallback determinista | ✅ | `core/src/context.rs`, `runtime.rs` |
| Telemetría de turno + Usage | ✅ | `core/src/telemetria.rs` |
| **Tools de red** | ⚠️ **solo `web_search`** (puerto `WebSearchProvider`) | `core/src/tools_web.rs` |
| **MCP** | ❌ no existe | — |
| **Skills** (SKILL.md, scopes) | ❌ no existe (solo capa [REGLAS] desde AGENTS.md) | `cli/src/reglas.rs` |
| **Slash/custom commands** | ⚠️ hardcodeados en `chat.rs` (`/salir /nuevo /plan /ayuda`) | `cli/src/chat.rs:200-230` |
| **Hooks lifecycle** | ❌ no existe | — |
| **LSP** | ❌ no existe | — |
| **ask_user / pregunta al usuario** | ⚠️ solo aprobación de tools | `core/src/aprobacion.rs` |
| **Notificaciones OS** | ❌ no existe | — |
| **Sesiones: resume / lista / export** | ⚠️ hay `persistencia_sqlite.rs`; sin subcomando de sesiones | `cli/src/persistencia_sqlite.rs`, `main.rs` |
| **webfetch / leer URL** | ❌ no existe | — |
| **@-menciones / adjuntar archivo** | ❌ no existe | — |
| **Memoria de aprendizaje** | ⚠️ solo capa reglas estática; PT tiene memoria en BD | PT `migrations/…agente` |
| **Multi-proveedor/cuentas/uso** | ⚠️ llaves en config, sin cuentas ni coste | `core/src/llm.rs`, `config/` |

**Herramientas registradas hoy** (runtime): `web_search`, `todo`, `task` +
condicionales `comando*`, `programar_tarea`, tools de archivo (solo modo local) —
`core/src/runtime.rs:238-275`.

---

## 2. Checklist — claurst (Rust; spec clean-room de Claude Code)

Ruta: `data/referencias-cli/claurst`. Lo más cercano al stack (Rust + ratatui).
Docs canónicas en `claurst/docs/*.md`; spec conductual en `claurst/spec/00–13`.

### 2.1 Sistema de prompts y contexto
- [ ] `SYSTEM_PROMPT_DYNAMIC_BOUNDARY` — cómo delimita bloques dinámicos que la
      compactación no debe tocar (ya replicado en F1: `[ENTORNO]`/`[REGLAS]`). **[YA]**
- [ ] `context_collapse.rs` — colapso de contexto largo (qué se descarta/condensa
      antes de compactar). **[REPLICAR — cotejar con nuestra compactación F6]**
- [ ] `session_memory.rs`, `memory.rs` (comando `/memory`), `claudemd.rs` — memoria
      por proyecto (CLAUDE.md) gestionada por comando. **[EVALUAR]**
- [ ] `attachments.rs`, `keywords.rs` — adjuntos y palabras clave del prompt. **[EVALUAR]**

### 2.2 Herramientas (catálogo `crates/tools/src/*.rs`)
- [ ] `apply_patch.rs` / `batch_edit.rs` / `file_edit.rs` / `file_read.rs` /
      `file_write.rs` — familia de edición (comparar con nuestra
      `file_write`/`file_patch`). **[YA/parcial]**
- [ ] `ask_user.rs` — **preguntar al usuario con opciones durante un turno**.
      **[REPLICAR]**
- [ ] `glob_tool.rs` / `grep_tool.rs` / `formatter.rs` — búsqueda y formateo. **[YA/EVALUAR]**
- [ ] `lsp_tool.rs` + `core/lsp.rs` — **inteligencia LSP como herramienta**
      (definición, referencias, símbolos). **[REPLICAR (alto esfuerzo)]**
- [ ] `monitor_tool.rs` — vigilar archivos/procesos. **[EVALUAR]**
- [ ] `cron.rs` (tool) — programar desde el agente (ya tenemos
      `programar_tarea`). **[YA]**
- [ ] `enter_plan_mode.rs` / `exit_plan_mode.rs` — plan mode como tools explícitas
      (ya tenemos modo plan). **[YA]**
- [ ] `goal.rs` / `goal_complete.rs` + `query/goal_loop.rs` — **objetivos de larga
      duración con loop autónomo**. **[EVALUAR — bloque futuro]**
- [ ] `mcp_auth_tool.rs` / `mcp_resources.rs` — tools para gestionar MCP y leer
      recursos MCP. **[REPLICAR con MCP]**
- [ ] `computer_use.rs` / `notebook_edit.rs` — uso de pantalla/notebooks. **[NO]**
- [ ] `bundled_skills.rs` + `query/skill_prefetch.rs` — skills empaquetadas y
      precarga proactiva. **[REPLICAR con skills]**
- [ ] `config_tool.rs` — el agente lee/escribe su propia configuración. **[EVALUAR]**
- [ ] `agent_tool.rs` (tools y query) — subagentes (ya tenemos `task`). **[YA]**
- [ ] `brief.rs` — contexto breve del proyecto para el agente. **[EVALUAR]**

### 2.3 MCP (`crates/mcp/`)
- [ ] `connection_manager.rs` / `rmcp_backend.rs` — cliente MCP (transporte,
      conexiones múltiples). **[REPLICAR]**
- [ ] `oauth.rs` / `registry.rs` — auth OAuth para servidores remotos y registro.
      **[EVALUAR]**
- [ ] `crates/mcp/backend.rs` — modelos de recursos/herramientas MCP. **[REPLICAR]**
- [ ] Doc `docs/mcp.md` — espec de qué expone. **[leer antes de diseñar]**

### 2.4 Hooks y plugins
- [ ] `docs/hooks.md` — **el catálogo completo**: eventos `PreToolUse`,
      `PostToolUse`, `PostToolUseFailure`, `Stop`, `StopFailure`,
      `UserPromptSubmit`, `Notification` (tipos `permission_prompt`,
      `idle_prompt`, `auth_success`, `elicitation_*`), `SessionStart/End`,
      `SubagentStart/Stop`, `PreCompact/PostCompact`, `PermissionRequest`.
      Tipos de hook: `command`, `prompt` (evaluación LLM), `agent` (verificador
      agéntico), `http` (POST), filtros con `if`. **[REPLICAR — diseño de
      referencia para hooks]**
- [ ] `crates/plugins/` — `manifest.rs`, `loader.rs`, `marketplace.rs`, `hooks.rs`,
      `plugin.rs`, `registry.rs` — **sistema de plugins cargables con mercado**.
      **[EVALUAR — después de hooks]**

### 2.5 Comandos y operación
- [ ] `crates/commands/src/` — `doctor`, `export` (sesión a markdown), `copy`,
      `memory`, `review` (auto-revisión), `permissions` (ver/editar reglas),
      `providers`/`accounts` (multi-proveedor + login OAuth/device-code),
      `remote` (sesiones remotas), `sandbox` (gestionar sandbox de permisos),
      `diagnostics`, `maintenance`, `named_commands` (comandos personalizados),
      `appearance` (tema), `keybindings`, `chrome`, `config_cmd`. **[chequear uno a
      uno: qué subcomandos replicar en nuestro `main.rs`]**
- [ ] `core/accounts.rs`, `auth_store.rs`, `cloud_session.rs`, `device_code.rs`,
      `codex_oauth.rs` — autenticación y cuentas. **[EVALUAR]**
- [ ] `core/analytics.rs`, `effort.rs`, `feature_flags.rs`, `file_history.rs`,
      `git_utils.rs`, `ide.rs` — analítica, esfuerzo (razonamiento), flags, undo de
      archivos, git, integración IDE. **[EVALUAR/REPLICAR parcial]**
- [ ] `core/keybindings.rs` — atajos configurables. **[EVALUAR — TUI]**
- [ ] `query/away_summary.rs`, `continuation.rs`, `auto_dream.rs` — resúmenes en
      pausa/continuación. **[EVALUAR]**
- [ ] `query/managed_orchestrator.rs`, `managed_agents.rs`, `buddy/` — agentes
      gestionados por el usuario (rollos tipo "buddy"). **[EVALUAR]**
- [ ] `query/command_queue.rs` — cola de comandos. **[EVALUAR]**
- [ ] Spec `spec/02_commands.md`, `03_tools.md`, `05_…permissions.md`,
      `07_hooks.md` — **lectura obligatoria** al diseñar cada área (son el
      comportamiento de Claude Code destilado).

**Notas claurst (rellenar en cada pasada):**
```
[ ] ______________________________________________________________________
```

---

## 3. Checklist — opencode (TypeScript; cliente/servidor, plugins, permisos)

Ruta: `data/referencias-cli/opencode`. La comparativa ya documenta capas de
prompt, agents/modos, permisos y compactación (§2 y §11 del comparativa). Aquí solo
lo que **aún no hemos mirado/replicado**.

### 3.1 Configuración, comandos y skills
- [ ] `src/config/command.ts` + `src/command/template/` — **comandos slash
      personalizados en markdown con variables** (`$ARGUMENTS`, `@file`, etc.).
      Docs web: `opencode.ai/docs/commands` (markdown + YAML en `commands/`).
      **[REPLICAR — unificar con skills (Claude Code 2026 fusionó slash → skill)]**
- [ ] `src/skill/discovery.ts` — **descubrimiento de skills** (qué carpetas mira,
      formato). **[REPLICAR]**
- [ ] `src/tool/skill.ts` + `skill.txt` — herramienta `skill` para cargar una skill
      bajo demanda. **[REPLICAR]**
- [ ] `src/config/config.ts`, `parse.ts`, `paths.ts` — jerarquía de config
      global/proyecto/local. **[EVALUAR]**
- [ ] `src/config/variable.ts` — variables del prompt (`$FILE`, `$PROJECT`…).
      **[REPLICAR con comandos]**
- [ ] `AGENTS.md` por jerarquía (ya documentado). **[YA — F2]**

### 3.2 Cliente/servidor, plugins y hooks
- [ ] `src/plugin/` + `packages/plugin/` — **API de plugins npm** (hooks de ciclo
      de vida, tools, providers, tema). **[EVALUAR — después de hooks/MCP]**
- [ ] `src/plugin/install.ts`, `loader.ts` — instalación/carga. **[EVALUAR]**
- [ ] `src/plugin/github-copilot/` — adaptador Copilot. **[EVALUAR]**
- [ ] `src/mcp/` — cliente MCP (config, tools, auth). **[REPLICAR con claurst mcp]**
- [ ] `src/acp/` — **ACP (Agent Client Protocol)**. **[EVALUAR — interoperar con
      otros agentes]**
- [ ] `src/hooks` — hooks de sesión. **[cotejar con catálogo claurst]**
- [ ] `src/event-manifest.ts` / `event-v2-bridge.ts` — **eventos estructurados**
      (tenemos `evento.rs`; cotejar cobertura). **[YA/parcial]**

### 3.3 Herramientas y experiencia
- [ ] `src/tool/` — `read/write/edit/apply_patch/glob/grep` **[YA]**; `shell` +
      `shell/` (bash con sesión, TTY) **[YA parcial]**; `webfetch` + `websearch` +
      `mcp-websearch` **[REPLICAR webfetch]**; `lsp` **[EVALUAR]**; `task`
      (subagente) **[YA]**; `todo` **[YA]**; **`question`** (preguntar al usuario)
      **[REPLICAR]**; `truncate`/`truncation-dir` (recortar contexto/tools);
      `plan` con `plan-enter.txt`/`plan-exit.txt` (modo plan guiado por prompt)
      **[YA]**; `external-directory` (editar fuera del cwd con permiso)
      **[EVALUAR]**; `code-mode`, `json-schema`, `invalid` (manejo de errores).
- [ ] `src/ide/` + `lsp.ts` — integración IDE/LSP. **[EVALUAR]**
- [ ] `src/snapshot.ts` / `src/session/` / `src/share/` — **sesiones: lista,
      resume, snapshot, compartir**. **[REPLICAR sesiones/export; compartir NO]**
- [ ] `src/background` — tareas de fondo. **[EVALUAR]**
- [ ] `src/notification` (buscar) — notificaciones. **[REPLICAR con hooks]**
- [ ] `src/worktree/`, `src/sync/`, `src/git/` — worktrees y git. **[EVALUAR]**
- [ ] `src/agent/` — agentes config (primary/subagents en `agent.ts`). **[YA]**
- [ ] `packages/llm` — routing de modelos, caché, tokens. **[EVALUAR]**

**Notas opencode (rellenar):**
```
[ ] ______________________________________________________________________
```

---

## 4. Checklist — grok-cli (TypeScript/OpenTUI; UX, verificación, remoto)

Ruta: `data/referencias-cli/grok-cli`.

### 4.1 UX / TUI
- [ ] `src/ui/plan.tsx`, `slash-menu.tsx`, `agents-modal.tsx`, `mcp-modal.tsx`,
      `schedule-modal.tsx` — modales y menú slash. **[EVALUAR — nuestro TUI es
      texto simple; ratatui como claurst sería la vía]**
- [ ] `src/utils/at-mentions.ts` — **menciones `@`** (archivos, tools). **[REPLICAR]**
- [ ] `src/utils/skills.ts` — **skills con frontmatter** (`name`, `description`,
      scope `project|user`, `skillMdPath`). **[REPLICAR — formato de referencia]**
- [ ] `src/utils/instructions.ts` — **cadena de AGENTS.md** desde la raíz git hasta
      el cwd + global `.grok/AGENTS.md`, con hook `InstructionsLoaded`. **[YA/parcial]**
- [ ] `src/utils/side-question.ts` — pregunta lateral con opciones. **[REPLICAR con
      ask_user]**
- [ ] `src/utils/workspace-trust.ts` — confianza del workspace. **[EVALUAR]**
- [ ] `src/utils/host-clipboard.ts`, `install-manager.ts`, `update-checker.ts`.
      **[EVALUAR]**

### 4.2 Verificación (`src/verify/`) — destacado
- [ ] `orchestrator.rs`→`src/verify/orchestrator.ts`, `recipes.ts`, `evidence.ts`,
      `checkpoint.ts`, `retry.ts`, `entrypoint.ts`, `environment.ts` — **subsistema
      de auto-verificación: recetas (tests/build), evidencia, checkpoint entre
      intentos**. **[REPLICAR — candidato fuerte para Bloque 3/4]**
- [ ] `src/agent/compaction.ts` (con tests) **[YA — cotejar umbrales]**
- [ ] `src/agent/delegations.ts`, `subagent-display.ts`, `subagents-settings.ts`
      **[YA parcial]**
- [ ] `src/agent/reasoning.ts` — UI de razonamiento. **[EVALUAR]**
- [ ] `src/agent/vision-input.ts`, `media.ts` — entrada de imágenes. **[EVALUAR]**
- [ ] `src/tools/computer.ts` — uso de pantalla. **[NO]**
- [ ] `src/hooks/` (`config.ts`, `executor.ts`, `types.ts`) — hooks con config
      tipada. **[REPLICAR con claurst]**
- [ ] `src/mcp/` — `catalog.ts`, `validate.ts`, `parse-headers.ts` —
      **catálogo/validación de servidores MCP**. **[REPLICAR]**
- [ ] `src/lsp/` — manager + builtins (lenguajes incluidos). **[EVALUAR]**
- [ ] `src/daemon/scheduler.ts` + `src/tools/schedule.ts` — **daemon + tareas
      programadas con UI**. **[YA — cotejar worker]**
- [ ] `src/telegram/` — puente Telegram (audio, turnos). **[NO para GH; EVALUAR
      para IA Tasks/hermes]**
- [ ] `src/storage/` — sqlite: `sessions`, `transcript`, `usage`, `tool-results`,
      `workspaces`. **[EVALUAR — nuestra persistencia vs transcript/usage]**

**Notas grok (rellenar):**
```
[ ] ______________________________________________________________________
```

---

## 5. Checklist — hermes-agent (Python; la IA personal — memoria/skills/cron)

Ruta: `data/referencias-cli/hermes-agent`. Referencia directa para la **IA de
Tasks** (no para el núcleo de GH). Docs: `hermes-agent.nousresearch.com/docs`.

### 5.1 Memoria y aprendizaje (lo más diferencial)
- [ ] `agent/memory_manager.py` — **orquestador de memoria**: providers,
      `build_system_prompt()`, `prefetch_all(user_msg)` **antes** del turno,
      `sync_all(...)` **después**. **[REPLICAR (diseño) en IA Tasks]**
- [ ] `agent/curator.py`, `curator_backup.py`, `insights.py`, `learning_graph.py`,
      `learning_mutations.py`, `learn_prompt.py` — **loop de aprendizaje que crea
      skills desde la experiencia y recuerda**. **[EVALUAR — fase avanzada]**
- [ ] `skills/` — árbol con `SKILL.md` (categorías: apple, devops,
      software-development, research…) + `skill_bundles.py`, `skill_commands.py`,
      `skill_preprocessing.py`. **[REPLICAR formato; categorías por dominio]**
- [ ] `agent/prompt_cache_boundary.py` / `prompt_cache_scope.py` — respetar
      límites de caché de prompt. **[EVALUAR]**

### 5.2 Contexto y calidad de turno
- [ ] `context_engine.py`, `context_breakdown.py`, `context_references.py`,
      `coding_context.py` — gestión fina de contexto. **[EVALUAR]**
- [ ] `conversation_compression.py`, `native_compaction.py`,
      `manual_compression_feedback.py`, `compaction_display.py`,
      `trajectory_compressor.py` — compactación con feedback manual. **[YA/parcial]**
- [ ] `turn_summary.py`, `title_generator.py`, `repetition_guard.py`,
      `empty_response_guard.py`, `bounded_response.py`, `think_scrubber.py`,
      `deadline.py`, `estop.py`, `iteration_budget.py`, `turn_liveness.py`,
      `reasoning_timeouts.py` — **salvaguardas de turno** (respuesta vacía,
      repetición, límites, parada de emergencia). **[REPLICAR las baratas:
      empty_response/repetition/deadline]**
- [ ] `side_question.py`, `plan_prompt.py`, `fast_mode.py`, `reasoning_effort.py`,
      `reasoning_summaries.py`. **[EVALUAR]**
- [ ] `file_safety.py`, `redact.py`, `secret_scope.py`, `ssl_guard.py`,
      `tool_guardrails.py`, `tool_result_classification.py`. **[EVALUAR —
      endurecimiento]**

### 5.3 Verificación y revisión
- [ ] `review_engine.py`, `background_review.py`, `review_idle_queue.py`,
      `verify/`, `verify_hooks.py`, `verification_evidence.py`,
      `verification_stop.py` — **revisión en segundo plano + verificación con
      evidencia**. **[EVALUAR con grok verify]**
- [ ] `agent/lsp/` — integración LSP. **[EVALUAR]**

### 5.4 Cron, mensajería y notificaciones
- [ ] `cron/` — `scheduler.py`, `jobs.py`, `delivery_queue.py`, `notepad.py`,
      `incidents.py`, `suggestions.py`, `lifecycle_guard.py`, `monitor.py` — **jobs
      que ejecutan al agente y entregan el resultado a una plataforma**.
      **[YA/parcial — nuestro `schedule` ejecuta comandos; falta "correr un turno
      de agente y entregar resumen"]**
- [ ] `gateway/` (Telegram/Discord/Slack) + `outbound_webhooks.py`,
      `reactions.py`, `subscription_view.py`, `notifications` — canales y avisos.
      **[NO para GH; EVALUAR para IA Tasks]**
- [ ] `plugins/` — `browser`, `image_gen`, `memory`, `observability`,
      `model-providers`, `cron_providers`, `kanban`… **[EVALUAR/NO]**

### 5.5 Proveedores, cuentas y coste
- [ ] `agent/*adapter.py` (anthropic, gemini_native, bedrock, azure, vertex,
      codex, copilot ACP, LMStudio…), `credential_pool.py`, `usage_pricing.py`,
      `credits_tracker.py`, `rate_limit_tracker.py`, `nous_rate_guard.py` —
      **multi-proveedor con pool de credenciales y control de coste**. **[EVALUAR —
      GH tiene `llm.rs` con proveedores básicos]**

**Notas hermes (rellenar):**
```
[ ] ______________________________________________________________________
```

---

## 6. Checklist — vscode (agente del editor)

Ruta: `data/referencias-cli/vscode` (sparse: `src/vs/workbench/contrib/chat`).
Aplica sobre todo si algún día GH se integra en un editor/desktop.

- [ ] `common/chatModes.ts` — modos ask/edit/build. **[YA — nuestros modos]**
- [ ] `browser/agentSessions/` — sesiones del agente (checkpointing por sesión,
      rehacer). **[EVALUAR — con sesiones]**
- [ ] `common/chatModel.ts`, `electron-browser/` — modelo y runtime del chat en el
      editor. **[NO directo]**
- [ ] Plan agent del editor (checkpoints git) — comportamiento conocido vía web.
      **[EVALUAR con checkpoint/undo]**
- [ ] Editor tooling: `apply_edit` del editor + terminal integrado. **[NO directo]**

**Notas vscode (rellenar):**
```
[ ] ______________________________________________________________________
```

---

## 7. Referencias cerradas (web, sin clonar) — qué chequear

| Referencia | Qué aporta | Dónde verificarlo |
|---|---|---|
| **Claude Code** (Anthropic) | CLAUDE.md, permisos allow/deny, plan mode, checkpoints, skills (slash fusionado), hooks (incl. Notification), MCP, subagentes, auto-compact, output styles, `/doctor` `/review` `/export` | `docs/` web + spec de claurst |
| **Codex CLI** (OpenAI) | modos de aprobación (read-only/auto/full), sandbox OS, apply_patch, web | docs web |
| **Gemini CLI** | skills dirs, hooks, media | docs web |
| **Aider** | **repo map** (índice del repo por relevancia), modo architect, watch, `/undo` | docs web |
| **Cursor / Cline / Roo** | checkpoints con git shadow, modo plan/act, MCP marketplace, browser | docs web |

- [ ] Output styles / formatos de respuesta (Claude Code) — **[EVALUAR]**
- [ ] Repo map estilo Aider (resumen de símbolos por relevancia) — **[EVALUAR —
      alternativa a LSP completa]**
- [ ] Checkpoints con git shadow (Cline/Roo/Codex) — **[REPLICAR con undo]**
- [ ] Sandbox OS-level — **[NO — decidido en plan 1 §8]**

---

## 8. Mapa de huecos "olvidados" detectados (resumen de la auditoría)

Cosas que **ninguno de los planes 1–2 cubrió** y que las referencias sí tienen:

| # | Capacidad | Evidencia en referencias | Verdict |
|---|---|---|---|
| 1 | **MCP cliente** (servidores stdio/HTTP, tools MCP con permisos, fail-closed) | opencode `src/mcp/`; claurst `crates/mcp/` (+oauth, resources); grok `src/mcp/catalog.ts` | **[REPLICAR — el mayor hueco]** |
| 2 | **Skills** (markdown frontmatter `name/description`, scopes proyecto/usuario, descubrimiento, precarga) | grok `utils/skills.ts`; opencode `src/skill/`; hermes `skills/`; claude skills | **[REPLICAR]** |
| 3 | **Comandos slash personalizados** (plantillas markdown con variables, unificados con skills) | opencode `config/command.ts`; claude (slash→skill); claurst `named_commands.rs` | **[REPLICAR]** |
| 4 | **Hooks de ciclo de vida** (Pre/PostToolUse, Notification, SessionStart/End, Subagent*, Pre/PostCompact, PermissionRequest; tipos command/prompt/agent/http) | claurst `docs/hooks.md` + `spec/07`; grok `src/hooks/`; opencode plugin hooks | **[REPLICAR]** |
| 5 | **ask_user / pregunta con opciones** en medio del turno | claurst `tools/ask_user.rs`; opencode `tool/question.ts`; grok `side-question.ts` | **[REPLICAR — barato, alto valor]** |
| 6 | **webfetch** (leer una URL → texto/markdown) | opencode `tool/webfetch.ts` | **[REPLICAR — barato]** |
| 7 | **Sesiones: lista/resume/export** (markdown) + continuar tras crash | opencode `src/session/`, `share/`; claurst `commands/export.rs`; grok `storage/transcript.ts` | **[REPLICAR sobre persistencia_sqlite]** |
| 8 | **Checkpoint/undo** (git por cambio, rehacer) | claude checkpoints; grok `verify/checkpoint.ts`; claurst `file_history.rs` | **[REPLICAR]** |
| 9 | **Notificaciones** (OS + evento Notification del hook) | claude hooks Notification; opencode notification | **[REPLICAR con hooks]** |
| 10 | **Salvaguardas de turno**: respuesta vacía, repetición, deadline/estop | hermes `empty_response_guard.py`, `repetition_guard.py`, `deadline.py` | **[REPLICAR las baratas]** |
| 11 | **LSP ligero / repo map** (definiciones, símbolos relevantes) | claurst `lsp.rs`/`lsp_tool.rs`; opencode `tool/lsp.ts`; Aider repo map | **[EVALUAR — esfuerzo alto; alternativa repo map]** |
| 12 | **@-menciones / adjuntar contexto** (`@archivo`) | grok `utils/at-mentions.ts`; opencode variables | **[REPLICAR]** |
| 13 | **Memoria de aprendizaje** (curator, prefetch por turno, auto-skills) | hermes `memory_manager.py`, `curator.py` | **[EVALUAR — fase avanzada IA Tasks]** |
| 14 | **Auto-verificación** (recetas, evidencia, retry) | grok `src/verify/`; hermes `review_engine.py` | **[EVALUAR — Bloque 4]** |
| 15 | **Coste/uso por sesión** y límites | hermes `usage_pricing.py`; opencode usage | **[EVALUAR]** |
| 16 | **Cron que ejecuta al agente** (no solo comandos) y entrega resumen | hermes `cron/jobs.py` | **[EVALUAR sobre nuestro schedule]** |
| 17 | **Plugins cargables / marketplace** | claurst `crates/plugins/`; opencode `src/plugin/` | **[EVALUAR — después de hooks/MCP]** |
| 18 | **Temas/atajos configurables en TUI** | claurst `appearance.rs`/`keybindings.rs` | **[EVALUAR]** |
| 19 | **Multi-cuenta OAuth + device code** | claurst `core/accounts.rs` | **[EVALUAR]** |
| 20 | **Reasoning effort/UI, output styles** | grok `agent/reasoning.ts`; claude output styles | **[EVALUAR]** |

---

## 9. Bloque 3 recomendado (propuesta — aún NO ejecutada)

Prioridad por **valor ÷ esfuerzo** y por dependencias. Cada fase seguirá el flujo de
los planes 1-2 (checklist propio, tests deterministas, clippy, gate Sentinel,
commit por fase). Mover a ejecución solo tras aprobación del usuario.

### Fase 1 — `ask_user` + `webfetch` (barato, alto valor inmediato) ✅
- [x] Tool `ask_user`: pregunta con opciones y respuesta acotada, evento SSE
      `Pregunta`, integración CLI (`chat.rs` muestra la pregunta con opciones).
      `core/src/pregunta.rs` (tool + `procesar_pregunta` libre determinista),
      intercepción en `runtime.rs` (patrón `task`, termina el turno, la respuesta
      del usuario llega como siguiente mensaje). Tests: `intercepcion_valida_emite_y_registra`,
      `intercepcion_exige_texto`, `canal_pregunta_registra_y_responde_una_vez`.
      La UI de PT se integra en una fase posterior (evento ya disponible).
- [x] Tool `webfetch`: leer URL → texto limpio (límite bytes/título), distinta de
      `web_search`. `core/src/tools_web.rs` (`ToolWebFetch`) + puerto
      `WebFetchProvider` en `ports.rs` + proveedor HTTP real en
      `cli/src/fetch.rs` (reqwest, HTML→texto sin scripts/estilos, errores HTTP
      propagados). Commit `37d6d83`.
- [x] Salvaguardas baratas: respuesta vacía → reintento único con aviso; detector
      de repetición. `core/src/guardas.rs` (puro, determinista) aplicadas en la
      finalización del turno de `runtime.rs` (aviso de repetición anexado;
      reintento único con `aviso_vacio`; configurables via `set_guardas`,
      activas por defecto). Tests: 11 en `guardas.rs` + aplicación en runtime.

### Fase 2 — MCP cliente (núcleo)
- [x] Puerto `McpProveedor` (stdio; HTTP evaluar) + registry de tools MCP con
      **permisos del modelo F3** (categoría `mcp`, efecto=true → ask en
      predeterminado; deny silencioso por categoría oculta las tools del
      schema), fail-closed sin proveedor, sin SQL. `core/src/ports.rs`
      (`McpHerramienta`+`McpProveedor`), `core/src/regla.rs` (`CAT_MCP`),
      `core/src/mcp.rs` (`McpProveedorStdio` JSON-RPC 2.0 línea a línea con
      `initialize`+`tools/list`+`tools/call`, `ToolMcpAdapter`,
      `sanitizar_id`), `tool.rs` (`registrar_mcp`, ids dinámicos `String`).
- [x] Config por consumidor (CLI ✓): `GLORY_MCP_CONFIG` (JSON
      `[{nombre,comando,argumentos}]`) en `cli/src/mcp_cli.rs`, negociación con
      timeout 10 s y error de arranque propagado (fail-closed); construcción
      del chat async (`construir_harness`). PT: no registra MCP en esta fase
      (su IA no ejecuta tools remotas; invariante task-IA sin ejecución) — la
      superficie env queda disponible para el consumidor que la adopte.
- [x] E2E determinista con servidor MCP stub (guion, sin proceso real):
      registro en registry → ids `mcp_*` en schema con descripción+schema del
      servidor, deny `mcp:*` los oculta, ejecución vía adapter devuelve el
      texto del servidor. 6 tests en `mcp.rs`. Evidencia: claurst
      `crates/mcp/`, opencode `src/mcp/`, grok `mcp/validate.ts`.

### Fase 3 — Skills + comandos slash personalizados unificados
- [ ] Descubrimiento de skills: carpetas de skills de proyecto y usuario, formato
      markdown frontmatter (`name`, `description`, scope), índice en contexto.
      Evidencia: grok `utils/skills.ts`, opencode `src/skill/discovery.ts`.
- [ ] Tool `skill` para cargar una skill bajo demanda + precarga opcional.
- [ ] Comandos slash **definidos como markdown** (plantilla + variables
      `$ARGUMENTOS`, `@archivo`) — sustituir el hardcode de `chat.rs` manteniendo
      `/ayuda /salir /plan` (patrón claude: slash = skill). Evidencia: opencode
      `config/command.ts`, claude skills.
- [ ] Alimentar la capa [REGLAS]/memoria de la IA de Tasks desde el mismo formato
      (skills de dominio propias).

### Fase 4 — Hooks de ciclo de vida
- [ ] Eventos: `PreToolUse`, `PostToolUse`, `Stop`, `UserPromptSubmit`,
      `SessionStart/End`, `SubagentStart/Stop`, `Pre/PostCompact`,
      `PermissionRequest` — emitidos por el núcleo sin acoplar a un runner.
- [ ] Tipos de hook: `command` (proceso con timeout) y `http` (POST); `prompt`
     /`agent` diferidos.
- [ ] `Notification`: hook + notificación OS opcional (cli) al terminar turno/
      pedir permiso. Evidencia: claurst `docs/hooks.md` + `spec/07_hooks.md`.

### Fase 5 — Sesiones y export
- [ ] Subcomando `session` (list/ver/resume/borrar) sobre `persistencia_sqlite`;
      resume de conversación con contexto recompuesto.
- [ ] `export` (conversación/turno → markdown con decisiones y eventos).
      Evidencia: claurst `commands/export.rs`, opencode `src/session/`.

### Fase 6 — Checkpoint/undo (git)
- [ ] Antes de aplicar un plan (F5 del plan 2) o cambios de tools de archivo:
      snapshot git opcional (stash-less: commit local `glory-harness/checkpoint`)
      o copia de archivos tocados; `undo` del último checkpoint. Evidencia: grok
      `verify/checkpoint.ts`, claude checkpoints, claurst `file_history.rs`.
- [ ] Integrar con el modo plan: aprobar deja checkpoint recuperable.

### Fase 7 — LSP ligero o repo map (decisión)
- [ ] Decidir A vs B: (A) cliente LSP opcional por lenguaje con tool
      `definicion`/`simbolos`; (B) **repo map** tipo Aider (índice de símbolos por
      relevancia alimentado por `grep`-like). Evidencia: claurst `lsp_tool.rs`,
      opencode `tool/lsp.ts`, Aider.
- [ ] La opción B es más barata y agnóstica; la A es más precisa. Recomendación
      inicial: **B** en núcleo, A como plugin futuro.

### Fase 8 — IA de Tasks: cron de agente + memoria de aprendizaje (diseño)
- [ ] Cron que **ejecuta un turno de agente** (no solo comando) y entrega resumen
      (extiende `schedule`). Evidencia: hermes `cron/jobs.py` + `delivery_queue.py`.
- [ ] Diseño (solo diseño) de memoria de aprendizaje: `prefetch` antes del turno,
      `sync` después, extracción de preferencias/decisiones a skills/memoria.
      Evidencia: hermes `memory_manager.py`, `curator.py`.

### Quedan fuera de este bloque (con razón)
MCP oauth remoto y marketplace de plugins (después de F2/F4), computer use,
notebooks, multi-mensajería (Telegram/Discord), sandbox OS, i18n, media
(imagen/audio), agentes "buddy"/goals autónomos, ACP.

---

## 10. Decisiones que necesita el usuario

1. **Orden y alcance del Bloque 3**: ¿todas las fases 1–8, o arrancar con 1–3
   (ask_user/webfetch + MCP + skills/comandos) que son las de mayor valor?
2. **MCP**: ¿cliente stdio primero (sí) y HTTP/SSE + oauth en fase posterior?
3. **LSP vs repo map** (Fase 7): mi recomendación es repo map.
4. **Hooks**: ¿solo `command` + `http` en v1, o también `prompt`/`agent`
   (verificadores LLM)?
5. **IA de Tasks**: ¿la memoria de aprendizaje entra en este bloque o queda como
   diseño para el siguiente?

## 11. Evidencia (paths de la auditoría inicial)

- `data/referencias-cli/README.md` — índice, rutas de interés por repo.
- claurst: `src-rust/crates/{core,tools,commands,mcp,plugins,query}/src/*.rs`,
  `docs/{hooks,mcp,plugins,agents,commands,configuration,keybindings}.md`,
  `spec/00_overview.md … 13_rust_codebase.md`.
- opencode: `packages/opencode/src/{mcp,plugin,config,command,skill,session,tool,
  agent,share,acp}/`, `src/tool/*.txt` (contratos), `packages/docs` (parcial).
- grok-cli: `src/{utils/{skills,instructions,at-mentions,side-question,
  workspace-trust}.ts, verify/, mcp/, hooks/, daemon/, ui/, tools/schedule.ts}`.
- hermes-agent: `agent/{memory_manager,curator,review_engine,verify,
  conversation_compression,empty_response_guard,…}.py`, `skills/`, `cron/`,
  `gateway/`, `plugins/`.
- vscode: `src/vs/workbench/contrib/chat/{common,chatModes,agentSessions}`.
- Web: `opencode.ai/docs/commands`, `code.claude.com/docs/en/skills`,
  `hermes-agent.nousresearch.com/docs`, comparativas 2026 (Claude Code features,
  ranking de agentes).
