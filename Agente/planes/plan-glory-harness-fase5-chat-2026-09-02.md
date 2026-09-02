# Plan: Glory Harness — Fase 5: modo chat interactivo (`glory-harness chat`)

- **Fecha:** 2026-09-02
- **ID:** 318A-13 · Fase 5 (extiende el plan base; **no** usar `318A-14`: ya existe en el roadmap de PROYECTO TASKS para otra tarea, tabs compartidos — evitar colisión)
- **Estado:** ✅ completado (02-09-2026, REPL lineal + TUI — opción C híbrida)
- **Plan base:** `plan-glory-harness-2026-09-01.md` (318A-13, Fases 0-4 ✅)
- **Tipo:** nueva interfaz de consumo (TUI/REPL interactiva sobre el contrato `AgenteEvento`)
- **Referencias de código (clonadas localmente, solo lectura):** ver
  `data/referencias-cli/README.md` — `claurst` (Rust+ratatui, la más aplicable),
  `grok-cli` (TS+OpenTUI), `opencode` (TS+React TUI).

---

## 0. Resumen ejecutivo (para el usuario)

Hoy `glory-harness` responde **un turno por invocación** (`run --prompt`/`--stdin`)
o como **daemon de fondo** (NDJSON/TCP). No abre un **chat en la terminal** como
hacen los CLI de agentes de código (Codex, Claude Code, opencode, grok): un
espacio donde escribes, ves los mensajes y la conversación continúa.

**Fase 5** añade `glory-harness chat`: un modo interactivo en la terminal que
mantiene **la misma conversación** entre turnos (el agente recuerda lo anterior),
trabaja en la carpeta donde se invoca (o `--dir`), usa por defecto **Laguna S 2.1
free** con fallback a gloryapi, y muestra los eventos del contrato (`AgenteEvento`)
de forma legible (tokens del asistente, tools ejecutadas, errores).

## 1. Problema real

1. **Sin interfaz conversacional:** `run` es one-shot (un prompt → una respuesta y
   se acaba) y `daemon` es un servicio (para un cliente programático). No hay forma
   de "sentarse a conversar" con el agente desde la terminal, que es el caso de uso
   natural de un CLI de agente de código.
2. **Sin historial entre turnos en el CLI:** cada `run` crea una conversación nueva
   (`conversacion_id` nuevo, `Vec::new()` de historial). Un chat necesita pasar el
   historial acumulado de un turno al siguiente en la misma conversación.
3. **UX plana:** el CLI no muestra progreso (tools ejecutándose, errores, contexto)
   de forma amigable; vuelca el texto final a stdout y los eventos a stderr.

### Resultado deseado

1. `glory-harness chat` abre una sesión interactiva en la terminal:
   - imprime un banner (modelo activo, workspace);
   - lee el prompt del usuario;
   - ejecuta el turno sobre el **mismo `conversacion_id`** y con el **historial
     acumulado** (para que el agente recuerde el hilo);
   - muestra la respuesta del asistente y, de forma discreta, las tools ejecutadas
     y los errores;
   - vuelve a esperar el siguiente mensaje.
2. Comandos de control del chat: `/salir` (o Ctrl+C/Ctrl+D), `/nuevo` (reinicia la
   conversación), `/ayuda`. Prefijos extensibles.
3. Reutiliza el **mismo contrato y runtime** del núcleo: sin cambios en
   `AgenteEvento`, en `AgentRuntime::ejecutar_turno` ni en el core (R1: paridad).
   El modo chat es **un cliente más** sobre la API existente (como previó el plan
   base §"Vías de consumo": "también TUI interactiva sobre el mismo contrato").
4. Comportamiento por defecto coherente con `run`: workspace = cwd (o `--dir`),
   Laguna free, fallback gloryapi, `AGENTE_MODO=local` para tools de archivo.

### No-goals (fuera de alcance)

