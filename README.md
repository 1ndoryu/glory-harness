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
  `glory-harness daemon` (NDJSON TCP en loopback, token de sesión, multi-sesión).
- **Como chat interactivo (Fase 5):** `glory-harness chat` (REPL en la terminal
  que mantiene la misma conversación entre turnos).

## CLI `run` — un comando, desde cualquier carpeta

El binario se instala en `~/.cargo/bin/glory-harness.exe` (ya en `PATH`), así que
se invoca desde cualquier directorio con un solo comando y **trabaja en la carpeta
donde se ejecuta** (o en `--dir`):

```bash
# Desde cualquier carpeta (usa el cwd como workspace y laguna free por defecto)
glory-harness run --prompt "¿qué hace este proyecto?"

# Pipeline (stdin)
echo "resume este README" | glory-harness run --stdin

# Otra carpeta de trabajo y/o forzar proveedor/modelo
glory-harness run --dir "C:\ruta\proyecto" --prompt "lista los archivos"
glory-harness run --provider glory --modelo commandcode --prompt "hola"
```

- **Modelo por defecto:** Laguna S 2.1 free (`commandcode/poolside/laguna-s-2.1-free`,
  cuesta $0). Si falla (sin key, 401/503), el núcleo **salta solo** a la cadena de
  respaldo: gloryapi/auto → DeepSeek directo → groq/cerebras (orden en
  `CHAT_FALLBACK_CHAIN` del core).
- **Workspace:** la raíz es el cwd (o `--dir`). Con `AGENTE_MODO=local` (default del
  CLI) se activan las tools de archivo (`file_read/file_write/…`) acotadas a esa
  carpeta: el agente puede leer/escribir el proyecto real.
- **Claves LLM:** el CLI carga `~/.glory-harness.env` si existe (formato
  `CLAVE=valor`, solo define las que falten; nunca las imprime). Cópialo una vez
  desde las claves de tu proyecto para que funcione en cualquier carpeta, o define
  las variables de entorno correspondientes.
- Flags de `run`: `--prompt/-p/--mensaje`, `--stdin`, `--provider/--proveedor`,
  `--modelo/--model`, `--dir/--cwd/--workspace`.

## CLI `chat` — sesión interactiva (Fase 5)

Abre un chat en la terminal: escribes un mensaje, el agente responde y la
conversación **continúa** (el historial acumulado se pasa de un turno al
siguiente, así el agente recuerda el hilo). Misma configuración por defecto que
`run` (workspace = cwd o `--dir`, Laguna free con fallback, `AGENTE_MODO=local`
para tools de archivo) y mismo contrato `AgenteEvento`: las tools ejecutadas se
muestran discretamente (`⏱ file_read`), los errores también.

```bash
glory-harness chat                      # chat en la carpeta actual
cat notas.txt | glory-harness chat      # entrada por pipeline (EOF cierra)
glory-harness chat --dir "C:\ruta\proyecto" --provider glory --modelo commandcode
glory-harness chat --tui                # TUI enriquecida (paneles, scroll, atajos)
```

Comandos del chat:

- `/salir` (o `/exit`) — termina la sesión (exit 0); también Ctrl+C o EOF
  (en Windows, Ctrl+Z+Enter).
- `/nuevo` (o `/reset`) — reinicia la conversación (el agente olvida lo anterior).
- `/ayuda` — lista los comandos y el estado (workspace, modelo activo).

Un mensaje que empiece por `/` y no sea un comando conocido se avisa y no se
envía al LLM. Interfaz híbrida (opción C del plan): REPL lineal por defecto y
`--tui` para la versión enriquecida (ratatui + crossterm: panel de conversación,
panel de entrada, atajos de teclado); ambas comparten el mismo bucle
(`procesar_turno`) y el mismo contrato `AgenteEvento`. En `--tui`: `Enter`
envía el mensaje, `Esc` o `Ctrl+C` salen (restaurando la terminal), y los
comandos `/salir`, `/nuevo`, `/ayuda` funcionan igual que en el REPL.

## Segundo consumidor (Fase 4): cliente del daemon

`examples/consumidor-daemon.mjs` es un cliente de ejemplo que consume
`glory-harness` **como servicio de fondo** (sin enlazar el crate como lib),
mostrando el contrato NDJSON end-to-end: abre una sesión con token, ejecuta un
turno (recibe el stream de `AgenteEvento`) y la cierra.

```bash
# 1) arranca el daemon con el token conocido
GLORY_HARNESS_DAEMON_TOKEN=mi-token glory-harness daemon --puerto 8798 --mostrar-token
# 2) consume desde otro proceso
node examples/consumidor-daemon.mjs --token mi-token --puerto 8798 --mensaje "hola"
```

Este es el caso de uso del daemon (§5.1 opción B / §5.2): un proceso externo
(task, WANDORIUS, un script o un escritorio) puede delegar turnos en el mismo
proceso de fondo sin acoplarse al build del núcleo. Probado contra el daemon
release: `sesion_abierta` → stream de eventos → `turno_done` → `sesion_cerrada`
(exit 0).

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
- **Fase 4** ✅ segundo consumidor: `examples/consumidor-daemon.mjs` consume el
  daemon como servicio de fondo vía NDJSON (abrir sesión → turno → cerrar),
  probado end-to-end contra el daemon release; sin cambios en el núcleo. Caso de
  uso documentado en "Segundo consumidor". Pendiente: elegir el consumidor de
  producción (p. ej. integrar en WANDORIUS) cuando el usuario lo decida, y
  evidencia de turno SSE real con proveedor externo.
- **Fase 5** ✅ chat interactivo completo: `glory-harness chat` (REPL lineal
  `gh> `) y `glory-harness chat --tui` (TUI enriquecida con ratatui, opción C
  del plan). Historial acumulado entre turnos (el agente recuerda el hilo),
  `/nuevo`, `/salir`/`/ayuda`, Ctrl+C/EOF con exit 0, `--dir/--provider/--modelo`
  y `--tui`, y tools de archivo activas con `AGENTE_MODO=local`. Evidencia
  funcional real: turno Laguna free, memoria entre turnos verificada (y olvido
  tras `/nuevo`), `file_read` real sobre el workspace, EOF exit 0, TUI
  renderizando (cabecera con modelo/workspace, paneles de conversación y
  entrada, cursor). Tests 49/49 (44 core + 2 chat + 3 TUI), clippy limpio en
  todo el workspace (core incluido, 0 warnings), gate PASS (318A-13).