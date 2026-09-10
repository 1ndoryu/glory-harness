# Plan 109A-4 — Comandos `/` estilo VS Code (`/compactar`, `/meta`)

> ID roadmap: **109A-4** · Fecha: 2026-09-10 · Estado: activo
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

### F1 — Relevamiento en referencias (primero, cierra el catálogo v1)
- VS Code: `/compact` + `PreCompact` (ya relevado 09-09); opencode
  `config/command.ts`; claurst/grok-cli slash (ver `data/referencias-cli/`).
- Entrega: tabla comparativa + lista cerrada de comandos v1
  (`/compactar`, `/meta`, + los que salgan del relevo) en este plan.

### F2 — Menú `/` en la entrada
- Detectar `/` al inicio del textarea (`componentes/entrada*`); menú flotante
  con filtrado por prefijo, navegación teclado (↑↓/Tab/Esc), inserción al elegir.
- Nuevo `componentes/menuComandos.ts` (≤300 líneas); iconos de `iconos.ts`,
  tokens en `variables.css`, sin literales. Datos: builtins + descubiertos del
  backend (nuevo IPC `comandos_listar` o vía `enviar_turno` con expansión previa).

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