- **No** añadir dependencias pesadas de TUI si no se necesitan: evaluar primero un
  REPL **lineal simple** (leer línea → respuesta → línea) con crate ligero de
  lectura de línea; ratatui/crossterm solo si el usuario quiere una TUI enriquecida
  (múltiples paneles, scroll, markdown renderizado). Decisión pendiente de
  validación.
- **No** tocar el núcleo (`core/`) ni el contrato; **no** tocar el default global
  de `TurnoConfig` (task depende de glory/commandcode).
- **No** separar la UI web de task; el chat de terminal es un cliente nuevo.
- **No** deploy ni creación de repos remotos.

## 2. Restricciones y dependencias

### Hechos confirmados (verificados 02-09-2026)

| # | Hecho | Evidencia |
|---|---|---|
| H1 | `run` es one-shot: crea `conversacion_id` nuevo y pasa `Vec::new()` como historial a `ejecutar_turno` | `cli/src/run.rs` (`ejecutar_turno_run`) |
| H2 | `AgentRuntime::ejecutar_turno(user_id, turno_id, conversacion_id, historial: Vec<AiMessage>, mensaje_usuario, tx)` ya recibe el historial por parámetro → un chat solo necesita **acumular** mensajes y pasarlos | `core/src/runtime.rs` |
| H3 | `PersistenciaMemoria::listar_mensajes(conversacion_id)` devuelve el historial ordenado (rol/contenido) y `guardar_mensaje` lo persiste en memoria → sirve como fuente del historial entre turnos del chat | `cli/src/persistencia.rs` |
| H4 | `AgenteEvento` incluye `Token{texto}`, `ToolStart`, `ToolResult`, `Error{mensaje,retryable}`, `Done` — el chat puede consumirlos igual que `run`/daemon | `core/src/evento.rs` |
| H5 | El CLI carga `~/.glory-harness.env` y por defecto usa Laguna free + workspace cwd + `AGENTE_MODO=local` | `cli/src/main.rs`, `cli/src/run.rs` |
| H6 | El plan base ya previó "TUI interactiva sobre el mismo contrato" como vía del CLI | `plan-glory-harness-2026-09-01.md` §"Vías de consumo" (L244) |
| H7 | Repos de referencia clonados localmente: claurst (ratatui), grok-cli (OpenTUI), opencode (React TUI) | `data/referencias-cli/` |

### Supuestos (a validar en ejecución)

- S1: el modo mínimo (REPL lineal con lectura de línea) satisface el caso de uso y
  evita dependencia de ratatui; si el usuario pide TUI enriquecida se añade.
- S2: acumular el historial en memoria (usando `PersistenciaMemoria`, que ya
  conserva mensajes por `conversacion_id`) es suficiente para la sesión de chat.
- S3: el `SYSTEM_PROMPT` + memoria/skills base por defecto del runtime aplican
  igual en el chat (misma construcción que `run`).

### Riesgos

- **R1 — Romper el contrato o el core:** el chat NO debe modificar el núcleo. Mitigación: el modo chat vive en `cli/`, es un cliente más.
- **R2 — Historial que crece sin límite:** en un chat largo el historial acumulado puede crecer; el runtime ya compacta por ocupación de ventana (`AgentContextManager`), así que el crecimiento se autocontrola. Validar en prueba larga.
- **R3 — UI de terminal en Windows:** la lectura de línea y Ctrl+C/Ctrl+D en Windows tienen matices (modo raw vs cooked). Validar con un crate de lectura de línea probado en Windows (p. ej. `rustyline`) o implementación acotada con `crossterm` si se decide TUI.
- **R4 — Confundir `modo` del runtime:** el chat debe usar `modo: "predeterminado"` (o permitir `--modo`) para que las tools con efecto pidan aprobación como en `run`; no cambiar la política por defecto.

## 3. Decisiones de diseño

### 3.1 Alcance de la interfaz (decidir con el usuario)

