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
- **Estado:** S0 ✔ (prerequisito documentado, 05-09) · S1–S8 pendientes de ejecutar.

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

- [ ] **Cargo.lock/manifests y `directorio-abarrotado` en la raíz:** evaluar si la regla
      debe ignorar `*.lock`, `Cargo.toml` y archivos de config de raíz (workspace) o si
      la excepción correcta es patrón de `directoryExceptions`. Decisión con caso mínimo.
- [ ] **Regla por defecto a "error" vs "warning"**: las reglas estructurales
      (`limite-lineas-*`, `directorio-abarrotado`) hoy son `warning`; verificar que el
      gate no falle por ellas y que el plan no necesite `sentinel-disable-file` (regla
      R1: solo con justificación).
- [ ] **Doble reporte `limite-lineas` + `-nivel-2`** en el mismo archivo: ¿ruido o
      intencional? Si es intencional, documentar; si no, colapsar en el reporte.
- [ ] **Cobertura vacía del analyzer:** `includePatterns` del `sentinel.config.json`
      analiza solo `**/*.rs` + `Cargo.toml/lock`; la UI (`desktop/ui/**/*.ts`), scripts y
      migraciones SQL quedan fuera aunque existan reglas para TS/React/SQL (p. ej.
      `innerhtml-variable`, SQL sin prepared). Añadir patrones por lenguaje con su perfil
      (ver S7).
- [ ] Cada cambio del gate: tests unitarios en glory-sentinel del caso mínimo, publicar
      commit, actualizar `quality-tools.json` (commit), regenerar `sentinel.lock.json`
      con el comando oficial, `quality:doctor` verde, re-analyze.
- [ ] Repinear **solo** cuando S7 confirme qué reglas nuevas entran (mismo bump de commit).

**Criterio de éxito S1:** el re-analyze no reporta los falsos positivos decididos como
tales (con evidencia de la regla/caso), y ningún cambio de gate usa `sentinel-disable-*`.

---

## S2 — Archivos grandes del núcleo/CLI (split estructural, cero cambio de comportamiento)

Regla: cada archivo nuevo < 500 líneas efectivas objetivo (≈<350 ideal); **solo mover
código, nunca cambiar lógica**; los tests existentes pasan sin edición de aserciones
(si un test cambia de archivo, se mueve con el código, sin alterar su contenido).

- [ ] **`core/src/runtime.rs` (1719 → objetivo ~<450/fragmento):** extraer por dominio:
      1) `bucle.rs`/`turno.rs` — el cuerpo de `ejecutar_turno` (380 efectivas) como
         runner del bucle LLM→tools; 2) `subagente_ejecucion.rs` — `ejecutar_subagente`
         y `_desde_llamada` (200); 3) dejar en `runtime.rs` la orquestación fina
         (struct `AgentRuntime`, `nuevo`, puertos, acceso a `registry`) + re-exports
         `pub use` para no romper consumidores (`cli`, `desktop`, PROYECTO TASKS).
         Atención: no romper los comentarios-memoria ni los `[318A-xx]`/`[Bloque 3]`.
- [ ] **`core/src/llm.rs` (1592):** separar por responsabilidad sin tocar `enviar_chat*`:
      1) tipos del contrato (`AiMessage`, `AiToolCall`, `AiChatResult`, `Keys/…` ya
         exportados: mover a `modelo.rs` con re-export); 2) red/HTTP y reintentos
         (`ejecutar_request*`, `ejecutar_request_stream`, parseo de tool_calls) →
         `red.rs`; 3) `LlmProviderService` (fallos, rotación, nutrición) queda en `llm.rs`
         como fachada fina.
- [ ] **`cli/src/tui.rs` (1305):** separar presentación de control: 1) render puro
      (`a_lineas`, `dibujar`, `envolver_*`, `render_markdown_linea`, tipos `Line`/estado
      visual) → `tui_render.rs`; 2) `spawn_worker` (167) y `bucle_ui` (120) → `tui_bucle.rs`
      o helpers por responsabilidad; `tui.rs` conserva el montaje.
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

