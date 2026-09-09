# Plan 089A-15 — Gate con cobertura por defecto (nada fuente queda fuera en silencio)

> ID roadmap: **089A-15** · Fecha: 2026-09-09 · Estado: activo (plan, pendiente de aprobación para ejecutar)
> Origen: 089A-13 demostró que `desktop/ui/src/**/*.ts` (35 ficheros) y
> `desktop/src-tauri` en VarSense eran invisibles para el gate desde fase 0.
> Defecto general: Sentinel confía ciegamente en `includePatterns` y `doctor`
> reporta `readyForGate: true` con `issues: []` aunque quede código productivo
> sin cubrir (fail-open en cobertura).

## 1. Objetivo

Que en glory-harness (piloto) y en próximos proyectos (por defecto) **todo
fichero fuente trackeado esté cubierto por el gate salvo exclusión explícita
y justificada**, y que cualquier hueco futuro grite en vez de callar.

## 2. Alcance / no alcance

- SÍ: (A) patrones amplios por extensiones fuente en glory-harness + etapa
  `coverage` que falla si hay fuente sin cubrir; (B) set estándar de
  extensiones + propuesta upstream a Sentinel + propuesta de actualización a
  la skill `quality-gate-setup`.
- NO: sanear los hallazgos TS revelados (eso es 089A-14, otro agente/turno);
  reescribir reglas de Sentinel; editar la skill global sin aprobación;
  cambiar severidades existentes; tocar `tools/sentinel` o checkouts externos.

## 3. Fase A — Piloto en glory-harness (089A-15)

1. **Patrones amplios.** `sentinel.config.json`:
   `**/*.rs`, `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.mjs`, `Cargo.toml`,
   `Cargo.lock` (ya hecho para `rs`+`ts` en 089A-13; completar `tsx/js/mjs`
   tras verificar que el motor los acepta sin ruido sobre `scripts/quality/`).
   `varsense.config.json`: añadir `desktop/ui/src/**/*.ts` (hecho en 089A-13;
   verificar) + mismos `js/mjs` si VarSense los soporta.
2. **Etapa `coverage` (adapter project-owned).** Nuevo
   `scripts/quality/sentinel-coverage.mjs`, primera etapa del gate en
   `scripts/quality/stages.json` (antes de `sentinel`):
   - Entrada: `git ls-files` (solo trackeados; lo ignorado no existe para el gate).
   - Filtro a "fuente obvia": allowlist de extensiones
     `rs, ts, tsx, js, mjs, css, html, json, toml, yaml, ps1, bat, py`.
   - Cruce: cada fuente debe matchear ≥1 `includePatterns` de
     `sentinel.config.json`/`varsense.config.json` y ningún `excludePatterns`.
   - Salida: reporte `coverage.json` (schema v1 del stage) + lista en terminal.
     **Fail-closed**: exit ≠ 0 si hay ≥1 fuente sin cubrir, con el remedio
     impreso (añadir a includes o justificar en excludes). Sin flag laxo: la
     laxitud es lo que nos trajo aquí; la válvula de escape es editar los
     patrones a propósito (queda en el diff, auditable).
   - Sin dependencias nuevas: matching con `node:path` + mini-glob propio para
     los patrones que usamos (`**`, `*`, `*.ext`); si un patrón no se puede
     evaluar, se reporta como "no evaluable" (warning, no error) para no
     bloquear por un falso positivo del propio vigilante.
3. **Verificación.** `sentinel check 089A-15` en árbol limpio de esta tarea:
   PASS/FAIL da igual (089A-14 sigue rojo), lo que debe cumplirse es que la
   etapa `coverage` corre primera y su reporte lista 0 fuentes sin cubrir.
   `cargo test`/`clippy` no afectados (la etapa es Node puro, <5 s).
4. **Docs.** Completada en `Agente/completados/tareas-2026-09-09.md`; este plan
   a `Agente/planes/completados/`; actualizar `Agente/documentacion/` del gate
   si existe (cómo añadir un lenguaje nuevo: 1 línea en includes + gate verde).

