# Plan 149A-1 — 15 reglas del gate en repos dueño + subir pin (2026-09-14)

Origen: auditoría glory-harness §4.4 (10 sentinel + 5 varsense) + cierre F7n
139A-8 (`tareas-2026-09-14.md`: 0/15 existen upstream → tarea externa).
Cierra el loop F7n: cuando las reglas existan, este repo sube el pin.

## Fases

### F1 — sentinel seguridad/red (5 reglas) → v0.7.11
Repo `area-trabajo/glory-sentinel` (main @9f475d2; sin roadmap; IDs `[149A-1]`
en comentarios/tests/CHANGELOG; CI = lint+compile+check:core+smoke:lsp+mocha+check:zed).
1. `rusqlite-bloqueante-en-async` (error): `async fn` con `Mutex<Connection>.lock()`
   sin `spawn_blocking`. ➕ `persistencia_sqlite/puerto.rs:26`.
2. `shell-modelo-sin-allowlist` (error): `Command::new("cmd"|"sh")` con arg que
   fluye de parámetro `comando:&str`. ➕ `infra/ejecutor.rs:69-78`; ➖ `git.rs:385`.
3. `secreto-en-log` (error): `(e)println!/tracing/log` × `token|secret|Bearer|api_key`.
   ➕ `daemon.rs:285`.
4. `ruta-post-sin-rate-limit` (error): `post(_)` sin `RateLimit|governor`.
   ➕ `web/mod.rs:457-499`.
5. `path-join-sin-canonicalize` (error): `join` sin `canonicalize()+starts_with`.
   ➕ `comandos/memoria.rs:417`.
- Mecánica (arquitectura verificada): entrada en `REGISTRO`
  (`src/config/ruleRegistry.ts`: `id/nombre/severidadDefault/categoria`) +
  `export function verificar*` en fichero NUEVO (`src/analyzers/rustAuditRules.ts`,
  salvo que exista punto mejor) + cableado en `analizarEstatico()` tras
  `reglaHabilitada()` + tests en fichero NUEVO `src/test/suite/batch149A1.test.ts`
  (patrón `createCoreDocument` + `assert` sobre `reglaId`, con casos ➕ y ➖ de
  la auditoría) + entrada CHANGELOG.
- DoD: `npm run compile` + mocha del suite nuevo + CI local
  (lint, check:core, smoke:lsp, mocha completo, check:zed) verde; commit
  `[149A-1]`; tag `v0.7.11`; push `main` + tag.

### F2 — sentinel async/estructural (5 reglas) → v0.7.12
6. `sqlite-carga-N-consultas` (warning): ≥3 awaits secuenciales a persistencia
   sin `join!`. ➕ `web_datos/conversaciones.rs:125-137`.
7. `clone-bajo-lock-async` (warning; `Arc::clone` barato exento).
   ➕ `scheduler.rs:66`, `web_datos/mod.rs:98`.
8. `html-modelo-sin-origen-auditado` (error): productor `html` fuera de los 3
   auditados. ➕ `mensajesUtil.ts:102-104` (productor), no el consumidor con allowlist.
9. `god-object-rs` (warning >500 lín, error >800; `mod.rs` re-export exento).
10. `port-filesystem-duplicado` (warning: similitud >0.8 en ≥2 crates; waiver
    `// diverge de X porque …`). Riesgo arquitectónico: sentinel analiza por
    fichero; implementar como pasada a nivel workspace (solo si hay roots,
    fail-closed sin roots, patrón `proyectoTieneModalCanonico`). Si la pasada
    workspace no encaja, degradar a detección mismo-workspace documentada.
- DoD igual que F1; tag `v0.7.12`; push.

