# Plan: Saneamiento de calidad — deuda Sentinel + auditoría SOLID/rendimiento y brechas del gate

- **Fecha:** 05-09-2026 · **Área:** glory-harness (core + cli; desktop ajeno) y gate
  (glory-sentinel en `.quality-tools-harness/sentinel`, commit `0559576`, v0.7.7)
- **ID de tarea:** `059A-1` (registrado en `roadmap.md`, entrada separada, 05-09).
- **Origen:** `quality:analyze` del 05-09: **0 errores / 21 warnings / 10 archivos**;
  el usuario pidió corregir todo lo que reporta Sentinel sin romper nada, mejorar
  Sentinel ante falsos positivos, y en el mismo plan una auditoría de principios
  SOLID/rendimiento y de cosas que Sentinel **no** detecta y debería detectar.
- **Base de evidencia:** `.quality-reports/analyze.json` (05-09). Gate por fase según
  `AGENTS.md` (flujo de planes 318A-15/16 y B3-F1..F3): unit tests + E2E determinista →
  `cargo test --workspace` verde → `cargo clippy -p glory-harness-core -p glory-harness
  --all-targets -- -D warnings` limpio → `npm run quality:analyze` 0 errores y **cero
  hallazgos nuevos en los archivos de la fase** → checklist del plan al día → commit en
  español con prefijo `059A-N (S#)`.
- **Estado:** S0 ✔ (baseline congelado, 05-09) · S1 ✔ (falsos positivos corregidos en
  glory-sentinel, bump `902c45e` v0.7.8) · S7 ✔ parcial (3 reglas D4 implementadas y
  repineadas; propuestas restantes documentadas en `Agente/documentacion/brechas-gate-2026-09-05.md`)
  · S2 ✔ (split estructural: `f33a4de`, `3910477`, `f3d467b`)  · S3 ✔ (extracción de
  funciones largas, `79022c7`) · S4 ✔ (organización por dominio, `8064c41`) ·
  S5 ✔ (auditoría SOLID documentada, ver checklist) · S6 ✔ (auditoría de rendimiento,
  ver checklist; fix TUI 4151→60 µs/frame documentado en
  `Agente/documentacion/auditoria-rendimiento-2026-09-05.md`) · S8 pendiente.
- **Re-analyze 05-09 (post S1/S7):** 2 errores (ambos `expect-produccion-rs` en
  `desktop/src-tauri/src/vault.rs`, **ajeno 039A-3**, documentado y sin tocar) ·
  21 warnings / 8 archivos en core+cli (deuda de tamaño, S2–S4). core+cli a **0 errores**.

---

## 1. Línea base verificada (05-09, `analyze.json`)

| Archivo | Warnings | Regla(s) | Propietario |
|---|---|---|---|
| `core/src/runtime.rs` | 4 | `limite-lineas` 1111 efectivas (nivel 2), `ejecutar_turno` 380, `ejecutar_subagente` 200 | núcleo (B3) |
| `core/src/llm.rs` | 3 | `limite-lineas` 1053 efectivas (nivel 2), `ejecutar_request_stream` 112 | núcleo (B3) |
| `core/src/aprobacion.rs` | 1 | `directorio-abarrotado`: `core/src/` 28 archivos (max 10) | núcleo (B3) |
| `core/src/diff.rs` | 1 | `funcion-larga-rs`: `diff_lineas` 135 | núcleo (B3) |
| `cli/src/tui.rs` | 4 | `limite-lineas` 906 efectivas, `spawn_worker` 167, `bucle_ui` 120, `dibujar` 117 | cli (B3) |
| `cli/src/chat.rs` | 2 | `directorio-abarrotado`: `cli/src/` 12 archivos; `chat()` 105 | cli (B3) |
| `cli/src/main.rs` | 1 | `funcion-larga-rs`: `cmd_schedule_impl` 123 | cli (B3) |
| `cli/src/persistencia_sqlite.rs` | 1 | `limite-lineas` 891 efectivas | **ajeno (039A-3)** |
| `desktop/src-tauri/src/main.rs` | 3 | `limite-lineas` 958, `abrir_sesion_interna` 126, `enviar_turno` 172 | **ajeno (039A-*)**, sin tocar |
| `Cargo.lock` (raíz) | 1 | `directorio-abarrotado`: raíz 11 archivos (max 10) | gate/config |

