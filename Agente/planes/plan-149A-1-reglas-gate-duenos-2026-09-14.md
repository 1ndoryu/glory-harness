# Plan 149A-1 — 14 reglas del gate en repos dueño + 1 opción de severidad + subir pin (2026-09-14, rev.2)

Origen: auditoría glory-harness §4.4 (10 sentinel + 5 varsense) + cierre F7n
139A-8 (`tareas-2026-09-14.md`: 0/15 existen upstream → tarea externa).
Cierra el loop F7n: cuando las reglas existan, este repo sube el pin.

> **rev.2 (14-09):** reescritura tras reto supervisor-thinking (H1–H11). Cambios:
> base real `2752448` (no `9f475d2`); cableado Rust por `analizarRust()` (no
> `analizarEstatico()`); UNA release sentinel `0.7.11` (no 0.7.11+0.7.12);
> conteo real 14 reglas + 1 opción (F3-12 no es regla); F0 preflight nueva;
> F4 con procedimiento explícito; criterio de precisión H11; F2-8 con
> allowlist configurable; frontera F2-10/F3-15 por extensión de fichero.

## F0 — preflight (bloqueante, ~30 min; nada de F1 sin su DoD)

1. **Upstream sentinel (H3/M4):** `git ls-remote origin` + dar tracking a `main`
   (hoy `main` no tiene upstream y `origin/main` ni existe en el clon; coexiste
   `fix/318A-4-core-budget`: leerla y descartar solape). Si falla auth → STOP
   total: sin push no hay pin y todo lo demás es trabajo perdido.
2. **Base (H2):** la base real es `main @2752448`, que incluye el batch 119A-4
   (react/portable/css, sin release propio). **Decisión tomada:** `v0.7.11`
   incluye 119A-4 con sección propia en el CHANGELOG. Descartado releasear
   119A-4 por separado (costaría un `bump` extra en 11 consumidores sin
   beneficio: ~5 min/proyecto medidos).
3. **Toolchain:** `wasm32-wasip2` instalado (verificado 14-09) para `check:zed`;
   `CARGO_TARGET_DIR=C:\tmp\glory-target/*`; techo `C:\tmp` 7 GB.
4. **Auditoría de solape (H1):** leer `detectarBlockEnAsync`,
   `detectarLockATravesAwait` (`rustReglasNuevas.ts`), `rustAxumStack.ts`,
   `rustTestScope.ts` y mapear qué cubren ya de F1-1/F1-2/F2-6/F2-7.
   Salida: anexo al plan con tabla solape → extender vs crear.

DoD F0: upstream OK + tabla de solape escrita + toolchain OK.

## F1+F2 — sentinel → UNA release `v0.7.11` (10 reglas)

**Por qué una sola (H9):** cada versión cuesta certificar + `bump` en 11
consumidores. Sin razón para partir F1/F2, van juntas.

**Mecánica corregida (H1):** NADA de `rustAuditRules.ts` nuevo: los detectores
van en `rustReglasNuevas.ts` (o módulo específico si F0 lo pide), cableados
como "Paso N" en `analizarRust()` (`rustAnalyzer.ts:39`, invocado desde
`core/analyzeDocument.ts:59`), con guarda `reglaHabilitada()` + entrada en
REGISTRO (`ruleRegistry.ts`) + `sentinel-disable-file <regla-id>` y
`sentinel-disable-next-line` siguiendo el patrón del fichero. Tests en fichero
NUEVO `batch149A1.test.ts` (patrón `createCoreDocument` + `assert` sobre
`reglaId`, casos ➕/➖ de la auditoría) + entrada CHANGELOG.
**Solo F2-8** (TypeScript) va por `analizarEstatico()`; su hunk evita la región
119A-4 (imports ~L8-12 + hunk S4 ~L156+; el diff 119A-4 ahí fue de 6 líneas).

