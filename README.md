# Glory Harness — núcleo de IA agnóstico extraíble (CLI + daemon)

Refactor estructural del plan **318A-13** (`PROYECTO TASKS/Agente/planes/plan-glory-harness-2026-09-01.md`).
Extrae el núcleo de IA del backend de task a una carpeta/repo **agnóstica**: el
runtime no conoce `AppState`, `PgPool` ni las tablas `agente_*`.

## Estructura

```text
glory-harness/
  core/        crate lib `glory-harness-core` — runtime, traits, eventos, tools agnósticas
  cli/         crate bin `glory-harness` — subcomandos run/daemon/tools/doctor (Fase 3)
  scripts/quality/  gate Sentinel/VarSense (mechanism idéntico al del área)
```

## Contrato del núcleo

### Puertos (traits) — sección 6.2 del plan

| Puerto | Responsabilidad | Implementador |
|---|---|---|
| `AgentPersistence` | turnos, mensajes, acciones, memoria, skills, tareas programadas | task (repositorios `agente_*`, SQL) |
| `WebSearchProvider` | búsqueda web acotada | task (`WebSearchService`) |
| `ProviderPort` | streaming de chat LLM (modelo, temperatura, límites) | task (proxy de proveedores) |

Regla R3: **el núcleo nunca persiste por sí mismo**; toda escritura pasa por
`AgentPersistence`. El consumidor es el único dueño de la base de datos.

### Eventos — sección 6.3/6.4 (invariante SSE, H3)

`AgenteEvento` (tag `tipo`): `token`, `tool_start`, `tool_result` (con `diff`
de líneas), `requiere_aprobacion`, `usage`, `contexto`, `contexto_detalle`,
`error` (mensaje presentable, sin internos), `done`. El transporte (SSE de
task, daemon loopback) serializa **este mismo contrato** en todos los
consumidores; el frontend no cambia.

### Frontera (sección 5.3/6.1)

- `core/` **no importa** `src/` de task, **no usa sqlx** y no conoce tablas.
- Las tools agnósticas (`file_read/file_write/web_search/…`) se registran con
  el trait `AgentTool` (OCP público); el consumidor registra sus tools de
  dominio (`crear_tarea`, `crear_habito`, …) contra el mismo trait.
- Task implementa los puertos y conserva: handlers HTTP/SSE, repositorios
  sqlx, rate limits por usuario/hora y el frontend del plugin.

## Consumo

- **Como lib (Fase 2):** `glory-harness-core = { path = "../glory-harness/core" }`.
- **Como CLI/daemon (Fase 3):** `glory-harness run --prompt "..."` /
  `glory-harness daemon` (SSE en loopback, token de sesión, multi-sesión).

## Gate

```text
npm run quality:setup    # evidencia release de los analyzers (nunca fabricada)
npm run quality:doctor   # sentinel doctor --json --workspace .
npm run quality:analyze  # sentinel analyze → .quality-reports/analyze.json
```

El checkout compartido de los analyzers vive en `area-trabajo/.quality-tools/`
(declarado en `quality-tools.json` con commits fijos); la evidencia se genera
por máquina en `.sentinel/release-evidence/` (gitignored).

## Estado por fase (checklist en el plan)

- **Fase 0** ✅ skeleton (workspace core+cli), contrato documentado, gate propio verde.
- **Fase 1** en curso — traits definidos; portar módulos agnósticos.
- **Fase 2-4** pendiente (integración lib en task, CLI/daemon, segundo consumidor).