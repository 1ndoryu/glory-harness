# Plan 2: Glory Harness — permisos v2, ejecución de comandos y operación del agente

- **Fecha:** 2026-09-03
- **ID:** 318A-16 (libre; no usar en PT hasta cerrar este plan)
- **Estado:** 🚧 en ejecución — F1 ✅, F2 ✅ (verificado en vivo :3001), F3 ✅
  (clasificador + runner CLI; decisión de habilitación del usuario) y F4 ✅
  (file_read por rangos; tests 158 workspace). Pendientes: F5-F6.
- **Base:** plan `318A-15` (`plan-mejora-agente-2026-09-03.md`, completo salvo
  pendientes ajenos) y comparativa `Agente/documentacion/comparativa-opencode-agente-2026-09-03.md`.
- **Referencias (clonadas en `data/referencias-cli/`, solo lectura):** claurst,
  opencode, grok-cli, hermes-agent, vscode — rutas exactas citadas por fase.

---

## 0. Resumen ejecutivo (para el usuario)

El plan 318A-15 dejó el agente con permisos por tool (`ask|allow|deny`), subagentes,
telemetría y compactación dirigida. Pero la **operación diaria** sigue con huecos que
los agentes de código maduros ya resuelven:

1. **Aprobar no es una decisión de 3 botones.** Cuando una tool pide permiso solo se
   muestra una insignia ("requiere aprobación") y el humano debe *escribir* "sí" en el
   chat para que el modelo reintente. No existe **Rechazar / Permitir / Permitir
   siempre**, y "siempre" no se guarda como **regla inteligente por categoría** (tipo
   de comando, lectura fuera del repo, etc.), sino que ni siquiera existe.
2. **No hay tool de comandos** (ni timeout, ni background, ni revisión de comandos
   inseguros) — es la carencia más grande frente a opencode/claude/grok.
3. **Leer archivos es por bytes, no por ventanas**: un archivo de miles de líneas se
   lee entero hasta 1 MB; no hay lectura por rango de líneas.
4. **No hay modo plan explícito**: `meta` niega efectos y ya no hay flujo
   plan → propuesta (diff) → aprobar → aplicar.
5. **Tareas programadas: PT ya las tiene** (cron + heartbeat + API
   `/agente/tareas-programadas`); lo que falta es que el **agente pueda programarlas
   desde la conversación** y que el CLI tenga su superficie `schedule`. Revisado en las
   referencias: grok y hermes las tienen; opencode y vscode **no** (ver Fase 6).

Este plan convierte cada hueco en una fase ejecutable (F1-F6) con checklist, decisión
de diseño fundamentada en las referencias y E2E determinista.

## 1. Problema real (verificado en código)

1. **Aprobación sin decisiones explícitas.** El runtime emite `RequiereAprobacion`
   (`core/src/runtime.rs:458`, `:976`) con la semántica "el LLM recibe el estado y
   pide confirmación" (`runtime.rs:433-436`). En el CLI solo se imprime un aviso
   (`cli/src/chat.rs:138-140`); en el front de PT la tarjeta `AprobacionPendiente`
   muestra solo la tool (`frontend/src/app/plugins/agente/mensajes.tsx:263-270`). No
   hay canal de respuesta (apruebo/rechazo/siempre) ni reglas persistentes: el
   override por conversación existe en `permiso.rs` (`permiso_efectivo`) pero ninguna
   UI lo escribe.
2. **`Permiso` es por tool, no por categoría+patrón.** `core/src/permiso.rs` decide
   solo por nombre de tool y modo; no hay reglas del tipo "lectura fuera del
   workspace → preguntar" o "git push → negar" (categorías), ni coincidencia por
   patrón con última regla ganando.
3. **Sin ejecución de comandos.** No hay tool de shell en el núcleo ni en el CLI;
   los perfiles de subagente la excluyen explícitamente (invariante de 318A-15).
4. **Lectura de archivos sin rangos.** `file_read` trunca a 1 MB con aviso; no hay
   `offset`/límite de líneas (contraste: `read_files` de Codebuff/opencode).
5. **`meta` no es un plan mode.** Niega todo efecto pero el agente no produce una
   propuesta formal (diff) que el humano apruebe y se aplique después.