1. `rusqlite-bloqueante-en-async` (error): EXTENDER `detectarBlockEnAsync` si
   ya cubre Mutex genérico; lo nuevo = caso `Mutex<Connection>` rusqlite sin
   `spawn_blocking`. ➕ `persistencia_sqlite/puerto.rs:26`.
2. `shell-modelo-sin-allowlist` (error): heurística conservadora — solo
   `Command::new("cmd"|"sh"|"powershell")` con argumento no literal; el flujo
   desde `comando:&str` se documenta como límite conocido, no como taint real.
   ➖ `git.rs:385` como contraejemplo en test.
3. `secreto-en-log` (error): `(e)println!/tracing/log` ×
   `token|secret|password|Bearer|api_key`. ➕ `daemon.rs:285`.
4. `ruta-post-sin-rate-limit` (error): supuesto documentado en comentario
   (stack axum + `RateLimit|governor`); waiver por disable-file; si el
   self-scan da FP por middleware propio → degradar a warning (salida H11).
   ➕ `web/mod.rs:457-499`.
5. `path-join-sin-canonicalize` (error): `join` sin `canonicalize()` +
   `starts_with()` en el mismo scope. ➕ `comandos/memoria.rs:417`.
6. `sqlite-carga-N-consultas` (warning): ≥3 awaits secuenciales a persistencia
   sin `join!/try_join`, solo dentro de la misma `fn` y mismo receptor.
   ➕ `web_datos/conversaciones.rs:125-137`.
7. `clone-bajo-lock-async` (warning): EXTENDER `detectarLockATravesAwait` o
   regla hermana; `Arc::clone` exento. ➕ `scheduler.rs:66`,
   `web_datos/mod.rs:98`.
8. `html-sin-origen-declarado` (error; renombrada, H4): **allowlist
   configurable por proyecto** (patrón `directoryExceptions`: opciones del
   analyzer con default). Default inicial = los 3 productores auditados,
   documentados en comentario como *semilla*, no como verdad. Flaggea al
   PRODUCTOR (`mensajesUtil.ts:102-104`), no al consumidor con allowlist.
9. `god-object-rs` (H5): implementar reutilizando `contarLineasEfectivas` y la
   escala de `limite-lineas` si encaja; si regla separada, el comentario
   justifica por qué no es parametrización. `mod.rs` solo-reexport exento
   (detectar: solo `pub mod`/`pub use`). Umbrales 500 warn / 800 err.
10. `port-fs-duplicado-rs` (warning; H6: **ámbito solo `.rs`**): similitud >0.8
    en ≥2 crates; pasada workspace con `obtenerWorkspaceRoots`
    (`core/workspaceRoots.ts` existe); fail-closed sin roots (patrón
    `proyectoTieneModalCanonico`); waiver `// diverge de X porque …`. Si la
    pasada no encaja → degradar a detección mismo-workspace documentada.

DoD: `compile` + suite nuevo + CI local (lint, check:core, smoke:lsp, mocha
completo, check:zed) verde + precisión H11 + commit `[149A-1]` + CHANGELOG
(secciones 119A-4 y 149A-1) + tag `v0.7.11` + push `main` + tag.

## F3 — varsense → `v2.2.2` (4 reglas + 1 opción) · paralelizable con F1/F2

Repo `area-trabajo/varsense` (verificado @544e3c8, limpio; Keep-a-Changelog;
artefacto = bundle esbuild + manifest; contrato `docs/artifact-contract.md`).
Mecánica verificada: `enum DiagnosticType` (`src/types/index.ts`) + emisión en
`analyzeDocument.ts`/`tokenRules.ts`/`classIndexBuilder.ts` + severidad/default
en `src/core/config.ts` (+ `contributes.configuration` si aplica) + tests en
`coreContracts.test.ts`/`varsenseCli.test.ts`.

11. `orphan-plantilla-resuelta`: resolver `` `pref-${var}` `` + ternarios ANTES
    de marcar (extiende `claseHuerfana`; zona caliente 318A-7V14/V17/V18).
