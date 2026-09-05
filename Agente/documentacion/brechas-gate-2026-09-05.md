# Brechas del gate — reglas Sentinel nuevas (S7, plan 059A-1)

- **Fecha:** 05-09-2026 · **Área:** glory-harness + glory-sentinel
- **Base:** plan `Agente/planes/plan-saneamiento-calidad-2026-09-05.md` (S7), defectos
  reales vividos en este repo y evidencia del `quality:analyze` del 05-09.
- **Estado del bump:** glory-sentinel `902c45e` (v0.7.8), repineado en glory-harness
  (`quality-tools.json` + `sentinel.lock.json`), doctor verde, suite 583 passing.

## Implementadas en glory-sentinel `902c45e` (decisión D4: entran ya)

| Regla | Severidad | Detección | Hits actuales tras el bump |
|---|---|---|---|
| `expect-produccion-rs` | error | `.expect(...)` fuera de archivos `#![cfg(test)]` (ampliación de `unwrap-produccion-rs`) | 3 reales corregidos en core+cli (fetch.rs, tareas.rs, llm.rs); 2 restantes = `desktop/src-tauri/src/vault.rs` (ajeno 039A-3, sin tocar) |
| `block-en-async-rs` | error | `block_on`/`Handle::block_on` en contexto async. Evidencia real: panic "Cannot block the current thread from within a runtime" en `tui.rs:281` (Bloque 3 F1 del chat) | 0 (block_on legítimo solo en `main()` síncrono y tests) |
| `lock-a-traves-await-rs` | warning | `MutexGuard` (std/tokio) viva a través de `.await` sin drop previo (heurística por rango con purga por bloque) | 0 en core+cli |

Tests: `src/test/suite/rustReglasNuevas.test.ts` (7 tests: caso mínimo + no-disparo).
Módulo: `src/analyzers/rustReglasNuevas.ts`; registro en `src/config/ruleRegistry.ts`;
integración en `src/analyzers/rustAnalyzer.ts` (severidad configurable vía
`sentinel.config.json`).

## Corregido en el mismo bump (S1, falso positivo)

- `directorio-abarrotado`: el conteo de archivos por directorio ignora `*.lock`,
  manifests y config de raíz (no-código). Tests: `directorioAbarrotado.test.ts`.

## Propuestas para el siguiente bloque de gate (decisión D4 restante)

1. **`tool-sin-schema-rs`** (o `registro-sin-nombre`, warning): tool registrada sin
   descripción/schema — el modelo la ignora. Estructural barato sobre el registro del
   núcleo (declaración vs uso).
2. **SQL no preparado en Rust** (error cuando aplique): patrón `format!`/`concat!` en
   query SQL. Solo para crates con SQL (PT; GH no tiene SQL por contrato — inactiva por
   defecto aquí).
3. **Cobertura de lenguajes (S1.4):** habilitar perfiles TS/React/SQL existentes
   (`desktop/ui`, scripts). Requiere decisión: toca `desktop/ui` (ajeno) y PT.