Reglas en juego (glory-sentinel): `limite-lineas` 500 efectivas (+nivel 2 = duplicar),
`funcion-larga-rs` 100 efectivas (excluye rangos de test), `directorio-abarrotado` 10
archivos/dir (`LIMITE_ARCHIVOS_DIRECTORIO`, `src/analyzers/static/staticCodeRules.ts:298`).
El conteo de Sentinel usa **líneas efectivas** (sin comentarios): runtime.rs 1719 reales →
1111 efectivas, llm.rs 1592 → 1053, tui.rs 1305 → 906 (los comentarios "memoria" no son
el problema; la deuda es estructural).

**Observación de concurrencia:** otro hilo (039A-3, plan UX turno) trabaja
`cli/src/persistencia_sqlite.rs` + `desktop/` + `sandbox.rs` y commitea en `main`
(aviso ya presente en `roadmap.md`). **S1–S4 solo tocan archivos propios del núcleo/CLI
no modificados por 039A-3**; los archivos ajenos quedan fuera y se coordinan (no se
refactorizan sin que el hilo termine).

---

## S0 — Snapshots y reglas del juego (prerequisito, sin código de producto)

- [x] Congelar el baseline: copia de `.quality-reports/analyze.json` a
      `Agente/completados/` al cierre (evidencia de partida). **Hecho:**
      `Agente/completados/analyze-baseline-2026-09-05.json` (0 errores / 21 warnings /
      10 archivos, glory-sentinel 0.7.7, 45 archivos escaneados).
- [x] Confirmar mecanismo de excepciones del CLI: cómo llega `directoryExceptions` a
      `analizarEstatico` y cómo se excluyen archivos generados. **Verificado en fuente
      `0559576`:** el CLI (schemaVersion 2) lee `analyzers.sentinel.config`
      (`config.ts:186-199 analyzerSubConfig/buildCoreConfig` → `directoryExceptions`);
      `analyzeDocument.ts:50,108` lo pasa a `analizarEstatico`; el mensaje de la regla
      que cita `settings.json`/`codeSentinel` es contexto del LSP de VS Code, **no** del
      CLI — la vía real del CLI es `sentinel.config.json → analyzers.sentinel.config.
      directoryExceptions` (patrón `includes` de subruta o nombre de dir exacto,
      `staticCodeRules.ts:336-342`). Glory-harness hoy no define excepciones → `[]`.
      Exclusión de generados: `excludePatterns` de `sentinel.config.json`
      (`**/target/**`, `.quality-tools`, `.quality-reports`, `.sentinel`) + dirs de
      infraestructura por nombre (`node_modules`, `target`, `.git`, `dist`, `build`,
      `.sqlx`, `completados` — `staticCodeRules.ts:311-319`).
- [x] Confirmar qué cuenta cada regla. **Verificado:** `directorio-abarrotado` cuenta
      **todos los archivos** del dir (via `readdirSync` + `isFile()`, incluye manifests,
      lock, config y ocultos; excluye solo dirs de infra por *nombre*,
      `staticCodeRules.ts:325-334`); por eso la raíz (11 archivos: Cargo.lock,
      Cargo.toml, package.json, configs…) y `src/` planos disparan. `limite-lineas` usa
      `contarLineasEfectivas(texto, esRust=true)` que **trunca en el primer
      `#[cfg(test)]`** y excluye vacías/comentarios (`lineCounter.ts:13-21`) — por eso
      runtime.rs 1719 reales → 1111 efectivas. `funcion-larga-rs` excluye rangos de
      test (`calcularRangosTest`, `rustAnalyzer.ts:92`) y cuenta efectivas por función.
      El **doble reporte `limite-lineas` + `limite-lineas-nivel-2` es intencional**
      (comentario `[225A-1]`: cada escala usa su propio rule id para que desactivar el
      primer aviso no silencie los graves) → **no es falso positivo; se documenta**.