| Opción | Descripción | Coste | Encaje con glory-harness (Rust) |
|---|---|---|---|
| **A. REPL lineal** (recomendado para v1) | Prompt `gh> `, escribe → turno → respuesta en línea; `/salir`, `/nuevo`. Markdown en texto plano. | Bajo (crate ligero de línea) | Directo; sin dependencias de TUI |
| **B. TUI enriquecida** | Paneles (mensajes / input / tools), scroll, colores, markdown renderizado | Medio-alto (ratatui+crossterm) | Más trabajo, UX superior; referencia claurst |
| **C. Híbrido** | REPL lineal por defecto; flag `--tui` para la versión enriquecida | Medio | Escalable |

**Recomendación:** A para v1 (mínimo viable que resuelve "un chat donde escribo y veo
mensajes"), con la API interna del bucle separada para poder añadir B después sin
reescribir. Referencia de UX para B: `claurst/src-rust/crates/tui/` (app/run.rs,
app/turns.rs).

### 3.2 Persistencia del historial en el chat

Reutilizar `PersistenciaMemoria` (H3), que ya indexa mensajes por `conversacion_id`:

- al abrir `chat`: un `conversacion_id` fijo para la sesión (o nuevo con `/nuevo`);
- antes de cada turno, pasar `listar_mensajes(conversacion_id)` como `historial`
  (convertido a `AiMessage`), o mantener el vector acumulado en el bucle;
- tras cada turno, `guardar_mensaje` del usuario y de la respuesta del asistente
  (igual que hace `run` para la respuesta; el usuario lo persiste antes de llamar).

Esto hace que el LLM reciba `system + historial previo + mensaje nuevo` → el agente
**recuerda el hilo** entre turnos (H2).

### 3.3 Consumo de eventos en el chat

Mismo patrón que `run` (`run.rs`), pero con salida formateada:

- `Token{texto}` → acumular y pintar la respuesta (tras el turno);
- `ToolStart{tool}` → línea discreta `⏱ herramienta: <tool>` (stderr, como `run`);
- `ToolResult{ok:false,...}` / `Error{mensaje}` → línea de error legible;
- `Done` → fin del turno, devolver el foco al prompt.

### 3.4 Comandos de control

- `/salir` o `/exit` o Ctrl+C / Ctrl+D → termina la sesión con exit 0.
- `/nuevo` o `/reset` → nuevo `conversacion_id` (limpia historial de la sesión).
- `/ayuda` → lista los comandos y el estado (workspace, modelo activo).
- Si el prompt empieza por `/` y no es un comando conocido → aviso, no se envía al LLM.

### 3.5 Estructura de código (en `cli/`)

```text
cli/src/
  main.rs     → dispatch `chat` + flags (--dir/--provider/--modelo)
  chat.rs     → NUEVO: bucle del chat (leer → turno → mostrar → repetir)
  run.rs      → se reutiliza turno_config_default(), quitar_prefijo_verbatim(),
               ejecutar_turno_run() (o se extrae un helper común construir_runtime())
```

Idea: extraer un helper `construir_runtime(opciones)` compartido por `run` y `chat`
para no duplicar la construcción del runtime/config/workspace. Sin tocar el core.

## 4. Verificación (Definition of Done)

- [x] `glory-harness chat` abre el REPL, ejecuta un turno real (Laguna free) y la
      respuesta aparece; con fallback a glory si Laguna falla. (Evidencia: turno
      real con respuesta completa; modelo banner `commandcode/poolside/laguna-s-2.1-free`.)
- [x] Tras dos mensajes en el mismo chat, el agente recuerda el contexto del primero
      (verificación real de historial acumulado: 2º turno referenció la reunión
      dicha en el 1º; tras `/nuevo` ya no la recordaba).
- [x] `--dir` hace que el chat trabaje en esa carpeta (file_read real sobre el
      workspace: `⏱ file_read` + primera línea de README.md; `--dir C:\tmp\gh-chat-test`
      acotó el workspace y el banner lo mostró).
- [x] `/nuevo` reinicia la conversación (el agente ya no recuerda lo anterior).
- [x] `/salir` y Ctrl+C terminan con exit 0; Ctrl+D también (EOF verificado con
      pipeline sin `/salir` → exit 0; Ctrl+C vía `tokio::signal::ctrl_c`).
- [x] `cargo build`/`test --workspace` (44/44 core + 2 tests nuevos del chat:
      `historial_preserva_orden_y_roles`, `historial_vacio_es_vacio`); clippy
      limpio en `cli` (core: 4 warnings preexistentes, sin tocar — R1).
- [x] Gate Sentinel 318A-13 PASS tras incluir cambios (`quality:analyze` → 0
      hallazgos).
- [x] Commit por bloque con mensaje `318A-13 (Fase 5): ...`; README actualizado con
      el modo `chat`.

### Bloque 2 (02-09-2026): pendientes cerrados — clippy core + TUI + Ctrl+C

- [x] **Clippy `-D warnings` verde en todo el workspace**: corregidos los 6
      warnings preexistentes en `core/` (2 `too_many_arguments` en `llm.rs`
      mediante struct `SolicitudStream`; 2 `redundant_closure` en `context.rs`;
      2 `bool_assert_comparison` en tests de `sandbox.rs`/`scheduler.rs`/
      `contrato_tests.rs`). Tests 46/46 tras el refactor (sin cambio de
      semántica).
- [x] **TUI enriquecida (opción C, `--tui`)**: `cli/src/tui.rs` (ratatui +
      crossterm) con panel de conversación, panel de entrada, cabecera con
      modelo/workspace y barra de estado. Comparte el bucle con el REPL vía
      `procesar_turno`/`construir_harness` extraídos en `run.rs` (sin tocar el
      core, R1). Atajos: `Enter` envía, `Esc`/`Ctrl+C` salen, `Backspace`/
      flechas editan; los comandos `/salir` `/nuevo` `/ayuda` aplican igual que
      en el REPL. `Right` ya no sobrepasa el final del buffer (bug real
      corregido con test unitario). Fix posterior (02-09-2026): los dos
      `blocking_send` a canales tokio paniqueaban con "Cannot block the current
      thread from within a runtime" al enviar un mensaje o al llegar un evento
      de tool; sustituidos por `.await` y una tarea independiente
      (`relevar_estado`) con test de regresión. Render verificado con pipeline:
      pantalla alterna, cabecera, paneles y cursor estables; entrada por pipe no
      llega a crossterm en Windows (limitación de verificación interactiva, no
      defecto).
- [x] **Ctrl+C en REPL**: verificado en terminal interactiva real (sale con
      exit 0); en `--tui` se limpia la entrada (segundo Ctrl+C sale).
- [x] Gate Sentinel tras el bloque: `quality:analyze` → 0 errores (6 warnings
      preexistentes en core/chat, 0 en `tui.rs` tras dividir `tui()` y
      `spawn_worker`).

## 5. Siguiente paso

1. ~~Validar con el usuario la opción de UI (3.1)~~ ✅ cerrado: se implementó la
   opción C híbrida (`--tui`) — REPL lineal por defecto + TUI enriquecida a
   demanda, compartiendo bucle y contrato.
2. ~~Elegir el crate de lectura de línea~~ ✅ resuelto sin dependencias para el
   REPL (hilo de stdin + `tokio::select!`); ratatui + crossterm solo en `--tui`.
3. ~~Implementar `chat.rs` + dispatch en `main.rs` + helper compartido~~ ✅ hecho
   (bloques 1 y 2).
4. ~~Probar funcional (turno real, historial, `/nuevo`, `/salir`) y gate~~ ✅
   hecho: 49/49 tests, clippy `-D warnings` limpio en todo el workspace, gate
   Sentinel 0 errores, commits por bloque.

Pendiente fuera de alcance (reportado, no oculto): ningún consumidor de
producción del daemon elegido aún (Fase 4, decisión del usuario) y evidencia de
turno SSE real con proveedor externo en task (Fase 2).