- [ ] Refactor **`core/src/diff.rs`** `diff_lineas` (135): separar cálculo de línea vs
      ensamblado del diff textual; caso de prueba por cada modo (crear/modificar/borrar).
- [ ] Refactor **`cli/src/main.rs`** `cmd_schedule_impl` (123): extraer parseo de
      argumentos, validación y ejecución en 2–3 helpers con tipos claros.
- [ ] Refactor **`cli/src/chat.rs`** `chat()` (105): extraer el bucle REPL y el manejo de
      comandos `/…` a helpers (aprovechar S4 de skills si ya separó comandos).
- [ ] Con S2 hecho, verificar que **ninguna función del núcleo/CLI** supere 100 efectivas;
      las que queden se extraen aquí con nombre de helper por responsabilidad.
- [ ] Regla de oro: cada extracción conserva el flujo exacto (sin cambios de semántica);
      si una extracción "natural" exige tocar lógica, se marca en el plan como decisión
      (no se mezcla con el movimiento).

**Criterio de éxito S3:** `funcion-larga-rs` en 0 para core+cli en el re-analyze;
tests + clippy verdes.

---

## S4 — Directorios abarrotados (organización por dominio)

- [ ] **`core/src/` (28 archivos planos)** → estructura por dominio con re-export en
      `lib.rs` (los consumidores importan `glory_harness_core::…` y **no deben cambiar**):
      1) `nucleo/` (o `agente/`): `runtime`, `context`, `contexto`, `turno`, `subagente`;
      2) `herramientas/` (tools): `tool`, `tools_archivo`, `tools_web`, `comando`,
         `mcp`, `skill`, `todo`, `tareas`, `scheduler`;
      3) `politica/` (permisos): `permiso`, `regla`, `aprobacion`, `bash_clasificar`;
      4) `puertos/contrato`: `ports`, `error`, `evento`, `modelo` (de S2), `sandbox`,
         `plan`, `guardas`, `pregunta`, `telemetria`, `diff`, `llm`.
      Mover archivos con `git mv` (historia preservada) y ajustar solo `use crate::…`
      internos + `lib.rs` (`mod`/`pub use`). Ojo: `#[path]`/rutas relativas de tests y
      del E2E del CLI (buscar referencias directas `core/src/` en scripts).
- [ ] **`cli/src/` (12 archivos)** → igual, en 2–3 subdirectorios (p. ej. `comandos/`,
      `ui/` para chat/tui, `infra/` para persistencia/reglas/ejecutor) manteniendo
      `cli::…` re-exportado. NO mover `persistencia_sqlite.rs` (ajeno 039A-3) hasta su cierre.
- [ ] **Raíz glory-harness (11 archivos):** si S1 decide excepción de config/manifests,
      aplicarla aquí; si decide reorganización, mover solo lo movible sin romper
      `package.json`/`quality-tools.json`/rutas de scripts.
- [ ] Después de cada reubicación: `cargo test --workspace` + verificación de que
      PROYECTO TASKS (`cargo check`, report-only) sigue compilando contra el core.

**Criterio de éxito S4:** `directorio-abarrotado` 0 en core+cli (o excepción documentada
en S1); sin cambios de comportamiento; árbol con `git mv` verificado en el log.

---

## S5 — Auditoría SOLID/arquitectura (con hallazgos reales, no cosmética)

Objetivo: revisar cada servicio del núcleo/CLI contra SRP/OCP/ISP/DIP y dejar un
informe con **hallazgos + evidencia (ruta/línea)** y, cuando aplique, correcciones ya
agendadas en S2–S4 (muchos hallazgos SOLID coinciden con los splits).

Checklist por pieza (marcar `[x]` con hallazgo/acción al auditar):

- [ ] **`AgentRuntime` (SRP):** ¿sigue siendo dios-módulo tras S2? Listar
      responsabilidades restantes (orquestar turno, permisos, plan, guardas, hooks,
      telemetría, reglas, subagentes). Cada una con su tipo/archivo; las que queden en el
      struct deben ser solo **coordinación** y delegación.