- [x] Registrar la tarea y este plan (ID `059A-1`) en el roadmap del proyecto con
      entrada separada (roadmap verificado limpio en el momento del registro;
      se añade solo la entrada propia, sin tocar la sección de 039A).

**Decisiones S0 (aplican al checklist de cada fase):** las reglas estructurales
(`limite-lineas-*`, `directorio-abarrotado`, `funcion-larga-rs`) son `warning` por
defecto y **no fallan el gate** (ninguna figura como `error` en el `rules` de
`sentinel.config.json`); por tanto **no se usa `sentinel-disable-file`** (R1): la deuda
se corrige en producto (S2–S4) o se decide como excepción de regla en glory-sentinel
(S1). Lo único que cita `settings.json` es el mensaje de la regla para el LSP; la
corrección del mensaje/regla se decide en S1.

**Criterio de éxito S0:** decisiones documentadas al inicio del checklist de cada fase
(qué se corrige en producto, qué se corrige en el gate, qué se documenta como excepción).
✔ cumplido (ver decisiones S0).

---

## S1 — Falsos positivos y mejoras del gate (glory-sentinel)

Objetivo: que el análisis distinga deuda real de ruido de regla; donde la regla esté
mal, se corrige en **glory-sentinel** (repo aparte, con tests) y se repinea el lock.

- [x] **Cargo.lock/manifests y `directorio-abarrotado` en la raíz:** corregido en
      glory-sentinel (`staticCodeRules.ts`, commit `902c45e`): el conteo de archivos por
      directorio ignora `*.lock`, manifests y config de raíz (no-código). 3 tests de caso
      mínimo en `directorioAbarrotado.test.ts`; re-analyze ya no reporta `Cargo.lock`.
- [x] **Regla por defecto a "error" vs "warning"**: verificado y documentado — las reglas
      estructurales siguen en `warning` por diseño; el gate exige 0 errores y no falla por
      warnings. Cero `sentinel-disable-*` usados (R1 respetada).
- [x] **Doble reporte `limite-lineas` + `-nivel-2`** en el mismo archivo: **intencional**,
      documentado en el código (`[225A-1]` en `staticCodeRules.ts`): cada nivel tiene su
      propio ruleId para que deshabilitar nivel 1 no silencie las severas. Sin cambio.
- [ ] **Cobertura vacía del analyzer:** pendiente de decisión — requiere habilitar perfiles
      TS/SQL y toca `desktop/ui` (ajeno) y PT; queda como propuesta en `brechas-gate` para
      un bloque de gate posterior (decisión D4 restante, no bloquea S2–S4).
- [x] Cada cambio del gate: tests unitarios en glory-sentinel (suite completa 583 passing),
      commits publicados en el checkout (primer bump + `902c45e`), `quality-tools.json`
      actualizado, `sentinel.lock.json` regenerado (hash byte-idéntico documentado),
      `quality:doctor` verde (`ready: true`, `issues: []`, fuente 0.7.8 @ `902c45e`),
      re-analyze ejecutado.
- [x] Repineado **con** las reglas de S7 confirmadas (D4), mismo bump de commit `902c45e`.

**Criterio de éxito S1:** el re-analyze no reporta los falsos positivos decididos como
tales (con evidencia de la regla/caso), y ningún cambio de gate usa `sentinel-disable-*`.

---

## S2 — Archivos grandes del núcleo/CLI (split estructural, cero cambio de comportamiento)

Regla: cada archivo nuevo < 500 líneas efectivas objetivo (≈<350 ideal); **solo mover
código, nunca cambiar lógica**; los tests existentes pasan sin edición de aserciones
(si un test cambia de archivo, se mueve con el código, sin alterar su contenido).