12. **(opción, NO regla — H8):** `hardcodedDetection.severity` → `'error'` solo
    UI. Default `warning` intacto (089A-3); test de no-regresión del default.
    Sin `DiagnosticType` nuevo.
13. `todo-prosa-sin-marcador`: exigir `TODO:|TODO(|FIXME|XXX`; no marcar
    `todo el…`. `analyzeDocument.ts` parsea comentarios de supresión (enganche
    probable): spike ≤30 min verificando acceso a líneas de comentario desde
    `tokenRules.ts`. **Fallback B:** si no hay enganche, la regla se implementa
    en sentinel `v0.7.11` como regex sobre texto completo y F3 queda en 3+1.
14. `ui-fanout-directorio`: excepción solo con tarea+fecha en roadmap, nunca muda.
15. `duplicado-cross-crate` (H6: **ámbito solo estilos/tokens**): REGLA
    SEPARADA (info→warning con rutas+similitud); NO tocar `token-duplicate`
    (decisión 318A-7V8: same-file a propósito).

DoD: `compile` + `compile:tests` + `lint` + targeted mocha verdes (+ `pretest`
completo si hay xvfb; si no, documentar) + commit + tag `v2.2.2` + push + tag.

## F4 — subir pin en glory-harness (cierra F7n)

Procedimiento explícito (H10; este repo NO tiene `quality:bump`, generador de
lock ni `verificar-alineacion`; scripts disponibles: `quality:setup`,
`quality:doctor`, `quality:analyze`):

1. `quality-tools.json`: sentinel `commit` → sha de `v0.7.11` + `version`
   `0.7.11`; varsense `commit` → sha de `v2.2.2` + `version` `2.2.2`.
   Es manifest, se edita; el lock NO se toca a mano.
2. `npm run quality:setup` (regenera `sentinel.lock.json`) →
   `npm run quality:doctor` (listo) → `npm run quality:analyze` (baseline nuevo).
3. **Triaje del delta 0.7.8→0.7.11:** cada finding nuevo de reglas 0.7.9–0.7.11
   se corrige en código propio o se registra como excepción firmada según
   convención del repo. Prohibido bajar severidades globales para pasar.
4. Roadmap 149A-1 → completados con evidencia; commit + push.

DoD: pins = shas publicados, doctor listo, analyze sin errores no triados,
roadmap limpio.

### Ejecución F4 (14-09, pin real 0.7.12 + 2.2.2)
- Pins: `quality-tools.json` sentinel `0.7.12/66a2113` (split mecánico modal,
  fix-forward tras `check-core` rojo de 0.7.11 por breach heredado in-base),
  varsense `2.2.2/7dac28e`. Staging `.quality-tools-harness/*` con
  `fetch --tags` + `checkout --detach` a ambos shas.
- **D1 (lock):** `quality:setup` provisiona y escribe evidencia pero NO
  regenera `sentinel.lock.json` (ningún script del repo lo escribe; verificado
  por grep). Actualizado `generatedAt` + `version/commit` de ambas herramientas
  siguiendo el precedente `065b445` (`sha256` intactos). No es edición libre:
  es el único procedimiento de repin que el repo practica.
- **D2 (versión):** el paso 1 decía `v0.7.11`; el pin real es `v0.7.12`.
- Doctor: `ready/readyForAnalyze/readyForGate` true, `issues: []`, lock
  `0.7.12/66a2113`, CLIs `0.7.12`/`2.2.2` con checkout==configured.
  Evidencia: `C:\tmp\doctor-0712.json`.
- Analyze 0.7.12: EXIT=1 esperado; 30 findings (10E/15W/5H) en 21/272
  archivos, determinista (2 corridas, mismo hash).
  Evidencia: `C:\tmp\analyze-0712-direct.json` (`.quality-reports/` gitignored).