- [ ] **`LlmProviderService` (SRP/OCP):** rotación de fallos + nutrición + stream + parseo
      en una clase (mitigado en S2.2). ¿Añadir un proveedor nuevo requiere tocar el
      servicio o hay registro/estrategia (OCP)? Evidencia en `keys_para`/`proveedor_abierto`.
- [ ] **`AgentToolRegistry` (ISP/OCP):** el trait `AgentTool` y el registro: ¿el runtime
      conoce detalles de cada tool o solo el contrato? ¿`ejecutar_tool` hace downcast de
      dominio dentro del núcleo (acoplamiento) o vía puerto? Revisar y documentar.
- [ ] **`tool.rs` 1000+ líneas:** SRP del archivo (definición de trait + registry +
      tools de red) — planificar split adicional si S2 no lo cubre.
- [ ] **`tui.rs`/`UiEstado`:** separación UI-estado vs lógica de eventos vs render
      (S2.3/S3); verificar que `UiEstado` no conozca SSE ni persistencia.
- [ ] **Puertos (`ports.rs`, `PuertosHarness`):** ISP: ¿cada consumidor recibe solo lo que
      usa o el struct grande con `Option`? Evaluar traits por rol (persistencia, web,
      proveedor) y qué rompería dividirlos (dependencias concretas de PROYECTO TASKS).
- [ ] **DIP en llm:** ¿el servicio HTTP depende de reqwest concreto o de un trait
      inyectable (tests actuales usan stub de qué nivel)? Documentar límite de testabilidad.
- [ ] **Errores:** ¿`Error::Validacion(String)` pierde contexto/tipos? ¿hay `unwrap`/
      `expect` en producción fuera de test (regla del gate) o mutex locks con
      `unwrap_or_else(|p| p.into_inner())` generalizados (ver S7)?
- [ ] **Persistencia (cliente CLI):** `AgentPersistence` como puerto: verificar que el
      CLI no haga SQL propio y que `persistencia_sqlite` no filtre al núcleo.

