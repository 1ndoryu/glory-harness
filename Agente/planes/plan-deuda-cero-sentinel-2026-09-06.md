# Plan 069A-5 — Deuda cero Sentinel (06-09-2026)

Objetivo: `sentinel check 069A-5 --stages scripts/quality/stages.json --full` con
**0 errores, 0 warnings, 0 info**. Estado de partida (full 069A-4): PASS con
0 errores, 18 warnings, 4 info. Sin `sentinel-disable-file` ni excepciones en
`settings.json`: solo fixes reales (la regla `limite-lineas-nivel-2` lo prohíbe
explícitamente).

## Inventario (full 069A-4, `.quality-reports/check/069A-4/latest.json`)

| # | Regla | Dónde | Tamaño |
|---|-------|-------|--------|
| 1 | directorio-abarrotado | `core/src/herramientas/` 11 ficheros (máx 10) | 1 |
| 2 | funcion-larga-rs | `cli/src/ui/chat.rs` `bucle_chat` 112 (máx 100) | 1 |
| 3 | funcion-larga-rs | `cli/src/ui/tui/bucle.rs` `spawn_worker` 107 | 1 |
| 4–8 | funcion-larga-rs ×5 | `core/src/contrato/ports.rs` defaults `tarea_*` 103 c/u | 5 |
| 9 | funcion-larga-rs | `core/src/politica/bash_clasificar.rs` `franja_baja` 198 | 1 |
| 10–11 | funcion-larga-rs ×2 | `desktop/.../main.rs` `abrir_sesion_interna` 118, `enviar_turno` 174 | 2 |
| 12 | limite-lineas | `cli/src/main.rs` 532 (máx 500) | 1 |
| 13 | limite-lineas + nivel-2 | `cli/src/persistencia_sqlite.rs` 1006 | 1 |
| 14 | limite-lineas | `cli/src/ui/chat.rs` 547 | 1 |
| 15 | limite-lineas | `core/src/nucleo/memoria.rs` 555 | 1 |
| 16 | limite-lineas + nivel-2 | `desktop/.../main.rs` 1121 | 1 |
| 17–20 | todo-pendiente ×4 info | `cli/src/comandos/notificar.rs:96`, `desktop/.../main.rs:54,1278,1280` | 4 |

## Fases (cada una cierra con tests + clippy + gate; un commit por fase)

- **F0 Preflight.** Verificar ID `069A-5` sin colisión; `git status`; estado del
  build de desktop: `desktop/src-tauri/tauri.conf.json` trae un diff ajeno de
  3 líneas que hoy rompe `cargo check --workspace` (`unknown field devtools`).
  Sin desktop compilable no se puede verificar F6: localizar al autor del diff
  ajeno o revertirlo si es deriva; hasta entonces F6 queda bloqueada y el resto
  avanza. Añadir `069A-5` al roadmap como activa.
- **F1 Quick wins: `todo-pendiente` ×4.** Leer los 4 marcadores; cada uno se
  resuelve o se migra al roadmap con ID propio y se borra el marcador.
  Riesgo nulo. Verificación: el gate full ya no lista `todo-pendiente`.
- **F2 `funcion-larga` core+CLI (nº 4–9, 2).** `ports.rs`: inspeccionar los 5
  defaults (103 líneas efectivas c/u); extracción esperada: SQL/strings a
  `const` + helper de error compartido. `franja_baja` (198): pasar a tabla de
  patrones o partir por familias. `bucle_chat` (112) y `spawn_worker` (107):
  extraer un auxiliar cada una (p. ej. carga de historial / relevo de evento).
  No desktop.
- **F3 Split `core/src/nucleo/memoria.rs` (nº 15, 555, código propio).**
  `core/src/nucleo/memoria/` con `sanitize.rs`, `proveedor.rs`, `curador.rs`,
  `tools.rs` (+ tests junto a cada pieza); `mod.rs` re-exporta para no romper
  `crate::memoria::*`. Riesgo bajo (código 069A-4 con 18 tests).
- **F4 `directorio-abarrotado` (nº 1).** Mover `scheduler.rs` (dominio tareas)
  a `core/src/herramientas/tareas_programadas/scheduler.rs` y re-exportar el
  módulo desde `herramientas/mod.rs` (`pub use tareas_programadas::scheduler;`)
  para que `crate::scheduler::*` siga resolviendo sin tocar consumidores.
  Quedan 10 ficheros + 1 subdirectorio. Verificación: build + tests + grep de
  paths.
- **F5 `limite-lineas` CLI (nº 12–14).** `cli/src/main.rs` 532 → mover
  `cmd_schedule_impl`/`cmd_session`/`cmd_memoria` a sus módulos de comandos.
  `chat.rs` 547 → extraer tabla de comandos `/` a `ui/chat_comandos.rs`
  (F2 ya recorta). `persistencia_sqlite.rs` 1006 (nivel-2, la pieza grande):
  `persistencia_sqlite/` por dominio (conversaciones / turnos-mensajes /
  tareas-scheduler / memoria-skills / config), `impl PersistenciaSqlite` en
  cada submódulo + re-export; los tests sqlite existentes son la red de
  seguridad + ciclo E2E `session`/`memoria`/`schedule` real.
- **F6 Desktop (nº 10–11, 16 + `todo-pendiente` ×3).** Prerrequisito F0
  (tauri.conf). Partir `abrir_sesion_interna` (118) y `enviar_turno` (174) en
  auxiliares; dividir `main.rs` 1121 por dominio (conversaciones, workspace,
  config) en módulos; resolver los 3 `todo-pendiente`. Verificación: `cargo
  check/test -p glory-harness-desktop` + E2E ventana real (según Bloque A).
- **F7 Cierre.** Gate full 0/0/0, commit por fase ya hechos, push, roadmap:
  retirar 069A-5 y registrar en `Agente/completados/`.

## Orden y dependencias

F0 → F1 → F2 → F3 → F4 → F5 → F6 → F7. F1–F4 independientes entre sí tras F0
(pueden reordenarse); F5 después de F2 (comparte `chat.rs`); F6 solo tras
desbloquear tauri.conf; F7 exige todo verde.

## Definition of Done

1. `sentinel check 069A-5 --stages scripts/quality/stages.json --full` → PASS,
   0 errores, 0 warnings, 0 info.
2. `cargo test -p glory-harness-core --lib` + `-p glory-harness --lib` +
   `-p glory-harness-desktop` (cuando compile) verdes, sin bajar conteo.
3. `cargo clippy -p glory-harness-core -p glory-harness --all-targets --
   -D warnings` limpio; ciclo E2E real `session`/`memoria`/`schedule` verde.
4. Sin `sentinel-disable-file`, sin excepciones nuevas, sin cambios de
   comportamiento (solo reestructuración + los 4 todos resueltos/migrados).