6. **Tareas programadas sin cara de agente.** PT tiene scheduler cron con heartbeat
   (`core/src/scheduler.rs`, worker en `PROYECTO TASKS/src/main.rs:58-63`) y CRUD HTTP
   (`src/handlers/agente_tareas.rs:179-184`), pero el modelo no tiene una tool para
   crearlas y el CLI no expone nada.

### Resultado deseado

- Toda petición `ask` se resuelve con **3 botones**: Rechazar / Permitir una vez /
  Permitir siempre. "Siempre" crea una **regla por categoría** (no por comando exacto)
  que queda persistida y evaluada con última coincidencia gana.
- El agente del CLI **ejecuta comandos** con clasificador de riesgo, timeout, salida
  truncada, background con readiness y revisión previa según modo (paridad
  claurst/grok/opencode). La IA de task mantiene `deny` permanente (invariante).
- `file_read` acepta **rango de líneas** (offset+límite) con límites duros.
- **Modo plan explícito**: el agente propone (diff), el humano aprueba, el cambio se
  aplica.
- El agente puede **programar una tarea en lenguaje natural** (cron v1 ya existente)
  y el CLI lista/administra tareas.

## 2. Alcance / no alcance

**Sí:**
- Núcleo agnóstico `glory-harness/core`: motor de reglas de permiso (categoría +
  patrón, última coincidencia gana), canal de respuesta de aprobación en el evento,
  clasificador de riesgo de comandos (función pura), lectura por rangos, modo plan.
- CLI `glory-harness chat`: UI de 3 botones (TUI y REPL), tool de comandos con
  runner del consumidor, subcomando `schedule`.
- Front de `PROYECTO TASKS`: tarjeta de aprobación con 3 botones + persistencia de
  reglas por conversación/usuario.
- Tareas programadas: tool de agente para crear/listar/cancelar sobre el scheduler y
  persistencia existentes.

**No:**
- Tool de comandos en la IA de task (queda `deny` total, permanente — invariante del
  plan 318A-15 §2, confirmado aquí).
- Sandboxing de SO/containers para comandos (documentado en F3; fuera de alcance).
- Cambios al contrato SSE de turno salvo los campos nuevos de aprobación (aditivos).
- MCP/LSP/headless browser (ya fuera de alcance en 318A-15).

## 3. Estrategia

```text
Fase 1 (motor de reglas de permiso v2)  ── base de todo (categorías + patrones)
Fase 2 (UI aprobación 3 botones)        ── requiere F1 (regla "siempre")
Fase 3 (tool de comandos con riesgo)    ── independiente (usa F1 para revisión)
Fase 4 (lectura por rangos)             ── independiente (mejora file_read)
Fase 5 (modo plan explícito)            ── requiere F1 (regla de aplicar) y diff.rs
Fase 6 (tareas programadas para el agente) ── independiente (scheduler ya existe)
```

- **Primer bloque: F1 + F2** — la petición explícita del usuario (3 botones +
  "permitir siempre" inteligente); F1 es puro y F2 le da cara.
- **F3 es la decisión de producto más grande**: el plan detalla el diseño (riesgo por
  niveles claurst, timeout/truncado/background grok) y la deja como *decisión del
  usuario* en su último ítem (habilitar por defecto en el CLI o solo modo autónomo).
- **F6 resuelve la duda "¿otros agentes tienen tareas programadas?"** con el
  inventario de referencias y una decisión explícita (ver §4.6).
- Cada fase se cierra con tests + clippy + `sentinel analyze` + E2E determinista sin
  proveedor real; commits por fase en español con prefijo `318A-16`.

## 4. Fases

### Fase 1 — Motor de reglas de permiso v2 (categorías + patrones)

**Problema:** el permiso es por tool; el usuario no puede decir "esta *clase* de
acción siempre permitida" sin abrir la tool entera.

**Diseño (verificado en opencode):**
- `opencode/packages/opencode/src/permission/index.ts:28-32`: regla =
  `{ permission (wildcard), pattern (wildcard), action: allow|deny|ask }`, evaluadas
  con **última coincidencia gana** (`findLast`). `:75` deny corta; `:80` allow sigue;
  `:216-220` `visibleTools` oculta la tool si hay deny con patrón `*`.
