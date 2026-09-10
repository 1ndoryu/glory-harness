# Plan 109A-4 — Comandos `/` estilo VS Code (`/compactar`, `/meta`)

> ID roadmap: **109A-4** · Fecha: 2026-09-10 · Estado: activo (F1 HECHO 10-09,
> catálogo v1 cerrado; F2–F4 pendientes)
> Origen: petición usuario — menú `/` como VS Code; `/compactar`; `meta` pasa de
> modo de ejecución a comando; luego relevar comandos útiles en las referencias.

## 1. Estado actual (verificado)

- Core ya define comandos slash como markdown (`core/src/herramientas/skill.rs:1-98`,
  patrón Claude/opencode): `.md` con `tipo: comando` + `comando: <nombre>`,
  plantilla con `$ARGUMENTOS` y `@archivo`, expansión pura y determinista
  (sin I/O ni LLM). Hay descubrimiento en directorio (`:100`).
- UI sin menú `/` (`entradaMontaje.ts:63` tiene `autocomplete='off'`).
- `meta` hoy es **modo global**: `permiso_por_modo("meta")`
  (`politica/permiso.rs:188`), `turno_config.modo == "meta"`
  (`nucleo/runtime/turno/permisos.rs:123`), segmento en `OPCIONES_EJECUCION`
  (`dominio/opciones.ts:82-93`).
- Compactación solo automática (`nucleo/context.rs`); hook previo en 109A-1.

## 2. Fases

### F1 — Relevamiento en referencias (HECHO 10-09, catálogo v1 cerrado)

Relevadas las 5 referencias en `data/referencias-cli/` (gitignored):

| Ref | Modelo de comando | Catálogo relevante | Lección para GH |
|-----|-------------------|--------------------|-----------------|
| VS Code (`vscode/src/vs/workbench/contrib/chat/`) | `IChatAgentCommand` por agente (`common/participants/chatAgents.ts:123`), + `IChatSlashCommandService`; `/compact` (`browser/actions/chatActions.ts:1127-1147`) + hook `PreCompact` (`common/promptSyntax/hookTypes.ts:18,98-100`) | `/compact` + comandos por agente | Comandos con ámbito por agente, no globales; `PreCompact` ya cubierto por 109A-1 |
| opencode (`opencode/packages/opencode/src/command/index.ts:22-44,70-88`) | `Info` (name/description/agent/model/source `command\|mcp\|skill`/template/subtask/hints) + `hints()` (`$ARGUMENTS`, `$\d+`); builtins `init`/`review` (review con `subtask:true`) + config + prompts MCP + skills | `init` (“guided AGENTS.md setup”), `review … defaults to uncommitted”` (subtarea) | Mapea a `ComandoSlash` markdown existente (`skill.rs:39-43`); `hints`/`subtask` se añaden tras v1 |
| claurst (`claurst/src-rust/crates/commands/src/lib.rs:113-132`, `tui/.../app/commands.rs:102-109`) | trait `SlashCommand` (name/aliases/description/help/hidden/execute→`CommandResult`) + ~100 impls; `intercept_*` (pantallas UI) vs `execute` (lógica); `/help` agrupado por categorías (`:560-572`); `/compact` pide resumen con instrucción (`:628-648`) | help/clear+new/compact/model/context/cost/diff+review/init/memory/skills/btw + 90 más | Forma del trait a portar a Rust core; UI solo filtra/inserta, ejecución siempre en core (único camino, testeable headless) |
| grok-cli (`grok-cli/src/ui/slash-menu.ts:1-27,29-64`) | `SlashMenuItem` (id/label/description/aliases) + `filterSlashMenuItems` (score exacto > prefijo > contiene, comando antes que descripción) | exit/help/agents/mcp/models/new/commit/review/verify/skills/btw/update… | Algoritmo de filtrado del menú F2 |
| hermes (`hermes-agent/acp_adapter/commands.py:44-66,88-95`) | `_COMMANDS` (name→help/desc/hint) + `_cmd_<name>`; desconocido → `None` (pasa al LLM); `/compress` manual aun con auto off (`:228-267`); `/context` muestra presión (`:161-213`) | help/model/tools/context/reset/compress/steer/queue/version | GH NO hereda el fall-through al LLM: comando desconocido → error explícito + `/ayuda` (prohibido silencio) |

Catálogo v1 CERRADO (8 comandos, nombres en español como la UI actual):

- `/ayuda [comando]` — lista agrupada + detalle (todos la tienen).
- `/modelo [id]` — ver/cambiar modelo (claurst+hermes+grok).
- `/compactar [instrucción]` — compactación bajo demanda (F3; claurst+hermes+vscode).
- `/contexto` — presión de contexto + umbral (hermes `/context`, claurst `/context`+`/cost`).
- `/limpiar` — vaciar conversación (claurst `/clear`, hermes `/reset`); `new` no aplica (GH create-on-write 069A-7).
- `/revisar [args]` — revisar cambios no commiteados por defecto (opencode `/review`, claurst `/review`+`/diff`).
- `/iniciar` — setup guiado `AGENTS.md` del proyecto (opencode `/init`, claurst `/init`).
- `/meta <texto>` — un turno solo-lectura como override (F4; específico GH, sin precedente en refs).

Diferidos a hijas (fuera de v1): gestión de skills, `btw` lateral, `steer`/`queue`
(turnos concurrentes que GH no tiene), commit/pr, MCP, agents, pantallas
config/theme, checkpoints/rewind (P2 ya existe por UI), coste detallado,
voz, teleport/remoto.

### F2 — Menú `/` en la entrada
- Detectar `/` al inicio del textarea (`componentes/entrada*`); menú flotante
  con filtrado por prefijo (algoritmo grok: exacto > prefijo > contiene,
  comando antes que descripción), navegación teclado (↑↓/Tab/Esc), inserción al elegir.
- Nuevo `componentes/menuComandos.ts` (≤300 líneas); iconos de `iconos.ts`,
  tokens en `variables.css`, sin literales. Datos: los 8 builtins del catálogo
  v1 + comandos markdown descubiertos del backend (nuevo IPC `comandos_listar`
  o vía `enviar_turno` con expansión previa). Desconocido → error explícito
  con sugerencia `/ayuda` (nunca fall-through silencioso al LLM).

### F3 — `/compactar`
- Builtin que dispara compactación bajo demanda (`context.rs` evaluar+compactar)
  y respeta el gancho de 109A-1. Si no hay material que resumir, aviso explícito
  (no-op visible, nunca silencio). Evento en historial.

### F4 — `/meta <texto>` y retiro del modo
- El comando ejecuta **un turno** con política meta (solo lectura) como override
  de turno, no como modo global. `permiso_por_modo("meta")` se conserva como
  política interna de turno.
- Retirar `meta` del segmentado de `OPCIONES_EJECUCION`; migración de config:
  `modo: meta` → `predeterminado` + aviso visible una vez.
- Tests de política (deny con efecto, allow lectura) + E2E en ventana real.

## 3. No alcance

- Nuevos comandos más allá del catálogo v1 (van a tareas hijas). Skills intactas.

## 4. Definition of Done

`tsc` EXIT 0, `vite build` OK, clippy 0, tests verdes, gate PASS, E2E
(`/compactar` resume, `/meta` no muta, menú navegable por teclado), roadmap y
completada con evidencia.