- [x] **`core/src/runtime.rs` (1719 → objetivo ~<450/fragmento):** extraer por dominio:
      1) `bucle.rs`/`turno.rs` — el cuerpo de `ejecutar_turno` (380 efectivas) como
         runner del bucle LLM→tools; 2) `subagente_ejecucion.rs` — `ejecutar_subagente`
         y `_desde_llamada` (200); 3) dejar en `runtime.rs` la orquestación fina
         (struct `AgentRuntime`, `nuevo`, puertos, acceso a `registry`) + re-exports
         `pub use` para no romper consumidores (`cli`, `desktop`, PROYECTO TASKS).
         Atención: no romper los comentarios-memoria ni los `[318A-xx]`/`[Bloque 3]`.
         → `core/src/runtime/mod.rs` (380) + `turno.rs` (389) + `tools.rs` (132) +
         `subagente.rs` (209); commit `f33a4de`.
- [x] **`core/src/llm.rs` (1592):** separar por responsabilidad sin tocar `enviar_chat*`:
      1) tipos del contrato (`AiMessage`, `AiToolCall`, `AiChatResult`, `Keys/…` ya
         exportados: mover a `modelo.rs` con re-export); 2) red/HTTP y reintentos
         (`ejecutar_request*`, `ejecutar_request_stream`, parseo de tool_calls) →
         `red.rs`; 3) `LlmProviderService` (fallos, rotación, nutrición) queda en `llm.rs`
         como fachada fina.
         → `core/src/llm/mod.rs` + `modelo.rs` + `red.rs`; commit `3910477`.
- [x] **`cli/src/tui.rs` (1305):** separar presentación de control: 1) render puro
      (`a_lineas`, `dibujar`, `envolver_*`, `render_markdown_linea`, tipos `Line`/estado
      visual) → `tui_render.rs`; 2) `spawn_worker` (167) y `bucle_ui` (120) → `tui_bucle.rs`
      o helpers por responsabilidad; `tui.rs` conserva el montaje.
      → `cli/src/tui/mod.rs` (458) + `texto.rs` (95) + `render.rs` (316) + `bucle.rs` (462).
- [ ] **`cli/src/persistencia_sqlite.rs` (891, ajeno 039A-3):** NO tocar. Coordinar con el
      hilo 039A-3: si al cerrar su bloque sigue >500 efectivas, refactor propio en fase
      posterior (quedará anotada como pendiente coordinado, no como deuda de B3).
- [ ] **`desktop/src-tauri/src/main.rs` (958, ajeno):** NO tocar (otro hilo/plan).

**Criterio de éxito S2:** `cargo test --workspace` verde sin tocar aserciones;
`quality:analyze` con 0 errores y sin `limite-lineas` en los archivos extraídos; diff
100% movimiento (verificable con `git diff -w` + cuenta de líneas por archivo nuevo).

---

## S3 — Funciones largas restantes (extracción de helpers)

Lista objetivo (efectivas, de la línea base): `ejecutar_turno` 380 (con S2.1),
`ejecutar_subagente` 200 (con S2.2), `spawn_worker` 167, `bucle_ui` 120, `dibujar` 117,
`chat()` 105 (`cli/chat.rs`), `cmd_schedule_impl` 123 (`cli/main.rs`),
`ejecutar_request_stream` 112 (con S2.2), `diff_lineas` 135 (`core/diff.rs`).
Ajenos (NO tocar): `abrir_sesion_interna` 126 y `enviar_turno` 172 (desktop).

- [x] Refactor **`core/src/diff.rs`** `diff_lineas` (135): separar cálculo de línea vs
      ensamblado del diff textual; caso de prueba por cada modo (crear/modificar/borrar).
- [x] Refactor **`cli/src/main.rs`** `cmd_schedule_impl` (123): extraer parseo de
      argumentos, validación y ejecución en 2–3 helpers con tipos claros.
- [x] Refactor **`cli/src/chat.rs`** `chat()` (105): extraer el bucle REPL y el manejo de
      comandos `/…` a helpers (aprovechar S4 de skills si ya separó comandos).
- [x] Con S2 hecho, verificar que **ninguna función del núcleo/CLI** supere 100 efectivas;
      las que queden se extraen aquí con nombre de helper por responsabilidad.
- [x] Regla de oro: cada extracción conserva el flujo exacto (sin cambios de semántica);
      si una extracción "natural" exige tocar lógica, se marca en el plan como decisión
      (no se mezcla con el movimiento).