- Categorías a modelar (port de `code-mode.ts`/`permission.shared.ts` de opencode:
  `external_directory`, `webfetch`, `bash`, `edit`):
  1. `escribir:<tool>` (file_write/file_patch) con patrón de ruta — dentro del
     workspace vs **fuera** (categoría `escritura_fuera_repo`).
  2. `leer:<tool>` con patrón — incluye **lectura_fuera_repo** (opencode
     `external_directory`).
  3. `comando` (F3): patrón = tipo de comando/riesgo, no comando literal.
  4. `red` (web_fetch/web_search).
  5. `subagente` (task).
- La regla "siempre" que cree la UI (F2) se guarda como `{categoria, patrón, allow}`
  con el patrón **generalizado** (p. ej. `bash:git *` para cualquier subcomando git,
  no el comando exacto).

- [x] Core: `regla.rs` con `ReglaPermiso { categoria, patron, accion }` +
      `evaluar_reglas`/`reglas_coincidentes` (última coincidencia gana) y
      wildcard propio `*` (no cruza `/`) / `**` (cruza todo).
- [x] Core: `permiso.rs` extiende la resolución — orden fail-closed: override
      deny de conversación corta primero; si no, regla v2 sobre la clave más
      específica de la llamada; si no, override/default del modo. Testeado.
- [x] Core: `regla::categorias_core` — tabla estática (tool → categoría +
      clasificador desde argumentos: ruta dentro/fuera, red, subagente); las
      tools del consumidor caen a su id con patrón `*`.
- [x] Tests: wildcard, precedencia findLast (deny específico y reciente gana),
      deny oculta la tool del schema, override deny de conversación fail-closed,
      deny `escritura_fuera_repo` no afecta lecturas ni escrituras dentro.
- [x] E2E determinista: fixture en `tool.rs::tests` (registry con stub
      file_write/web_search) → `permiso_para_llamada` por regla + `schemas_openai`
      resultante. Sin proveedor: decisión pura sobre la llamada.

  **Nota semántica verificada contra glob(7)/opencode:** el patrón `*` no cruza
  `/`, por lo que una regla de clase sobre rutas (p. ej. `escritura_fuera_repo`)
  usa `**` como patrón comodín total; la UI de F2 generalizará igual.

**Criterio de éxito:** con reglas `{escritura_fuera_repo → ask}` y
`{bash:git * → allow}`, un `file_write` a `../x` pide aprobación y un
`comando "git status"` se ejecuta sin preguntar — sin proveedor LLM.

### Fase 2 — UI de aprobación: Rechazar / Permitir / Permitir siempre

**Problema:** hoy la aprobación es conversacional (insignia + "escribe sí").
**Referencia:** opencode `permission.shared.ts`: máquina de estados
`permission → [Allow once / Always / Reject]`, y `always` exige **paso de
confirmación** (`Confirm/Cancel`) antes de persistir la regla.

- [x] Core: evento de aprobación con `id` de petición y respuesta:
      `RespuestaAprobacion { aprobar | rechazar | siempre }` como método del runtime
      (canal explícito; el modelo NO reintenta tras rechazo explícito — hoy solo no
      reintenta tras deny). Hecho: `core/aprobacion.rs` + `PeticionAprobacion` en el
      contrato + `responder_aprobacion` en el runtime; rechazo/deny sin reintento.
- [x] CLI REPL+TUI: ante `RequiereAprobacion`, 3 opciones tecladas/botones
      (Rechazar / Permitir / Permitir siempre + confirmación); "siempre" guarda la
      regla (F1) en la persistencia del CLI. Hecho: `resolver_aprobaciones` en
      `cli/chat.rs` y gate equivalente en `cli/tui.rs` (resolución entre turnos;
      la regla de clase se persiste en el registry compartido del runtime).
- [x] Front PT (`PanelAgente`/`mensajes.tsx`): reemplazar la insignia por 3 botones;
      "Permitir siempre" persiste vía endpoint nuevo de overrides por conversación.
      Hecho: `responderAprobacion` en `service.ts`+`store.ts`, tarjeta de 3 botones
      (Rechazar/Permitir/Permitir siempre) en `mensajes.tsx`, estilos en
      `panelIA.css`; la petición llega por el evento SSE `peticion_aprobacion`.
