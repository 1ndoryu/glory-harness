# Plan 089A-6 — Búsqueda indexada (tgrep): spike + tool `content_search`

> ID roadmap: **089A-6** (v1, superada) → **089A-8** (integración) · Fecha: 2026-09-08 · Estado: completado
> Referencia externa: `https://github.com/microsoft/tgrep` (trigram-indexed grep,
> cliente/servidor, Rust;alle Infos en el README del repo).

## 1. Objetivo

Dar al agente búsqueda **por contenido** en el workspace (hoy no existe: `file_search`
solo busca por nombre con un `read_dir` por query, `tools_archivo.rs:274`), usando
tgrep como motor indexado cuando esté disponible y un recorrido acotado como
fallback. Decisión por evidencia: primero spike medido (Fase 0); solo si pasa el
criterio se implementa la tool (Fase 1).

## 2. Alcance / no alcance

- SÍ: Fase 0 (binario en `C:\tmp`, benchmark sobre este repo) + Fase 1 (tool
  `content_search` en `core`, registro, política, tests).
- NO: gestionar `tgrep serve` desde Tauri (Fase 2 futura), UI Files/089A-5,
  tocar ficheros UI con cambios ajenos en curso, compilar dentro del árbol,
  añadir la dependencia `tgrep-core` como lib (se invoca el **binario**).
- Disco: `C:\tmp` está en 5.52/7 GB. El build de tgrep va a
  `C:\tmp\tgrep-target` y **se borra tras extraer el exe** (queda solo
  `C:\tmp\tgrep\tgrep.exe`, unos MB). Si no hay espacio, abortar Fase 0.

## 3. Fase 0 — Spike (sin cambios en el repo salvo este plan)

