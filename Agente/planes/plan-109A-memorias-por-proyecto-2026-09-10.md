# Plan 109A — Memorias por proyecto + hook pre-compact (gaps vs VS Code)

> IDs roadmap: **109A-1/2/3** · Fecha: 2026-09-10 · Estado: F1 completada; F2/F3 pendientes
> Origen: comparativa VS Code (§ memorias/compactación, 09-09): VS Code aporta
> hook `PreCompact` y memoria versionable por ámbitos; GH compacta y cura mejor
> pero su memoria es global por usuario y no tiene hook previo.

## 1. Estado actual (verificado)

- Memoria solo por `user_id` → **global entre proyectos**: `memoria_listar/upsert/borrar`
  (`cli/src/persistencia_sqlite/puerto.rs:145-223`), tabla `memoria`
  (`cli/src/persistencia_sqlite.rs:93` + `ALTER` origen/usos/ultimo_uso `:147-153`).
- Skills (`con_skills_base`) globales por usuario — **se quedan globales** (no alcance).
- Compactación automática en `core/src/nucleo/context.rs`: umbral 0.5, piso 0.75
  (<512K), degenerado 0.85, `pct_compactar` por consumidor (F6), anti-thrash,
  conserva cola, no interrumpe tool (`:198-299`). Sin punto de extensión previo.
- Curador nativo (`nucleo/memoria/curador.rs`, `es_peticion_curador` en `cron.rs:65`);
  sanitize anti-secretos (`nucleo/memoria/sanitize.rs`); CLI
  `memoria <listar|recordar|guardar|borrar|curar>` (`cli/src/main.rs:210,258`).
- Identidad de proyecto: tabla `workspaces` + `comun.workspace` (`elegir_workspace`).
- Modal de configuración declarativo (`desktop/ui/src/dominio/opciones.ts`: controles
  texto/seleccion/segmentado/lectura/valor/booleano; montar en
  `componentes/modal.ts` + `orquestador/vistaModal.ts`).

## 2. Objetivo

Memorias **estrictamente por proyecto** (nunca mezcladas), hook ejecutable antes de
compactar, export/import por ámbito estilo VS Code (`project` versionable / `local`),
y sección "Memorias" en Configuración para gestionarlas.

## 3. Fases

### F1 — 109A-1 Hook pre-compactación (HECHA 2026-09-10)
- `ContextoConfig.gancho_pre_compact: Option<ComandoGancho>` se ejecuta antes
  de resumir y recibe JSON por stdin (ocupación, mensajes y resumen candidato).
- Runner de comando: timeout cubre spawn/escritura/espera/lectura, stdout limitado
  a 64 KiB, `exit 2` veta, salida JSON permite únicamente `resumen` o
  `resumen_llm`; fallo/timeout/JSON inválido registra warning y continúa.
- Runner HTTP: errores y timeout no exponen URL, credenciales, query, fragmento
  ni ruta; solo se registra origen `esquema://host:puerto`.
- Configuración durable: resolver compartido con warning para JSON inválido;
  aplicado a sesión, `run`, `chat`, `tui` y `schedule run`; `null` elimina el
  hook y la UI devuelve `null` al leerlo.
- Evidencia: `cargo test -p glory-harness-core --lib --quiet` (273/273),
  `cargo test -p glory-harness --lib --quiet` (102/102), Clippy de core+CLI
  con `-D warnings` limpio, build UI `tsc --noEmit && vite build` correcto,
  análisis reducido Sentinel 0 errores/0 warnings en 18 archivos.
- Gate canónico ejecutado dos veces: FAIL por `sccache-no-configurado` y
  317 errores/206 warnings heredados de `data/referencias-cli/**`; no se
  declara PASS del gate global.

### F2 — 109A-2 Memoria por proyecto + export/import (base de 109A-3)
- Migración: `ALTER TABLE memoria ADD COLUMN workspace_id TEXT`; recuerdos viejos
  (NULL) quedan en ámbito `global` legacy: **legibles con flag explícito, nunca
  listados en un proyecto** (aislamiento por `WHERE workspace_id = ?`).
- Puerto `AgentPersistence::memoria_*` con scope de proyecto (workspace activo;
  flags CLI `--proyecto/--global`); curador y `ProveedorMemoria` filtran por proyecto.
- Export/import markdown por recuerdo (frontmatter clave/origen/usos/ultimo_uso +
  cuerpo): ámbito `project` (para versionar en el repo) / `local` (no versionar);
  `sanitize` obligatorio en import.
- Tests de aislamiento: dos workspaces no se ven entre sí; global no fuga a proyecto.
- Gate: clippy 0 + tests + `memoria` CLI verificado a mano.

### F3 — 109A-3 Sección "Memorias" en Configuración (depende F2)
- El esquema declarativo de `opciones.ts` no admite listas gestoras → panel custom
  `componentes/memorias.ts` (≤300 líneas; partir en lista/detalle si crece),
  montado como sección del modal (`modal.ts`/`vistaModal.ts`), tokens en
  `variables.css`, sin literales.
- Funciones: listar **solo proyecto activo** + buscar, ver detalle, borrar,
  curar, exportar/importar; distintivo de ámbito (proyecto/global-legacy);
  confirmación en borrado. Nunca muestra recuerdos de otros proyectos.
- IPC Tauri nuevos: `memoria_listar_proyecto`, `memoria_borrar`, `memoria_curar`,
  `memoria_exportar`, `memoria_importar` (todos con workspace activo del backend).
- Validación: `tsc` + `vite build` + E2E manual en ventana real con 2 proyectos
  (no mezcla) + gate.

## 4. No alcance

- Skills siguen globales por usuario. La migración no reasigna recuerdos viejos a
  proyectos. Sin memoria semántica/vectorial (KV + curador basta por ahora).

## 5. Definition of Done

`cargo clippy --workspace --all-targets` 0 warnings, `cargo test --workspace` verde,
`tsc` EXIT 0, gate full PASS 0/0/0, E2E 2-proyectos sin mezcla, roadmap actualizado y
completada con evidencia por fase.