## 4. Fase B — Por defecto en próximos proyectos

1. **Set estándar "fuente obvia".** Proponer como estándar del área (para
   pegar en bootstraps): includes
   `**/*.rs`, `**/*.ts`, `**/*.tsx`, `**/*.js`, `**/*.mjs`, `**/*.css`,
   `**/*.html`, `**/*.json`, `**/*.toml`, `**/*.yaml`, `**/*.ps1`,
   `Cargo.toml`, `Cargo.lock`, `package.json`; excludes solo artefactos
   (`**/target/**`, `**/node_modules/**`, `**/dist/**`, `**/build/**`,
   lockfiles de terceros cuando aplique) + infra del gate
   (`.quality-tools`, `.quality-reports`, `.sentinel`). Binarios/imágenes/logs
   no necesitan excluirse: no son fuente y `coverage` no los pide.
2. **Propuesta upstream a Sentinel** (el defecto real está en el producto):
   - `doctor` debe exponer `uncoveredFiles`: fuentes trackeadas (por
     extensiones conocidas) fuera de todos los patrones, como `info` siempre
     y como `warning` si superan un umbral (p. ej. >5 ficheros o >500 líneas).
   - `sentinel check` debe abortar fail-closed (o al menos avisar en el
     reporte) cuando el scope efectivo excluya fuentes trackeadas y no exista
     `coverage: { allowUncovered: true }` explícito en config.
   - Texto del issue preparado en §6; abrirlo donde viva el proyecto
     Sentinel (no editar el checkout fijado por commit).
3. **Propuesta de actualización a la skill `quality-gate-setup`** (NO editar
   la copia global sin aprobación del usuario): añadir al "Bootstrap mínimo"
   el paso de patrones amplios + etapa coverage con el set del punto 1, y la
   regla "ningún bootstrap sin `coverage` verde en el primer gate".
4. Definition of Done de la fase: (A) gate 089A-15 con etapa coverage verde;
   (B) issue/propuesta redactada y skill pendiente de aprobación (no se exige
   merge upstream para cerrar: depende de terceros).

## 5. Riesgos

- El motor Sentinel podría no aceptar `tsx/js/mjs` (ruido o error de etapa):
  mitigación en A.1 — probar patrón a patrón y dejar solo los que analizan
  limpio; documentar la matriz motor×extensión en la completada.
- `coverage` fail-closed rompe el flujo del otro agente si añade un lenguaje
  nuevo sin actualizar patrones: es el comportamiento deseado (grita antes de
  integrar), y el remedio es 1 línea. Avisar en el resumen de sesión.
- `.gitignore` vs `git ls-files`: un fuente ignorado a propósito (generado
  commiteado por error, vendor) no lo ve `coverage`; contrapeso: `excludePatterns`
  sigue siendo explícito y el `doctor` valida hashes.
- Coste: la etapa recorre el árbol git (<5 s); no compila, no toca `C:\tmp`.

## 6. Borrador del issue upstream (para abrir, no enviado)

> **Título:** `doctor`/`check` silencian fuentes fuera de `includePatterns`
> (fail-open en cobertura)
>
> **Caso:** proyecto nacido Rust-only con `includePatterns: ["**/*.rs"]`;
> meses después suma frontend TS (35 ficheros, `main.ts` 1424 líneas) y
> backend Tauri. `doctor` reporta `readyForGate: true, issues: []` y los
> gates salen PASS 0/0/0: nada indica que 1/4 del código no se analiza.
>
> **Propuesta:** (1) `doctor --json` incluye `uncoveredFiles` (trackeados con
> extensión fuente conocida fuera de todos los patrones); (2) `check` falla o
> advierte si hay fuentes sin cubrir sin `coverage.allowUncovered: true`
> explícito; (3) el bootstrap generado incluye patrones amplios por defecto
> en vez de lista mínima.
