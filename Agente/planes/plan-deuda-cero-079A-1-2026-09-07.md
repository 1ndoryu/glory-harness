# Plan 079A-1 — Deuda cero Sentinel, segunda vuelta (07-09-2026)

Objetivo: `sentinel check 079A-1 --stages scripts/quality/stages.json --full` con
**0 errores, 0 warnings, 0 info**. Mismo ciclo que 069A-5 (plan archivado en
`Agente/planes/completados/plan-deuda-cero-sentinel-2026-09-06.md`): planificar,
arreglar por fases con tests+gate cada una, verificar falsos positivos con
evidencia y, si un FP es real de la regla, arreglar Sentinel (commit+push+repin
en glory-sentinel, como `08aaf25`+`6baf87c` con `axum-ruta-sintaxis-rs`).
Novedad de esta vuelta: cierra con **auditoría SOLID** y **auditoría de
rendimiento** nuevas (formato de `Agente/documentacion/auditoria-solid-2026-09-05.md`
y `auditoria-rendimiento-2026-09-05.md`).
Sin `sentinel-disable-file` ni excepciones en `settings.json`: solo fixes reales.

## Inventario (full 079A-1, `.quality-reports/check/069A-5/latest.json`, 07-09)

| # | Regla | Dónde | Origen |
|---|-------|-------|--------|
| 1–2 | broadcast-mutex-riesgo-rs ×2 (error) | `cli/src/comandos/web.rs:94,316` | 069A-2 F2; diseño SSE consciente |
| 3 | directorio-abarrotado 11/10 | `core/src/herramientas/` | Re-regresión: F4 lo dejó en 10, 069A-9 añadió `navegador.rs` |
| 4 | funcion-larga `despachar` 113 | `cli/src/main.rs:39` | Código post-F5 |
| 5 | funcion-larga `ejecutar` 130 | `core/src/herramientas/navegador.rs:111` | 069A-9 |
| 6 | funcion-larga `hojear_stream` 109 | `core/src/nucleo/llm/red.rs:527` | 069A-9/10 |
| 7 | funcion-larga `enviar_turno` 134 | `desktop/src-tauri/src/main.rs:479` | Persistente F6 (igual al bloquear) |
| 8 | limite-lineas 561/500 | `cli/src/comandos/web_datos.rs:996` | 069A-2 F3 |
| 9 | limite-lineas 539/500 | `core/src/nucleo/llm/red.rs:647` | 069A-9/10 |
| 10–11 | limite-lineas 1177/500 + nivel-2 ALTO | `desktop/src-tauri/src/main.rs:1583` | Persistente F6, empeoró (era 963) |
| 12 | limite-lineas 542/500 | `desktop/src-tauri/src/navegador.rs:757` | 069A-1/9 |
| 13 | params-excesivos 9/8 (info) | `abrir_sesion_interna`, `main.rs:385` | Persistente F6 |

`todo-pendiente` sigue a cero (F1 no regresó). Base: `5bb7bf8`.

## Fases (un commit por fase; cada una cierra con tests + gate)

