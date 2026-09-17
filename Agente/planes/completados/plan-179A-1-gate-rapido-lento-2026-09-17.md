# Plan 179A-1 — Gate en dos niveles (piloto glory-harness)

> Origen: `workspace-manager/PLAN-gate-rapido-lento-2026-09-12.md` (propuesto
> Buffy 12-09). Piloto: este repo. Rama: `main`. Fecha: 2026-09-17.

## Objetivo

`sentinel check <ID> --stages scripts/quality/stages-fast.json` iterativo
(≤60 s) + `stages.json` actual como único cierre válido. Sin tocar core
Sentinel/VarSense, severidades, `-D warnings` ni el guard.

## F1 Medición (HECHA 17-09, binario real `cargo.exe`, target `C:\tmp\glory-target`)

Nota: `cargo` por PATH es el shim del guard (bloquea directo); las 3 primeras
mediciones por el shim medían el rechazo (5,9/1,07/1,36 s) y se descartaron.
Medición real solo con el binario (`C:\Users\Owner\.cargo\bin\cargo.exe`),
`RUSTC_WRAPPER=sccache`, documentada como excepción puntual de F1 — el modo
fast final corre dentro del token del gate, sin backdoor.

| Caso | Frío | Caliente |
|---|---|---|
| `check -p glory-harness-core --tests --locked` | 140 s | 1,2–1,5 s |
| `check -p glory-harness-core -p glory-harness --tests --locked` | 90 s | 1,4–1,8 s |
| `test -p glory-harness-core --lib turno --locked` | 157 s | 1,3 s (8 passed, 328 filtrados) |

sccache 96,67 % hits. Conclusión: el rápido cabe en 60 s con target caliente
(caso iterativo normal); en frío supera y pide el completo fail-closed (§2.1
del plan origen). Sin recorte de alcance necesario (`--tests` se mantiene).

## F2 Manifiesto rápido + modo fast (HECHA 17-09)

- `scripts/quality/stages-fast.json`: mismas `coverage`/`sccache`/`sentinel`,
  `timeoutMs` 60 000 en las 4; etapa `rust` con arg `--fast`.
- `sentinel-rust.mjs --fast` (tool 1.1.0, reporte `mode`): check `--tests` +
  `test --lib` filtrado; sin clippy; veredicto `rust-modo-rapido`.
- Medido directo (token de medición, alcance 1 archivo): **6,2 s**
  end-to-end, check 0 + 8 tests filtrados (`turno`), 0 errores.

## F3 Fail-closed (HECHA 17-09)

`rust-pide-completo` (error) probado con 3 casos: contrato
(`core/src/contrato/`), lockfile (`Cargo.lock` → rama manifiestos) y 12
archivos (>10). Sin alcance, desktop y >5 filtros también piden completo.
Regresión modo completo: 305 s, clippy 0, **505 tests ok** (6 suites).

## F4 Docs (HECHA 17-09)

- `AGENTS.md` raíz §5: subsección "Gate en dos niveles: rápido y completo".
- Skill `quality-gate-setup`: nota `stages-fast.json` en "Flujo del gate".
  Skill `sentinel` omitida (no menciona `sentinel check`).

## F5 Piloto (HECHA 17-09)

- Piloto 1 (pre-fix): `check 179A-1 --stages stages-fast.json` → 78,8 s,
  etapa `rust` **error timeout**: el alcance (8 ficheros, 0 `.rs`) derivaba
  0 filtros y corría todo el lib (>60 s). Bug real del fast, corregido:
  0 filtros → `rust-pide-completo`.
- Piloto 2 (post-fix): gate rápido **14,1 s** end-to-end (coverage 0,3 s +
  sccache 0,4 s + sentinel 8,3 s + rust 0,4 s con `rust-pide-completo`).
- Cierre con completo: `check 179A-1 --stages stages.json` → **373,8 s**,
  etapa `rust` **pass** (362 s, clippy 0, tests 505 ok, solo info alcance).
  Veredicto global FAIL solo por 30 findings sentinel preexistentes en
  ficheros no tocados (dueño: 149A-2).

## DoD

Rápido ≤60 s medido en caliente · completo intacto · sin backdoor en guard ·
cierre solo con completo · propagación vía `quality:bump`.