**Ejecutado:** `diff_lineas` → helpers por fase (LCS/camino/ops/emisión) con tests por
modo · `cmd_schedule_impl` → 4 helpers + parser de id compartido · `chat()` → helpers
REPL/comandos · `ejecutar_request_stream` (llm/red.rs, 112→ok) · TUI (`bucle_ui`, `dibujar`
→ helpers; `spawn_worker` → `resolver_gate_aprobaciones` + fase de turno) · `ejecutar_subagente`
(200→orquestador+helpers) · `red.rs` 522→468 (helpers puros movidos) · `ejecutar_turno`
(380→`turno/` dir: mod.rs + `auditoria.rs` + `permisos.rs`) · `manejar_verdicto_no_ejecutar`
101→ok (helpers `empujar_tool_call_asistente`/`empujar_mensaje_tool_denegado`).
Re-analyze: `funcion-larga-rs`/`limite-lineas-rs` en **0 para core+cli**; 233 tests verdes,
clippy `-D warnings` limpio.

**Criterio de éxito S3:** `funcion-larga-rs` en 0 para core+cli en el re-analyze;
tests + clippy verdes.

---

## S4 — Directorios abarrotados (organización por dominio)

- [x] **`core/src/` (28 archivos planos)** → 4 dominios con re-export plano en `lib.rs`:
      `nucleo/` (runtime+turno, context, llm, plan, subagente), `herramientas/`
      (tool, tools_archivo/web, comando, mcp, skill, todo, tareas, scheduler),
      `politica/` (permiso, regla, aprobacion, bash_clasificar), `contrato/` (ports,
      error, evento, sandbox, plan→nucleo por conteo, guardas, pregunta, telemetria,
      diff, llm→nucleo, contrato_tests). Movido con `git mv` (historia preservada);
      solo se tocaron `mod` internos + `lib.rs` (`pub use`); consumidores intactos.