### F3 — varsense (5 reglas) → v2.2.2
Repo `area-trabajo/varsense` (clon fresco @544e3c8, limpio; Keep-a-Changelog;
artefacto = bundle esbuild + manifest; contrato `docs/artifact-contract.md`).
Mecánica: `enum DiagnosticType` (`src/types/index.ts`) + emisión en
`analyzeDocument.ts`/`tokenRules.ts`/`classIndexBuilder.ts` + severidad/default
en `src/core/config.ts` (+ `contributes.configuration` si aplica) + tests en
`coreContracts.test.ts`/`varsenseCli.test.ts` (workspaces temporales).
11. `orphan-plantilla-resuelta`: resolver `` `pref-${var}` `` + ternarios ANTES
    de marcar (extiende `claseHuerfana`; zona caliente 318A-7V14/V17/V18).
12. `css-var-sin-token`: opción para subir `hardcodedDetection` a error en UI
    (default conserva warning; no romper 089A-3).
13. `todo-prosa-vs-marcador`: exigir `TODO:|TODO(|FIXME|XXX`; no marcar
    `todo el…` (no existe infra TODO: regla nueva, ver S1 `defaultRules.ts`
    de sentinel como referencia de cuerdas).
14. `ui-fanout-directorio`: excepción solo con tarea+fecha en roadmap, nunca muda.
15. `duplicado-cross-crate`: REGLA SEPARADA (info→warning con rutas+similitud);
    NO tocar `token-duplicate` (decisión 318A-7V8: same-file a propósito).
- DoD: `compile` + `compile:tests` + `lint` + targeted mocha verdes (+ `pretest`
  completo si hay xvfb); commit; tag `v2.2.2`; push + tag.

### F4 — subir pin en glory-harness (cierra F7n)
Transacción AGENTS.md §6: commit fuente publicado por fase → `quality:bump`
(o equivalente declarado) → setup oficial → regenerar lock (sin editar a mano)
→ `quality:doctor` → tests adapter → `sentinel check` PASS → sync guard
(`sync:quality` + `verificar-alineacion.mjs` si existe) → commit + push.
DoD: pin ≥ reglas nuevas, gate PASS, roadmap limpio.

## Mitigaciones
- **M1 colisión 119A-4** (WIP ajeno sin commit en sentinel: 7 ficheros + test
  untracked): ficheros de regla y test NUEVOS; único fichero ajeno tocado =
  `staticAnalyzer.ts` (cableado) en hunk mínimo y región distinta a la modal;
  `git status` antes/después de cada fase; jamás tocar sus 7 ficheros fuera
  del hunk de cableado.
- **M2 FPs** (cultura de ambos repos: 039A-1/119A-4/318A-7 son batches FP):
  cada regla trae tests ➕ (ejemplos reales auditoría) y ➖ (contraejemplos
  `git.rs:385`, allowlist `:65-99`, `Arc::clone`, `mod.rs`); patrones
  conservadores; waivers donde la spec los exige (F2-10, F3-14).
- **M3 decisiones previas**: token-duplicate same-file (318A-7V8) intacto;
  `hardcodedDetection` default warning intacto (089A-3).
- **M4 empuje**: verificar `git push` con 1ª fase; si falla auth, STOP y pedir
  credencial (sin push no hay pin → no continuar a F4).
- **M5 disco**: `C:\tmp` techo 7 GB; `CARGO_TARGET_DIR=C:\tmp\glory-target/*`
  si `check:zed` compila; nada de `target/` en árbol.
- **M6 alcance**: sin medidor ni reglas en glory-harness (restricción F7n);
  roadmap/planes de cada repo dueño sin inventar estructura (sentinel no tiene
  roadmap: solo comentarios+tests+CHANGELOG con `[149A-1]`).
- **M7 varsense tests**: suite completo requiere xvfb (`npm test`); mínimo por
  fase = compile+lint+targeted mocha; documentar si el full no corre.

## Estado
- Pendiente de ejecución (plan aprobado por usuario 14-09).
- IDs: `[149A-1]` (sin colisión en sentinel; verificar en varsense al empezar F3).
- Versiones propuestas: sentinel `0.7.11` (F1) + `0.7.12` (F2); varsense `2.2.2` (F3).