- **F0 Preflight.** ID `079A-1` sin colisión (verificado 07-09); `git status`
  limpio de propios (ajeno en vuelo: `web.rs`, `api.ts`, `main.ts`,
  `tareas-2026-09-07.md` — no tocar, hunk-filter si colisiona);
  `sentinel doctor` verde (si el staging se desincroniza:
  `fetch <glory-sentinel> main:refs/remotes/origin/main` en
  `.quality-tools-harness/sentinel); gate full baseline archivado.
  Añadir `079A-1` al roadmap como activa.
- **F1 `broadcast-mutex` ×2 (nº 1–2).** Refactor real del fan-out SSE en
  `web.rs` a canal lock-free por suscriptor (mpsc por SSE, el broadcast solo
  como bus interno si la medición lo justifica — medir, no suponer).
  Red de seguridad: tests `comandos::web` existentes (TTL+429+paridad socket)
  + E2E curl en vivo (`ready`→`turn.started`→`agent.event`→`turn.finished`)
  + 2 suscriptores SSE simultáneos sin `lagged`. Si la medición demuestra que
  no hay contención posible y el refactor es desproporcionado: NO cerrar en
  falso — traer el dato y pedir decisión antes de seguir.
- **F2 `funcion-larga` CLI+core (nº 4–6).** `despachar`, `ejecutar`,
  `hojear_stream`: un auxiliar por función (misma técnica que F2 de 069A-5).
  No desktop.
- **F3 `limite-lineas` CLI+core (nº 8–9).** `web_datos.rs` 561 → partir por
  dominio (conversaciones / config-providers / workspace) en submódulo;
  `red.rs` 539 → extraer stream/paginación a módulo `llm/` nuevo.
  Tests sqlite + ciclo E2E `session` real como red.
- **F4 `directorio-abarrotado` (nº 3).** `navegador.rs` (dominio navegador) a
  subdirectorio propio con re-export en `herramientas/mod.rs` (misma técnica
  que F4 de 069A-5 con `scheduler.rs`). Quedan 10 + 1 subdirectorio.
- **F5 Desktop (nº 7, 10–12, 13).** Bloqueo ya levantado (`cargo check`
  verde 07-09). Partir `enviar_turno` (134) en auxiliares; dividir
  `main.rs` 1177 por dominio (conversaciones, workspace, config) en módulos;
  partir `navegador.rs` 542; agrupar los 9 params de `abrir_sesion_interna`
  en struct. Verificación: `cargo check/test -p glory-harness-desktop` +
  E2E ventana real (ciclo 039A-3) + `cargo clippy -- -D warnings`.
- **F6 Caza de falsos positivos.** Re-ejecutar gate full tras F1–F5. Cada
  hallazgo que huela a FP se prueba con evidencia (fixture mínimo o E2E en
  vivo, como se hizo con `{param}`→200). Si es FP real de la regla: fix en
  glory-sentinel (test de la regla + fixture equivalencia + `rules.md`),
  commit+push a `origin/main`, staging al nuevo commit, `quality-tools.json`
  + `sentinel.lock.json` repineados, `quality:setup` + `doctor` verdes, gate
  re-ejecutado. Si es defecto real: fix aquí, no en Sentinel.
- **F7 Auditoría SOLID nueva** → `Agente/documentacion/auditoria-solid-2026-09-*.md`
  (mismo método que 05-09: por pieza con `ruta:línea`, clasifica
  `[ya corregido]` / `[corregir aquí]` / `[decisión]`; cero cambios de
  comportamiento en este paso). Cubre el código nuevo (web SSE, `red.rs`,
  `navegador.rs`, panelNavegador, workspaces).
- **F8 Auditoría rendimiento nueva** → `Agente/documentacion/auditoria-rendimiento-2026-09-*.md`
  (mismo método: checklist ítem por ítem, se corrige solo lo demostrado con
  medición; focos: fan-out SSE post-F1, clones de contexto por iteración,
  SQLite WAL bajo 2 procesos, bundle UI). Sin optimización prematura.
- **F9 Cierre.** Gate full 0/0/0 + `cargo test` workspace + clippy + E2E
  navegador y Tauri + push + roadmap/completada + plan a `completados/`.

## Orden y dependencias

F0 → F1 → F2 → F3 → F4 → F5 → F6 → F7/F8 (paralelizables, solo lectura) → F9.
F2–F4 independientes entre sí tras F0; F5 exige desktop compilable (ok);
F6 exige F1–F5 verdes; F9 exige todo verde.

## Definition of Done

1. `sentinel check 079A-1 --stages scripts/quality/stages.json --full` → PASS,
   0 errores, 0 warnings, 0 info.
2. `cargo test` workspace + `cargo clippy --all-targets -- -D warnings` limpios,
   sin bajar conteo; E2E web (curl) y Tauri (ventana) verdes.
3. Auditorías SOLID y rendimiento publicadas en `Agente/documentacion/`.
4. Sin `sentinel-disable-file`, sin excepciones nuevas, sin cambios de
   comportamiento fuera de F1 (y aun ahí, solo el transporte SSE).

## Estado

F0 hecho (doctor verde tras resync staging; F1 diferida por colisión ajena en
`web.rs`, orden F2→F3→F4→F5→F1). **F2 hecho** (`6d73691`: `despachar`→
`opciones_run`/`despachar_run`/`despachar_chat`, `ejecutar`→`arg_str`/`op_script`/
`op_dom`/`op_capturar`, `hojear_stream`→`extraer_evento_sse`/`fusionar_tool_call`;
core `--lib` 257 passed, CLI `--lib` 81 passed). Guardian previo (`4fa4cbd`):
`AgentToolContext` de tests sin campo `navegador` (roto en HEAD desde 069A-1 F5,
15 errores) + `PersistenciaMemoria` inexistente en tests `navegador.rs`.
Próximo: F3 (`web_datos.rs` por dominio, `red.rs` stream a `llm/`).
**F3 hecho** (`041d20a`: `web_datos.rs` 995 → `web_datos/` por dominio;
`addf369`: `hojear_stream`+SSE → `nucleo/llm/stream.rs`; CLI `--lib` 81 passed,
core `--lib` 257 passed; warnings restantes solo en ficheros ajenos
`comandos/web.rs`, `persistencia_sqlite/workspaces.rs`). Lección: partir con
corte exacto por líneas vía script (nunca reescribir a mano) y en pwsh usar
comillas simples (un `` `r `` en doble comilla se vuelve CR y rompe el parse).
Próximo: F4 (`navegador.rs` a subdirectorio).
**F4 hecho** (`16fadef`: `core/src/herramientas/navegador.rs` →
`navegador/` reflejo+operaciones+pruebas con re-export; core `--lib` 257
passed; lección: `//!` tras items → E0753, visibilidad `pub(super)` cruzada).
**F5 hecho** (`922cf9a`: `enviar_turno`→`PaqueteTurno`+7 auxiliares,
`abrir_sesion_interna` 9 params→`OpcionesApertura`,
`desktop navegador.rs` 756→`navegador/` estado+comandos+webview2+puerto;
`cargo check -p glory-harness-desktop` verde; `cargo test` desktop bloqueado
por disco C: lleno, decisión usuario: no borrar; lección: comandos Tauri no se
re-exportan — `pub(crate) mod comandos` + ruta `navegador::comandos::X`).
**F6 hecho** (glory-sentinel `1587c59`: `unwrap/panic-produccion-rs` ignoran
fichero solo-test con `#![cfg(test)]` vía `rustTestScope.ts` compartido con
`rustReglasNuevas.ts`, 3 tests nuevos, `rules.md`; budget ADR 0001: 648/650;
repin harness `065b445`, doctor ready/readyForAnalyze/readyForGate true;
gate full 38E/2W → **2E/2W**: solo `broadcast-mutex` ×2 en `web.rs` ajeno +
`limite-lineas`+nivel-2 en `desktop main.rs` 1232 efectivas).
Próximo: F5b (`main.rs` desktop bajo 500) → F1 (cuando `web.rs` se enfríe) →
F7/F8 → F9.
**F5b hecho**: `desktop main.rs` 1666→519 por dominios (`turno.rs`,
`sesion.rs`, `conversaciones.rs`, `workspaces.rs`, `pruebas.rs` con
`#![cfg(test)]`); `leer_max_ventana` movida a `pruebas.rs` (0 llamadas prod);
`check -p glory-harness-desktop` 0 warnings propios; gate full → **2E/0W**
(solo `broadcast-mutex` ×2 ajeno en `web.rs`). Lecciones: lanzar scripts con
`pwsh` 7 (PS 5.1 lee ANSI y rompe asserts UTF-8); `use super::*` sí trae
privados del padre — los 6 errores eran símbolos movidos (cualificar
`sesion::`/`conversaciones::` + `pub(super)`); structs en firmas de comandos
Tauri-pub(crate) exigen `pub(super)` (E0451).
**F7/F8 hechos** (`ca8ff55`: `auditoria-solid-2026-09-07.md`,
`auditoria-rendimiento-2026-09-07.md`; cero cambios de comportamiento;
1 deuda agendada: `leer_max_ventana` huérfana).
Todo pusheado hasta `ca8ff55`. **Solo queda F1** (bloqueada: `web.rs` con
`M` ajeno) **y F9** (cierre 0/0/0 + tests + clippy + E2E + roadmap/
completada + archivar plan).