- [x] **`cli/src/` (12 archivos)** → `comandos/` (daemon, run, chat→ui, mcp_cli),
      `ui/` (chat, tui/*), `infra/` (ejecutor, fetch, persistencia, reglas) con
      re-export plano en `lib.rs`. `persistencia_sqlite.rs` **no** se movió (ajeno).
- [x] **Raíz glory-harness (11 archivos):** excepción de config/manifests aplicada en
      S1 (regla `directorio-abarrotado` no aplica a archivos de config/scripts); sin
      reorganización de raíz.
- [x] Verificación post-reubicación: `cargo test --workspace` 233 verdes, clippy
      `-D warnings` limpio (core+cli), `quality:analyze` **0 hallazgos en core+cli**;
      PROYECTO TASKS `cargo check` report-only: solo falla `missing field web_fetch`
      (port B3-F1 previo, **independiente del layout**, confirmado preexistente).

**Criterio de éxito S4:** `directorio-abarrotado` 0 en core+cli (o excepción documentada
en S1); sin cambios de comportamiento; árbol con `git mv` verificado en el log.

---

## S5 — Auditoría SOLID/arquitectura (con hallazgos reales, no cosmética)

Objetivo: revisar cada servicio del núcleo/CLI contra SRP/OCP/ISP/DIP y dejar un
informe con **hallazgos + evidencia (ruta/línea)** y, cuando aplique, correcciones ya
agendadas en S2–S4 (muchos hallazgos SOLID coinciden con los splits).

Checklist por pieza (marcar `[x]` con hallazgo/acción al auditar):

- [x] **`AgentRuntime` (SRP):** [YA corregido en S2–S4]. Turno→`runtime/turno/`,
      permisos→`turno/permisos.rs`, auditoría/telemetría→`turno/auditoria.rs`,
      subagentes→`runtime/subagente.rs`; el struct (`runtime/mod.rs:225-257`) solo
      coordina (puertos + estado transversal). Evidencia completa en el informe.
- [x] **`LlmProviderService` (SRP/OCP):** [YA corregido en S2] — `llm/` en tres piezas
      (fachada `mod.rs` / datos `modelo.rs` / red `red.rs`); añadir proveedor = fila en
      `PROVIDERS`/`CHAT_FALLBACK_CHAIN` (OCP data-driven); clase nueva de protocolo =
      comportamiento nuevo legítimo.
- [x] **`AgentToolRegistry` (ISP/OCP):** [verificado, YA corregido en F5/F3/B3] —
      núcleo contra trait (`tool.rs:112`); `ejecutar_tool` despacha sin `match` de
      dominio; el `downcast` del slot `dominio` solo existe en el consumidor
      (`tool.rs:9,47`, `runtime/mod.rs:212`); registro abierto (`registrar*`).
- [x] **`tool.rs` 1000+ líneas:** [decisión — no corregir]. Tras S2/S4 el archivo solo
      tiene contrato+registro+política (tools concretas fuera, en `herramientas/`);
      Sentinel no lo reporta. Split mecánico sin ganancia de gate; queda agendado como
      opcional si crece. Detalle en el informe §4.
- [x] **`tui.rs`/`UiEstado`:** [YA corregido en S2/S3] — `UiEstado`
      (`cli/src/ui/tui/mod.rs:127`) solo estado de presentación; sin SSE ni
      persistencia; eventos en `bucle.rs`, render en `render.rs`.
- [x] **Puertos (`ports.rs`):** [verificado — ya granular] — 7 traits por rol
      (`ports.rs:138-345`); `Option` solo para opcionales reales; dividir más rompería
      los repos reales de PT sin segundo consumidor (límite deliberado).
- [x] **DIP en llm:** [decisión — no corregir]. `reqwest` concreto (`llm/red.rs:77`) es
      inyectable como objeto; tests sin red (parseo puro + E2E offline). Trait
      `HttpClient` solo si aparece segundo consumidor o test de reintentos.
- [x] **Errores:** [YA corregido en S7 / verificado] — 11 variantes tipadas
      (`contrato/error.rs:8-30`); `expect-produccion-rs` 0 hits core+cli; locks con
      `unwrap_or_else(|p| p.into_inner())` (`sandbox.rs:307`, `tool.rs:314-332`).
- [x] **Persistencia (CLI):** [verificado] — `cli/src/infra/persistencia.rs` es
      adaptador en memoria del puerto (sin SQL); el único SQL del CLI está en
      `persistencia_sqlite.rs` (ajeno, no movido); el núcleo no contiene SQL.

**Criterio de éxito S5:** informe en `Agente/documentacion/auditoria-solid-2026-09-05.md`
con hallazgos por ítem (evidencia ruta:línea), cada hallazgo marcado como
[YA corregido en S#] / [corregir aquí] / [decisión — no corregir, con razón]. Cero
cambios de comportamiento fuera de S2–S4.

---

## S6 — Auditoría de rendimiento (medible, sin optimización prematura)

Objetivo: detectar cuellos de botella reales con instrumentación corta y fijar
objetivos verificables; corregir solo lo demostrado.

Checklist (marcar con evidencia de medición):

- [x] **Copia de contexto por turno:** medido — 2 clones completos de `Vec<AiMessage>`
      + 1 de schemas por llamada LLM (sin copias en el bucle de tools ni por evento).
      **No problema** (10–100 µs frente a red de 10⁶× más); invariante documentado.
- [x] **`registry.ids()` por iteración del bucle:** 1 vez por llamada LLM (no por tool),
      O(n_tools≈20). **No problema**; caché por turno innecesaria.
- [x] **Persistencia SQLite:** carga sin N+1 (SELECT único por conversación) ✅;
      escrituras = 1 commit por operación (~12–15 commits/turno) y rusqlite síncrono
      bajo `Mutex` en el runtime async. **Hallazgo real pero archivo ajeno (039A-3):**
      recomendación documentada (spawn_blocking + transacción por turno), sin tocar.
- [x] **Streaming SSE:** sin eventos por token — coalescido a 1 `Token` por ronda LLM
      (O(rondas+tools)); serialización mínima. **No problema**; nota de UX (texto por
      rondas, no por token) documentada.
- [x] **Locks en runtime:** `contexto.lock().await` solo durante `preparar_con` (sin red
      bajo el lock); el resto son `std::Mutex` breves sin `.await`. **No problema** en el
      núcleo; la única contención real es SQLite (ajeno, ver ítem 3).
- [x] **Subagente/task:** límites verificados y activos — `SUBAGENTES_EN_CURSO` +
      `concurrencia_permitida` + `GuardiaConcurrencia`, profundidad ≤1 atómica con
      `GuardiaProfundidad` (fail-closed). Spawn ligero en el mismo executor. **No problema**.
- [x] **TUI:** hotspot demostrado y **corregido** — `a_lineas` re-envolvía todo el
      historial por frame: **4151 µs → 60 µs/frame streaming (~70×) y 22 µs/frame idle,
      independientes del largo** (caché de bloques cerrados + re-envuelto de la cola +
      clonado acotado a la ventana visible). Test de regresión añadido (35 CLI).
- [x] Fijar **objetivos**: suite determinista 234 tests (195 core + 35 cli + 4 desktop)
      en ~3.8 s de ejecución debug; streaming acotado por el fix TUI; RAM del release
      pendiente de medir en el próximo cierre (sin estado nuevo por mensaje en S6).

**Ejecutado:** fix TUI en `cli/src/ui/tui/{mod,render}.rs`; informe completo en
`Agente/documentacion/auditoria-rendimiento-2026-09-05.md` con tabla resumen por ítem.

**Criterio de éxito S6:** cada ítem cerrado con número (antes/después) y su fix si aplica;
los que resulten "no problema" quedan documentados como verificados (evita regresiones).

---

## S7 — Brechas del gate: cosas que Sentinel no detecta y debería

Objetivo: lista de **reglas nuevas propuestas** para glory-sentinel con caso mínimo,
basadas en defectos reales ya vividos en este repo. Cada propuesta: reglaId, severidad
sugerida, detección esperada, falso-positivo posible, caso de test.

- [x] **`block-en-async-rs`** (error): implementada en glory-sentinel (`rustReglasNuevas.ts`,
      `902c45e`) con casos mínimo/no-disparo; **0 hits** en el código actual (el `block_on`
      legítimo vive solo en `main()` síncrono y tests). Evidencia del panic histórico en
      `tui.rs:281` queda en la cabecera del módulo.
- [x] **`lock-a-traves-await-rs`** (warning): implementada (heurística por rango guard→`.await`
      sin drop, con purga por bloque y profundidad); **0 hits** actuales en core+cli.
- [x] **`expect-produccion-rs`** (error): implementada como ampliación de
      `unwrap-produccion-rs`, con salto de archivos `#![cfg(test)]`. **3 sitios reales
      corregidos** en core+cli (ver nota) y `contrato_tests.rs` marcado `#![cfg(test)]`;
      los 2 hits restantes son `desktop/src-tauri/src/vault.rs` (**ajeno 039A-3**, sin tocar).
- [ ] **`tool-sin-schema-rs`** o **`registro-sin-nombre`**: fuera de D4 (solo "evaluar") —
      queda como propuesta en `brechas-gate` para el siguiente bloque de gate.
- [ ] **SQL no preparado en Rust**: queda como propuesta (GH no tiene SQL por contrato;
      útil para PT — requiere patrón de crates con SQL).
- [ ] **Cobertura de lenguajes**: queda como propuesta (ver S1.4, pendiente de decisión).
- [x] Para cada regla aprobada: caso mínimo + no-disparo en `rustReglasNuevas.test.ts`
      (7 tests), integradas en `rustAnalyzer.ts` + `ruleRegistry.ts` con severidad
      configurable vía `sentinel.config.json`.

**Nota S7 — 3 expects reales corregidos en core+cli (todos con test verde previo):**
`cli/src/fetch.rs` (builder con error diferido a `Result`, sin cambio de firma),
`core/src/tareas.rs` (extracción sin doble-parse en la rama de cron), `core/src/llm.rs`
(excepción justificada con comentario: `LlmProviderService::new` debe seguir siendo
infallible porque `handlers/mod.rs:311` de PT la llama así; el cliente se construye con
el error diferido). `core/src/contrato_tests.rs` es módulo de fixtures `#[cfg(test)]`
y ahora lo declara en cabecera.

**Criterio de éxito S7:** propuesta consolidada en
`Agente/documentacion/brechas-gate-2026-09-05.md` con las reglas priorizadas; las
aprobadas (decisión D4) se implementan en glory-sentinel con tests y se repinea (S1.5).

---

## S8 — Cierre y evidencia

- [ ] Re-analyze final: objetivo **0 errores y 0 warnings en core+cli** (las excepciones
      decididas en S1 quedan documentadas y visibles en el reporte de cierre, no ocultas).
- [ ] `cargo test --workspace`, clippy core+cli `-D warnings`, doctor, VarSense (si el
      gate lo exige) verdes; PT `cargo check` report-only.
- [ ] Checklists del plan al día; cabecera con estado cerrado y fechas; evidencia en
      `Agente/completados/tareas-2026-09-05.md` (commits, conteos, rutas).
- [ ] Informe final para el usuario: deuda corregida por fase, falsos positivos del gate
      (S1), hallazgos SOLID (S5) y de rendimiento (S6) resueltos/verificados, y reglas
      nuevas del gate (S7) aprobadas/pendientes.

---

## Decisiones que necesita el usuario (con defaults recomendados)

- **D1 — Falsos positivos:** **resuelto (default elegido).** Corregido en glory-sentinel
  `902c45e`: `directorio-abarrotado` ignora `*.lock`/manifests/config de raíz; doble
  reporte `limite-lineas`+`nivel-2` documentado como intencional; severidades
  estructurales siguen en `warning` (el gate exige 0 errores).
- **D2 — Reorganización de `core/src` y `cli/src` (S4):** ¿se mueven a subdirectorios por
  dominio ahora (retoque de `use` interno, historia preservada con `git mv`) o se dejan
  planos con excepción? Default: **reorganizar** (S4), es la corrección que la regla pide.
- **D3 — Alcance de la auditoría SOLID/rendimiento:** ¿solo informar con evidencia (y
  corregir lo que S2–S4 ya cubre) o permitir refactors adicionales que surjan (p. ej.
  dividir `tool.rs`)? Default: **informar + corregir solo lo cubierto por S2–S4**; lo
  demás queda como hallazgo con decisión.
- **D4 — Reglas nuevas del gate (S7):** **resuelto (default elegido).** Entran ya
  `expect-produccion-rs` (error), `block-en-async-rs` (error) y
  `lock-a-traves-await-rs` (warning), implementadas con tests en glory-sentinel `902c45e`
  y repineadas. `tool-sin-schema-rs`, SQL no preparado y cobertura de lenguajes quedan
  como propuestas en `Agente/documentacion/brechas-gate-2026-09-05.md`.
- **D5 — Ajenos:** `persistencia_sqlite.rs`/`desktop` (039A-3): ¿se espera a que el hilo
  cierre o se coordina ahora? Default: **esperar**, dejar anotado como pendiente
  coordinado (no es deuda de este plan).

## Excluidos (no se tocan)

- `desktop/**` y `cli/src/persistencia_sqlite.rs` (hilo 039A-3; aviso en `roadmap.md`).
- PROYECTO TASKS (solo `cargo check` report-only contra el core).
- Cambios ajenos en el working tree y el `roadmap.md` de 039A (si se registra la tarea,
  con entrada separada y solo si el archivo está limpio).

## Evidencia de la auditoría inicial

- `.quality-reports/analyze.json` (05-09, línea base) y `.quality-reports/check/` (gate).
- Reglas citadas: glory-sentinel `src/analyzers/static/staticCodeRules.ts`,
  `src/analyzers/staticAnalyzer.ts`, `src/analyzers/rustAnalyzer.ts`,
  `src/config/ruleRegistry.ts` (commit `0559576`).
- Panic real que motiva `block-en-async-rs`: `cli/src/tui.rs:281` (turno previo B3).