- [x] Backend PT: endpoint para responder la aprobación y para listar/borrar reglas
      de la conversación (aditivo al SSE). Hecho: `handlers/agente_aprobacion.rs`
      (POST aprobación con 3 decisiones, GET/DELETE reglas; almacén por conversación
      en `AppState.agente_permisos`, TTL 10 min para tokens de una vez); el stream
      siembra reglas/tokens en el registry antes del primer tool_call. Verificado en
      vivo contra :3001: login → aprobación "siempre" → regla `categoria/** allow`;
      "aprobar" → token de una vez; GET/DELETE reglas OK.
- [x] Tests: rechazo explícito → sin reintento en el turno; siempre → regla creada y
      aplicada en la siguiente petición igual. Hecho: `f2_*` en `core/tool.rs`
      (aprobar una vez consume token, siempre crea regla de clase, rechazar crea
      deny y no reintenta, id desconocido → error, misma tool supersede).
- [x] E2E: simular una petición `ask`, responder las 3 vías y verificar el evento y
      la regla resultante. Hecho: los tests `f2_*` recorren el ciclo completo
      registrar-petición → responder → volver a decidir (determinista, sin
      proveedor); más los unit de `aprobacion.rs`.

**Nota (pendiente PT, ítems 3-4):** el front `PanelAgente`/`mensajes.tsx` y el
endpoint backend de PT se ejecutan en la fase siguiente (dependen del árbol de
PROYECTO TASKS; no hay diffs ajenos que lo bloqueen hoy).

**Criterio de éxito:** cada `ask` se resuelve con 3 botones y el "siempre" crea una
regla por categoría que evita volver a preguntar para la misma *clase* de acción.

### Fase 3 — Tool de comandos con revisión de riesgo (CLI)

**Problema:** no hay ejecución de comandos; cuando exista necesita espera acotada,
timeout, salida acotada, background y revisión previa de comandos inseguros.
**Referencias:**
- claurst `src-rust/crates/core/src/bash_classifier.rs`: niveles de riesgo
  `Safe < Low < Medium < High < Critical` (read-only, writes dev, rm/kill/systemctl,
  sudo/pipe-to-shell, destructivos), auto-aprobación según `PermissionMode`
  (`lib.rs:1121-1127`: Default/AcceptEdits/BypassPermissions/Plan).
- grok `src/tools/bash.ts`: `MAX_TAIL_BYTES = 8192` (trunca salida), procesos en
  background con log propio, `MAX_BACKGROUND_PROCESSES = 8`.
- opencode: la petición `bash` muestra el comando y su diff/categoría antes de
  ejecutar.

**Diseño propuesto (agnóstico):** el núcleo aporta el **clasificador puro** de riesgo
(port fiel de claurst) y la tool `comando` se registra **solo si el consumidor inyecta
un runner** (el CLI lo hace; PT no). El runtime aplica la regla de F1 con la categoría
`comando:<riesgo>`; el runner del CLI impone timeout (p. ej. 120 s), salida truncada a
8 KB, flag `background` con log propio y comando `comando_status` para consultarlo.

- [x] Core: `bash_clasificar.rs` (port fiel claurst: niveles, wrappers `sudo/env`,
      flags peligrosos, pipe-to-shell) — puro y con tests del clasificador.
      *(318A-16 (F3): port con 12 tests, franjas crítico→alto→medio→bajo)*
- [x] Core: trait `EjecutorComando` (puerto) + registro de la tool `comando`
      únicamente cuando hay runner (fail-closed: sin runner → deny).
      *(runner `None` en PT: las tools no se registran; el tope de riesgo del perfil
      subagente actúa como defensa en profundidad)*
- [x] CLI: runner con timeout 120 s, truncado a 8 KB, background (log propio) y
      `comando_status`/`comando_matar`.
      *(`ejecutor.rs`, 4 tests de runner real: síncrono, truncado, fondo, matar)*