1. Obtener binario win-x64: buscar asset en releases de tgrep; si no hay,
   `git clone --depth 1` en `C:\tmp\tgrep-src` + `cargo build --release` con
   `CARGO_TARGET_DIR=C:\tmp\tgrep-target`; extraer `tgrep.exe` a `C:\tmp\tgrep\`;
   borrar `tgrep-src` y `tgrep-target`.
2. Medir sobre `glory-harness` (repo real):
   a. `tgrep index` → tiempo de construcción + nº de ficheros.
   b. `tgrep serve` + 5 queries representativas (`--stats`) → latencia p50.
   c. Referencia O(bytes): walk completo leyendo todos los `.rs/.ts` (proxy del
      coste que pagaría un grep sin índice por query).
3. **Criterio de éxito**: query indexada p50 < 500 ms y build < 5 min.
   Si NO pasa → se cierra 089A-6 como "no compensa" con la evidencia y NO hay Fase 1.

## 4. Fase 1 — Tool `content_search` (solo si Fase 0 pasa)

- Nueva `ToolContentSearch` en `core/src/herramientas/tools_archivo.rs`.
- Args: `patron` (requerido), `glob` (opcional, p. ej. `*.rs`), `limite` (1–50,
  defecto 20), `solo_archivos` (bool, defecto false).
- Motor tgrep (si `GLORY_TGREP_BIN` apunta a un ejecutable): spawn
  `tgrep --json [--glob] <patron> .` con `current_dir` = raíz sandbox y timeout
  30 s; parsea el stream JSON (eventos `match`); cada ruta se valida con
  `sandbox.resolver()` (fuera del workspace → se descarta, nunca se lee).
- Fallback (sin binario o error del motor): recorrido acotado — mismos
  directorios excluidos que `file_search` + tope 500 ficheros, 200 KB/fichero,
  detección binaria (NUL en primeros 8 KB), subcadena literal insensible a
  mayúsculas por líneas, máx 50 coincidencias; la salida lleva el aviso
  `[motor: recorrido local; configura GLORY_TGREP_BIN para índice]`.
- Salida: una línea `ruta:linea:contenido` por coincidencia (máx ~12 KB +
  `[truncado]`); con `solo_archivos`, una ruta por línea (máx 50).
- Descripción con FORMATO DE SALIDA + LÍMITES + CUÁNDO USARLA + ERRORES, sin
  backslashes (lo exige el test `descripciones_ricas_sin_escapes_rotos`).
- Registro: `registrar_tools_archivo` (+ test de 7 → 8 descripciones),
  `categorias_core()` en `regla.rs` → `CAT_LECTURA` (lectura, sin clasificador,
  igual que `file_search`), whitelists de perfiles exploradores en
  `subagente.rs` donde ya está `file_search`.
- Tests (módulo de `tools_archivo.rs`): fallback encuentra contenido y respeta
  límites; ruta fuera del workspace → error; sin sandbox → fail-closed
  (no se registra); motor tgrep simulado (binario falso por env en test).
- Definition of Done: `cargo test -p glory-harness-core` verde,
  `cargo clippy` sin warnings nuevos, gate `089A-6` PASS, commit, evidencia en
  `Agente/completados/tareas-2026-09-08.md`, plan archivado.

## 5. Riesgos

- Sin asset win-x64 y sin red/compilador → Fase 0 bloqueada (registrar y cerrar).
- `C:\tmp` lleno → abortar antes de compilar (límite 7 GB).
- Divergencias tgrep (límite 64 MiB, UTF-8 reparado): el fallback y los límites
  propios las absorben; la tool nunca promete exhaustividad total.
- Cambios ajenos en curso (UI sin commitear): no se tocan; Fase 1 es solo `core`.

## 6. Fase 1b — Integración 100% sin dependencia del sistema (089A-8)

> Directiva del usuario tras la v1: "tiene que estar integrado al 100% en el
> proyecto sin depender si está instalado en el sistema o no". La v1 (motor
> por `GLORY_TGREP_BIN` + fallback) queda superada y su código eliminado.

- Dependencia `tgrep-core = { git = ".../microsoft/tgrep.git", tag = "v1.0.5" }`
  en `core/Cargo.toml` (el tag fija la versión legible; `Cargo.lock` pinnea el
  sha `d55b0220...`; `tgrep-core` NO está en crates.io —404 verificado—).
- Módulo propio `core/src/herramientas/content_search.rs` (esto cierra también
  la deuda 089A-7: `tools_archivo.rs` pierde ~500 líneas y `ToolContentSearch`
  sale de él; `glob_simple`/`obtener_sandbox` pasan a `pub(crate)`).
- Índice persistente por workspace en `<temp>/glory-harness/tgrep-idx/<hash16>/`
  + `glory-manifest.json` (foto ruta/mtime/tamaño del mismo paseo que el
  builder, `walk_file_metadata` con opciones por defecto). Si la foto difiere
  → rebuild antes de responder (el agente ve escrituras de su sesión).
- Consulta: `build_query_plan` (error de sintaxis = `Argumentos` antes de
  tocar disco) → `execute_plan_with_masks` + `lookup_trigram_with_masks`
  (o `all_file_ids` si `MatchAll`) → verificación línea a línea con `regex`
  (misma sintaxis), smart-case. Lectura vía `sandbox.leer`: hereda contención
  y lista negra de secretos (un secreto indexado no aflora).
- Concurrencia: `Mutex` por índice; build+consulta en `spawn_blocking` con
  timeout global 180 s. Fallo de índice → recorrido local de emergencia con
  aviso (nunca es un fallo). Sin `unsafe` (`#![forbid(unsafe_code)]` intacto).
- Tests (6, en el módulo): índice integrado, glob+solo_archivos, frescura
  (escritura posterior → rebuild, resumen "índice reconstruido"),
  vacío+inválido fail-closed, binarios+secretos excluidos, `TODO|FIXME`.
- Definition of Done: `cargo test -p glory-harness-core` verde, `clippy` sin
  warnings nuevos, gate 089A-8 PASS, commit (solo `core` + docs; la UI de
  089A-3/4 sigue pendiente de verificación visual), evidencia en
  `Agente/completados/tareas-2026-09-08.md`, plan archivado.