- Triaje delta (14 hits de reglas nuevas; 16 restantes de reglas
  preexistentes sin cambio de conducta entre 0.7.11→0.7.12):
  `path-join` ×8 = 6 FP (literales/`temp_dir`, UUID/`global` interno en
  `memoria.rs:436`, slug saneado sin `/` ni `..` en `memoria_io.rs:153,159,164`,
  hash hex en `content_search.rs:244`, segmentos fijos `.git/HEAD` en
  `prompt.rs:199`) + 2 TP-bajo solo-lectura con `is_file` previo
  (`content_search.rs:399` rel del índice, `skill.rs:183` referencia `@` del
  prompt); `html` ×1 = TP-identificación, productor legítimo y saneado
  (`mensajesUtil.ts:16`, `escaparHtml`) → candidato a `htmlProductoresPermitidos`,
  no vulnerabilidad; `sqlite` ×5 = FP (awaits dependientes, `join!` imposible:
  loop en `scheduler.rs:61`, `&mut` compartido en `curador.rs:155`, sync
  post-turno en `chat.rs:404`/`bucle.rs:366`, finalizar→log en `cron.rs:304`).
- Sin cambios de código en F4 (fuera de alcance; HEAD es WIP ajeno `f1dde81`):
  los 2 TP-bajo y el allowlist quedan para las tareas dueñas.
- **H11-(b) no literal:** 11 FP heurísticos en workspace real (sin proveniencia
  en path-join, sin análisis de dependencias en sqlite). Las releases ya están
  publicadas; el gap se deriva a `149A-2` (precisión + cablear allowlist),
  no bloquea este cierre.

## Criterio de precisión H11 (todas las fases, sin excepción)

Toda regla nueva: **0 FP** sobre (a) self-scan del repo dueño y (b) workspace
glory-harness, + casos ➕/➖ de la auditoría en tests. Escalera de degradación:
`error → warning → aparcar` con nota en CHANGELOG. Ninguna regla se publica en
`error` sin pasar (a)+(b).

## Mitigaciones

- **M8 oráculo de prueba = glory-harness @ `ddf82e3`** (verificado 14-09):
  139A-8 remedió los ejemplos ➕ a HEAD (`puerto.rs` ya usa `con_conn`,
  `ejecutor.rs` construye sin shell, `daemon.rs`/`scheduler.rs` se partieron
  en F4): probar contra HEAD daría falsos negativos. Baseline con TODOS los
  ➕ vivos = `ddf82e3` (padre de `c986dac`, último pre-139A-8; la auditoría v2
  se verificó contra ese árbol). Oráculo: `git worktree add C:\tmp\gh-oraculo
  ddf82e3` (solo lectura, fuera del árbol) + fixtures sintéticas mínimas por
  regla para los ➖. Los ➖ a HEAD (`git.rs` argv fijo, allowlist
  `mensajesUtil.ts`, `Arc::clone`) valen en cualquier commit. (rev.2)

- **M1 (reescrita):** base `2752448` con 119A-4 incluido y declarado en
  CHANGELOG; `git status` antes/después de cada fase; región 119A-4 de
  `staticAnalyzer.ts` vetada salvo el cableado F2-8 que la evita; jamás tocar
  `reactComponentRules.ts`/`portableRules.ts`/`staticCssRules.ts`/
  `analisisHelpers.ts`/`batch119A4.test.ts` fuera de lo declarado.
- **M2:** H11 + tests ➕ (casos reales auditoría) y ➖ (`git.rs:385`,
  allowlist, `Arc::clone`, `mod.rs` solo-reexport) por regla.
- **M3:** `token-duplicate` same-file intacto (318A-7V8);
  `hardcodedDetection` default `warning` intacto (089A-3).
- **M4:** F0 verifica push primero; sin push no hay pin → STOP, sin continuar
  a F4.
- **M5:** `C:\tmp` techo 7 GB; `CARGO_TARGET_DIR=C:\tmp\glory-target/*` si
  `check:zed` compila; nada de `target/` en árbol.
- **M6:** sin medidor ni reglas en glory-harness (restricción F7n); sentinel
  sin roadmap (verificado: no existe): IDs `[149A-1]` en
  comentarios+tests+CHANGELOG.