- [x] Perfiles de subagente del CLI: `explorar` declara tope de riesgo `Safe`; ningún
      perfil de PT incluye comandos (invariante). *(subagente.rs)*
- [x] Tests: clasificador (git status Safe, `rm -rf /` Critical, `curl | bash`
      High), timeout, truncado, background. *(154 tests workspace verdes)*
- [ ] **Decisión del usuario (ítem final, se queda sin marcar si no responde):**
      ¿`comando` habilitado por defecto en el CLI `predeterminado` (con `ask` para
      riesgo ≥ Medium) o solo en `autonomo`? El plan recomienda **habilitado con
      ask ≥ Medium**, paridad opencode. *(implementado con regla F1 `comando:<nivel>`
      + ask según reglas del modo; la habilitación por defecto del modo
      `predeterminado` queda a decisión del usuario)*

**Criterio de éxito:** `comando "git status"` en `predeterminado` corre sin preguntar
(Safe), `comando "sudo rm -rf /x"` se bloquea (Critical, deny por regla) y un comando
largo en background responde con id y permite consultar su salida — sin proveedor LLM
(la revisión es del runtime, no del modelo). *(verificado: tools registradas en vivo
`glory-harness tools`; `sudo`/`rm -rf` clasifican Alto/Crítico por regla)*

### Fase 4 — Lectura de archivos por rangos

**Problema:** `file_read` lee hasta 1 MB sin poder pedir una ventana; leer un archivo
de 5.000 líneas de una vez es caro.
**Referencia:** herramientas de lectura por rango de Codebuff/opencode (offset+límite
de líneas).

- [x] `file_read` acepta `offset_linea` y `limite_lineas` opcionales (1-based), con
      aviso de trunción y total de líneas del archivo.
      *(sandbox.leer_rango_lineas: ventana por líneas sin materializar el archivo
      entero; fail-closed si van por separado o el offset excede el total)*
- [x] El resultado informa el rango leído y sugiere continuar si hay más (formato de
      salida rico: cabecera `[lectura de líneas X-Y de N (hay más; usa
      offset_linea: N+1…)]`).
- [x] Tests: rango válido, rango fuera de límites (fail-closed), archivo grande sin
      rango (comportamiento actual conservado). *(4 tests nuevos, 158 workspace)*
- [x] E2E determinista: fixture de archivo de 200 líneas, leer 1-40 y verificar el
      contrato del resultado. *(test `file_read_rango_valido_devuelve_ventana_con_cabecera`)*

**Criterio de éxito:** `file_read` con `{offset_linea: 1, limite_lineas: 40}` devuelve
solo esas 40 líneas con el rango declarado. *(cumplido: 158 tests verdes, clippy
limpio, sentinel analyze 0 errores)*

### Fase 5 — Modo plan explícito (plan → propuesta → aplicar)

**Problema:** `meta` (318A-15 F3) niega todo efecto, pero no hay flujo de propuesta
formal; claurst tiene `PermissionMode::Plan` (`lib.rs:1126`).
**Diseño:** modo `plan` (alias de `meta` en configs existentes) + tool `proponer`
que recoge el diff acumulado (reutilizar `core/src/diff.rs`) y **no** aplica; el
usuario aprueba el diff (F2) y el cambio se aplica como `file_patch` permitido por la
regla de la aprobación.

- [ ] Core: modo `plan` explícito (comportamiento = meta actual, sin romper configs)
      con la distinción de que las tools de *propuesta* (diff/review) quedan `allow`.
- [ ] Core: resultado de tool con `diff` acumulado en el evento (extender F5).
- [ ] CLI/PT: al cerrar el turno en modo plan, mostrar el diff y botón
      "Aprobar y aplicar" (regla de una sola aplicación).
- [ ] Tests: modo plan no aplica cambios, el diff llega al humano, la aprobación
      aplica exactamente ese diff.
- [ ] E2E determinista: fixture plan → propuesta → aprobar → verificar el archivo.

**Criterio de éxito:** en modo plan el agente edita su propuesta en memoria, el humano
ve el diff y al aprobar se aplica una sola vez.

### Fase 6 — Tareas programadas: ¿otros agentes las tienen? y cara de agente

