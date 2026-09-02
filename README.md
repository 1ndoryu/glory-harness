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
- **Fase 1** ✅ traits definidos; módulos agnósticos portados al núcleo (44/44 tests core, gate PASS).
- **Fase 2** ✅ integración lib en task: task declara `glory-harness-core` como dependencia path;
  `AgentPersistence`/`WebSearchProvider` implementados en task; 6 módulos huérfanos de task
  eliminados; `AiMessage` con `utoipa::ToSchema` para el OpenAPI del consumidor. Evidencia: `cargo
  check` task 0 errores, tests task 23 pass, gate glory-harness PASS. Gate task pendiente de
  realinear varsense (318A-6VAR, preexistente). Pendiente: evidencia de turno SSE real con
  proveedor externo.
- **Fase 3** ✅ CLI/daemon: binario `glory-harness` con subcomandos `run` (turno
  one-shot), `daemon` (proceso NDJSON TCP en loopback, multi-sesión, token obligatorio),
  `tools` y `doctor`. Persistencia en memoria (`PersistenciaMemoria`) y transporte
  validados end-to-end (abrir sesión → stream `AgenteEvento` → `done` → cerrar; token
  inválido rechazado). `cargo build`/`test --workspace` 44/44, clippy limpio en cli, gate
  PASS (318A-13). Para un turno real con proveedor externo: configurar clave LLM en env.
- **Fase 4** pendiente (segundo consumidor, a validar con el usuario).