- **M7:** suite varsense completo requiere xvfb (`npm test`); mínimo por fase
  = compile+lint+targeted mocha; documentar si el full no corre.
- **M8 (nueva):** el `tools/varsense` de WANDORIUS quedó commiteado en detached
  HEAD (`5f40ccf`, snapshot 14-09): NO usar ese clon como fuente de release;
  releases solo desde `area-trabajo/varsense` limpio.

## Anexo F0 — evidencia (14-09, verificado)

- **Upstream:** `git ls-remote origin main` OK → `9f475d2` (remote = release
  `0.7.10`); local `main @2752448` (1 commit por delante: snapshot WIP 14-09).
  `main` sin tracking. Pendiente (requiere OK):
  `git branch --set-upstream-to=origin/main main`.
- **Rama `fix/318A-4-core-budget @fbb580f`:** base era 0.7.6; el diff
  `main..fix` son demoliciones (sin `rustReglasNuevas.ts`, sin 95 líneas de
  CHANGELOG) → **stale/abandonada, cero solape**. Propuesta: dejar intacta, no
  mergear jamás, no borrar sin autorización aparte.
- **Toolchain:** `wasm32-wasip2` + `x86_64-pc-windows-msvc` instalados
  (`check:zed` viable); `target/` gitignored y sin ficheros commiteados.
- **varsense:** limpio `@544e3c8`, `main` con tracking `origin/main` OK.

### Tabla de solape (auditoría real sobre el código)

| Regla | Estado del solape | Decisión |
|---|---|---|
| F1-1 rusqlite | `detectarBlockEnAsync` (rustReglasNuevas.ts:145) solo matchea `block_on` (L155); nada de Mutex genérico | CREAR detector nuevo; reutiliza `calcularRangosAsync` + `esArchivoSoloTest` + `tieneDisableSiguiente` |
| F2-7 clone-bajo-lock | `detectarLockATravesAwait` (:228) solo dispara con `.await` posterior en la misma fn | Regla hermana; el disparo cae en líneas distintas (clone vs await) → sin doble-marcado; test que lo aserte |
| F1-4 rate-limit | `rustAxumStack.ts` solo resuelve sintaxis matchit/axum; nada de middleware | CREAR; reutilizar patrón lectura Cargo.toml + cache para eximir proyecto con `tower_governor`/`governor` en dependencias |
| F1-2 shell, F1-3 secreto, F1-5 path-join, F2-6 N-consultas | Sin solape | CREAR |
| F2-8 html | TS por `analizarEstatico()`; región 119A-4 vetada (imports ~L8-12 + hunk S4 ~L156+) | CREAR fuera de esa región |
| F2-9 god-object | Sin solape; reutiliza `contarLineasEfectivas` | CREAR (o parametrizar; justificar en comentario) |
| F2-10 port-fs | `core/workspaceRoots.ts:obtenerWorkspaceRoots` existe | CREAR con pasada workspace + fail-closed sin roots |

### Punto de inserción exacto (F1+F2)

- Detectores: `rustReglasNuevas.ts` tras L299 + consts `REGLA_*` junto a L22-24
  + 1 línea por regla en el header L1-16.
- Cableado: `rustAnalyzer.ts` tras L106 (cierre Paso 7) como Pasos 8, 9, … con
  guarda `reglaHabilitada()` + REGISTRO.
- Tests: fichero NUEVO `batch149A1.test.ts` (patrón `createCoreDocument`).

## Estado

- rev.2 (14-09): corregido tras reto (H1–H11). Pendiente de ejecución desde F0.
- IDs: `[149A-1]` (sin colisión en sentinel; verificar en varsense al empezar F3).
- Versiones: sentinel `0.7.11` única (F1+F2, 10 reglas); varsense `2.2.2`
  (F3, 4 reglas + 1 opción). Total: **14 reglas + 1 opción de severidad**.
- Nota: el roadmap de glory-harness dice "15 reglas"; actualizar su texto al
  cerrar F4 (14+1), no antes.