**Inventario verificado en las referencias (solo lectura):**
| Agente | ¿Programa tareas? | Dónde |
|---|---|---|
| **grok-cli** | **Sí** | `src/daemon/scheduler.ts` (daemon, tick 60 s, pid) + `src/tools/schedule.ts` (`StoredSchedule {instruction, cron, model, directory, enabled, maxToolRounds}`, valida cron, logs por id) — ejecución headless desacoplada |
| **hermes-agent** | **Sí** | `cron/` (`scheduler.py`, `jobs.py`, `executions.py`, `delivery_queue.py`) — cron en lenguaje natural |
| opencode | No (sin cron/scheduler en `packages/opencode/src`) | — |
| vscode | No (solo sesiones de agente en `contrib/chat`) | — |
| **Glory Harness + PT** | **Ya existe (cron v1 + heartbeat)** | `core/src/scheduler.rs`; worker PT `main.rs:58-63`; CRUD `/agente/tareas-programadas` (`agente_tareas.rs:179-184`) |

**Conclusión:** PT/glory-harness ya supera a opencode/vscode y empata con grok en el
motor; el hueco real es que **el agente no puede programar desde la conversación** y
el CLI no expone nada. Hermes añade la idea de **cron en lenguaje natural** (el LLM
traduce "cada lunes a las 9" a cron) — reutilizable.

- [ ] Core: tool `programar_tarea` (crear/listar/cancelar/ver-logs) sobre el puerto
      `tareas_*` existente; cron v1 del scheduler + traducción de lenguaje natural a
      cron como función pura (inspirada en hermes `cron/`), con fallback explícito si
      no se entiende la expresión.
- [ ] CLI: subcomando `schedule` (list/create/remove/logs) que habla con el mismo
      puerto; el worker corre donde lo corra el consumidor (PT ya lo corre; el CLI
      documenta el comando para lanzarlo).
- [ ] Perfiles: `programar_tarea` NO se incluye en subagentes (solo el agente
      principal, como la tool task).
- [ ] Tests: traducción NL→cron (casos válidos e inválidos), CRUD a través del
      puerto con mock, exclusión de subagentes.
- [ ] E2E determinista: fixture "cada lunes a las 09:00" → cron `0 9 * * 1` → tarea
      creada en el mock.

**Criterio de éxito:** el agente crea una tarea programada diciendo "revisa el repo
cada lunes a las 9" y el scheduler la lista con el cron correcto — sin servidor real.

## 5. Orden de ejecución recomendado

```text
F1 (motor reglas) → F2 (UI 3 botones) → F4 (rangos)  [rápidas, puro + cara]
F3 (comandos)     [decisión del usuario en el ítem final]
F5 (modo plan)    [requiere F1 + diff]
F6 (tareas)       [independiente; consultar inventario §4.6]
```

## 6. Evidencia y gate por fase

- `cargo test --workspace` (core + cli) y `cargo clippy --workspace --all-targets -- -D warnings`.
- `sentinel analyze` en glory-harness sobre los archivos tocados; corregir solo
  defectos reales.
- `PROYECTO TASKS`: `cargo check --lib` (reporte) cuando se toque el contrato del
  núcleo; `npm run type-check`/build si se toca el front.
- E2E determinista por fase (fixtures sin proveedor LLM).
- Commit por fase en español, prefijo `318A-16 (F<n>)`, solo los archivos de la fase.

## 7. Decisiones que necesita el usuario

1. **F3 (comandos):** ¿habilitar `comando` en el CLI por defecto (ask ≥ riesgo
   Medium) o solo en modo `autonomo`? Recomendado: por defecto con ask ≥ Medium.
2. **F3 (alcance de binarios):** ¿el clasificador arranca con una whitelist de
   comandos comunes (git, cargo, npm, node, curl, powershell/cmd en Windows) y todo lo
   demás cae a ask? Recomendado: sí (fail-closed).
3. **F2 (persistencia de reglas):** ¿las reglas "siempre" viven por conversación
   (recomendado, simple) o globales por usuario/config (opencode guarda las globales
   en `opencode.json`)? Se puede hacer conversación primero y global después.

Sin respuesta, cada fase se implementa con la opción recomendada y el ítem queda
marcado con nota.