**Criterio de éxito S5:** informe en `Agente/documentacion/auditoria-solid-2026-09-05.md`
con hallazgos por ítem (evidencia ruta:línea), cada hallazgo marcado como
[YA corregido en S#] / [corregir aquí] / [decisión — no corregir, con razón]. Cero
cambios de comportamiento fuera de S2–S4.

---

## S6 — Auditoría de rendimiento (medible, sin optimización prematura)

Objetivo: detectar cuellos de botella reales con instrumentación corta y fijar
objetivos verificables; corregir solo lo demostrado.

Checklist (marcar con evidencia de medición):

- [ ] **Copia de contexto por turno:** `preparar_con(&mensajes…)` y `mensajes = mensajes_prep`
      — ¿cuántas copias de `Vec<AiMessage>` por llamada LLM? Medir con conteo simple o
      `tracing`; si >1 copia completa por turno, evaluar clonar solo delta/índices.
- [ ] **`registry.ids()` por iteración del bucle** (se recalcula por llamada LLM):
      medir frecuencia; cachear por turno si aplica.
- [ ] **Persistencia SQLite:** historial cargado por turno (¿N+1 mensaje a mensaje?);
      escrituras de turno/mensajes: ¿una transacción por batch o N commits? (WAL ya
      activo). Verificar en `cli/src/persistencia_sqlite.rs` (lectura; cambios coordinados
      con 039A-3).
- [ ] **Streaming SSE:** ¿un evento por token y por tool? Volumen y serialización por
      evento (medir tamaño JSON por evento `Usage`/`Token`); si hay eventos redundantes
      por turno, coalescer.
- [ ] **Locks en runtime:** `self.contexto.lock().await` (¿retenido durante llamadas de
      red?), `Arc<Mutex>` de plan/reglas/guardas — detectar contención real con un turno
      sintético (fixture) y `tracing` de espera.
- [ ] **Subagente/task:** coste de spawn por delegación vs reutilización; límites ya
      existentes (concurrencia/profundidad) verificados bajo carga de fixture.
- [ ] **TUI:** render completo por evento vs incremental; coste de `a_lineas` en
      conversaciones largas (envolver todo el historial por frame).
- [ ] Fijar **objetivos**: turno sintético pequeño (3 tools) sin red < X ms en debug; RAM
      del release ≤ línea base 40 MB (WS); streaming sin pausas > 200 ms por token
      (medir con fixture SSE local).

**Criterio de éxito S6:** cada ítem cerrado con número (antes/después) y su fix si aplica;
los que resulten "no problema" quedan documentados como verificados (evita regresiones).

---

## S7 — Brechas del gate: cosas que Sentinel no detecta y debería

Objetivo: lista de **reglas nuevas propuestas** para glory-sentinel con caso mínimo,
basadas en defectos reales ya vividos en este repo. Cada propuesta: reglaId, severidad
sugerida, detección esperada, falso-positivo posible, caso de test.

- [ ] **`block-en-async-rs`** (error): `block_on`/`futures::executor::block_on`/
      `tokio::runtime::Handle::block_on` dentro de código async. **Evidencia real:**
      panic "Cannot block the current thread from within a runtime" en `cli/src/tui.rs:281`
      (Bloque 3, F1 del chat) corregido manualmente; la regla lo habría cazado en el gate.
- [ ] **`lock-a-traves-await-rs`** (warning): `MutexGuard` (std/tokio) viva a través de
      `.await` en la misma función (sospechoso de contención/reención). Heurística por
      rango: guard antes de `.await` sin drop previo.
- [ ] **`expect-produccion-rs`** (error): ampliar `unwrap-produccion-rs` a `expect`
      (auditar hoy cuántos `expect` hay fuera de test en core/cli).
- [ ] **`tool-sin-schema-rs`** o **`registro-sin-nombre`**: tool registrada sin
      descripción/schema (el modelo la ignora) — regla estructural barata sobre el
      registro del núcleo (patrón conocido: declaración vs uso).
- [ ] **SQL no preparado en Rust** (cuando aparezca SQL en consumidores): patrón de
      `format!`/`concat!` dentro de query SQL en crates Rust (hoy no analizado; S1 abre
      los patrones). Solo para crates con SQL (PT lo necesita; GH no tiene SQL por
      contrato — dejar inactiva por defecto en GH).
- [ ] **Cobertura de lenguajes** (con S1.4): habilitar reglas TS/React/SQL existentes en
      `desktop/ui` y scripts (sin análisis nuevo en PHP/WordPress si no hay código).
- [ ] Para cada regla: caso mínimo + caso de no-disparo, tests en glory-sentinel,
      integración con severidad configurable vía `sentinel.config.json`.

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

- **D1 — Falsos positivos:** ¿se corrige en glory-sentinel (repo + bump commit/lock) o se
  acepta excepción documentada vía `directoryExceptions`? Default: **corregir en
  glory-sentinel** lo que sea regla (Cargo.lock/config en `abarrotado`, `nivel-2`
  redundante) y excepción solo para la raíz de config si la regla no distingue.
- **D2 — Reorganización de `core/src` y `cli/src` (S4):** ¿se mueven a subdirectorios por
  dominio ahora (retoque de `use` interno, historia preservada con `git mv`) o se dejan
  planos con excepción? Default: **reorganizar** (S4), es la corrección que la regla pide.
- **D3 — Alcance de la auditoría SOLID/rendimiento:** ¿solo informar con evidencia (y
  corregir lo que S2–S4 ya cubre) o permitir refactors adicionales que surjan (p. ej.
  dividir `tool.rs`)? Default: **informar + corregir solo lo cubierto por S2–S4**; lo
  demás queda como hallazgo con decisión.
- **D4 — Reglas nuevas del gate (S7):** ¿cuáles entran ya (`block-en-async-rs` con
  evidencia real, `expect-produccion-rs` como ampliación barata, `lock-a-traves-await-rs`
  como warning) y cuáles quedan propuestas? Default: **las 3 primeras** en este bloque;
  el resto a un bloque de gate posterior.
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
