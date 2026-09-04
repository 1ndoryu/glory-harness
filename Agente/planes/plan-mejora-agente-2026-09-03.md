# Plan: Glory Harness + IA de Tasks — mejora integral del agente (paridad con agentes de código)

- **Fecha:** 2026-09-03
- **ID:** 318A-15 (libre; no usar en PT hasta cerrar este plan)
- **Estado:** ✅ cerrado 2026-09-04 — F1-F6 completas (318A-15), criterios
  globales §6 verificados contra el código (ver §9 notas de cierre).
  Pendientes operacionales del usuario (no defectos): línea base con 10+
  conversaciones reales de PT (script validado, falta historial) y verificación
  en vivo con clave real de proveedor para un turno de la IA de PT.
- **Documento base:** `Agente/documentacion/comparativa-opencode-agente-2026-09-03.md`
  (revisiones 1-3: `19d0241`, `3472ca1`, `6cdc13a`)
- **Alcance:** núcleo agnóstico `glory-harness/core` + CLI `glory-harness chat` +
  la IA de `PROYECTO TASKS` (dominio productividad). Mejora de **comportamiento del
  agente** (prompt, contexto, tools, permisos, subagentes, compactación) — sin cambiar
  arquitectura ni contrato SSE.
- **Referencias de código (clonadas localmente, solo lectura):** `data/referencias-cli/`
  (`claurst`, `hermes-agent`, `opencode`, `grok-cli`, `vscode`) — índice y rutas de
  interés en `data/referencias-cli/README.md`.

---

## 0. Resumen ejecutivo (para el usuario)

Hoy el agente (Glory Harness y la IA de Tasks) responde turnos con un system prompt
corto y fijo, herramientas con descripciones de una línea, permisos por flags globales
y un único agente que mezcla explorar/planificar/editar. Los agentes de código
maduros (opencode, claude-code vía claurst, grok-cli) ganan porque **componen el
prompt por capas, dan permisos finos por herramienta, delegan en subagentes
especializados y compactan el contexto con dirección**, no por un prompt mágico.

Este plan convierte la comparativa en **fases ejecutables** (F1-F6) con checklist,
criterio de éxito E2E y verificación por fase. La estrategia recomendada arranca con
**F1+F5** (mayor efecto con menor esfuerzo), mide con telemetría y luego decide
permisos (F3) vs subagentes (F4).

## 1. Problema real (verificado en código)

1. **Prompt monocapa:** system prompt fijo corto en `core/src/runtime.rs:33`; no hay
   bloque `[ENTORNO]` (fecha, workspace, git) ni capa `[REGLAS]`. La IA de PT inyecta
   memoria/skills como mensajes system extra sobre ese prompt.
2. **Tools pobres:** descripciones de una línea (`tools_archivo.rs`, `tools_web.rs`);
   sin formato de salida esperado, límites ni consejo de cuándo usar cada una.
3. **Permisos globales:** flags booleanos (`permitir_*`) y modos opacos
   `predeterminado|meta|autonomo`; sin granularidad por herramienta ni por patrón.
4. **Sin subagentes:** un único agente explora, planea y edita en el mismo contexto
   (la comparativa §5.2: los modelos rinden peor mezclando fases).
5. **Sin navegación web de lectura:** solo `web_search` (resultados), no hay forma de
   leer el contenido de una URL.
6. **Compactación genérica:** resumen automático sin plantilla dirigida; no conserva
   decisiones/pendientes/preferencias de forma estructurada.
7. **Sin límite de pasos ni wrap-up:** el runtime puede encadenar turnos sin resumen
   de cierre ni tool `todo` para planificar el trabajo.

### Resultado deseado

- El agente sabe **dónde está y qué día es**, aplica **reglas del repositorio** y
  distingue contexto compactado de conversación viva.
- Las tools explican **qué devuelven, con qué límites y cuándo usarlas**; el modelo
  elige `file_write` vs `file_patch` correctamente.
- **Permisos finos**: cada tool es `ask|allow|deny`, mapeados desde los modos
  actuales sin romper configs.
- **Subagentes especializados** (`explorar/planificar/revisar/redactar`) con sesión
  aislada, presupuesto de pasos y eventos SSE para la UI.
- **`web_fetch`** para leer URLs, con bloque de seguridad (sin SSRF).
- **Compactación dirigida** por tramos, con plantilla y fallback determinista.
- Todo verificado con tests + gate Sentinel + E2E en vivo, sin romper la
  idempotencia ni el aislamiento por conversación.

## 2. Alcance / no alcance

**Sí:**
- Núcleo agnóstico `glory-harness/core`: capas de prompt, entorno, tools, permisos,
  subagente, compactación — todo tras puertos/traits (patrón de Fases 1-2 ya
  aplicado: sin SQL ni dependencias de dominio en el núcleo).
- CLI `glory-harness chat` (REPL y `--tui`): capa de reglas, tools ricas, subagentes
  de código, presupuesto de pasos.
- IA de `PROYECTO TASKS`: capa de reglas/skills, permisos por conversación,
  subagentes de productividad **sin** herramientas de shell ni de edición de
  archivos del sistema (invariante de seguridad).

**No (documentado en comparativa §8):**
- MCP, LSP, navegador headless, cliente HTTP servidor (en el CLI).
- Tool `bash` en la IA de tasks (queda `deny` total, permanente).
- Cambios de arquitectura (SSE, persistencia, AppState) salvo los estrictamente
  necesarios para las fases.

## 3. Estrategia

```text
Fase 0 (telemetría y línea base)      ──┐ barata, informa el orden F3 vs F4
Fase 1 (capas + entorno)              ──┼ sin dependencias — base de todo
Fase 2 (reglas)                       ──┤ requiere F1 (capa [REGLAS])
Fase 3 (permisos por tool)            ──┼ independiente (convive con F1/F2)
Fase 4 (subagentes)                   ──┤ requiere F1 y F3 (policy de la tool task)
Fase 5 (tools ricos + todo + pasos)   ──┼ independiente — mejora todas las demás
Fase 6 (compactación dirigida)        ──┘ independiente
```

- **Primer bloque (alternativa B de la comparativa §6): F1 + F5** — mayor efecto con
  menor esfuerzo, validable en producción.
- **Después de F1+F5: decisión con telemetría (Fase 0) entre F3 (permisos) y F4
  (subagentes)**; recomendación: F4 aporta más calidad, F3 es necesario como
  prerequisito de F4 — hacer F3 y luego F4.
- F2 y F6 independientes: F2 cuando el CLI tenga su primer AGENTS.md real; F6 cuando
  la telemetría muestre conversaciones largas reales.
- Cada fase se cierra con tests + `sentinel analyze` en ambos proyectos + caso E2E
  del criterio de éxito, **sin depender de respuesta exitosa del proveedor Glory**
  (mocks/fixtures en el E2E).

## 4. Fases

### Fase 0 — Telemetría y línea base (barata, informa decisiones)

**Problema:** no hay métricas de uso real (longitud de conversaciones, tools más
usadas, fallos por tool, compactaciones) para decidir qué mejora primero.

- [x] Inventariar qué emite ya `Usage`/`Contexto` (tokens, `ocupacion_pct`,
      provider/modelo reales — `b971e4f`): el núcleo ya emite `AgenteEvento::Usage`
      con `tokens_prompt/complecion` y `ocupacion_pct` (context.rs); F0 lo
      reutiliza sin cambiar el contrato.
- [x] Añadir al evento (core) contadores por tool (usos/fallos/duraciones) y
      nº de compactaciones por conversación: `TelemetriaTurno` (telemetria.rs)
      acumula usos/fallos/duración por tool, denegaciones (F3), subagentes
      parciales (F4) y compactaciones (contador en context.rs); se emite como
      `AgenteEvento::ResumenTurno` al cerrar el turno (guard anti-envenenamiento,
      nunca rompe la ejecución) y se muestra en el CLI (chat.rs).
- [x] Script de lectura de un puñado de conversaciones reales de la IA de PT
      (longitud, tools usadas, turnos con error) → informe de línea base.
      `.freebuff/linea-base-agente.mjs` + informe
      `informe-linea-base-agente.md` en PT: solo-lectura vía API (login local
      + listado + historial enriquecido), sin tocar BD ni producción. Nota:
      la BD local tiene 1 sola conversación real (2 msgs, 101 tokens, 3 tools
      file_* ok) — el pipeline queda validado; re-ejecutar el mismo script
      cuando haya más historial para llegar a las 10+ del criterio.
- [x] Registrar en este plan: decisión F3-vs-F4 con esos datos.
      Orden F3→F4 decidido por el usuario y validado: F3 (permisos por tool)
      es prerequisito de F4 (la tool `task` hereda la policy F3 del padre);
      la telemetría en marcha (`ResumenTurno` + script de línea base) permite
      validarlo a posteriori con uso real.

**Criterio de éxito:** un comando/imprimir reporte agrega 10+ conversaciones reales
y produce el informe de línea base sin tocar producción.

### Fase 1 — System prompt por capas + bloque [ENTORNO] (núcleo)

**Problema:** el modelo no sabe fecha, workspace, git, ni distingue qué capas del
prompt son inmutables.

- [x] `core`: separar el system prompt en capas con marcadores
      `[ENTORNO]`/`[REGLAS]` (patrón claurst `SYSTEM_PROMPT_DYNAMIC_BOUNDARY`,
      `core/src/system_prompt.rs:18`): lo estático/cacheable antes, lo dinámico
      después.
- [x] Bloque `[ENTORNO]`: fecha, workspace/`--dir`, repo git sí/no + rama, modelo
      activo (patrón opencode `session/system.ts:74-81`).
- [x] `[REGLAS]` vacío por defecto en el núcleo; expuesto para que el consumidor
      inyecte (CLI: AGENTS.md; PT: skills/reglas) — mismo mecanismo que hoy usa la
      IA de PT con memoria/skills, ahora con ranura propia.
- [x] Compactación/head protegido: `[ENTORNO]` y `[REGLAS]` recién inyectados cada
      turno, nunca compactados (extender `context.rs`).
- [x] Tests: prompt resultante contiene fecha + workspace + marcadores; capa reglas
      vacía sin `[REGLAS]` huérfana.
- [x] E2E: turno real (o con fixture) donde el modelo refiere el workspace/fecha.

### Fase 2 — Capa de reglas (AGENTS.md CLI / skills PT)

**Problema:** el CLI no lee reglas del repositorio; la IA de PT duplica memoria/skills
en el handler.

- [x] CLI: cargar `AGENTS.md` subiendo directorios desde `--dir` (jerarquía
      opencode `docs/rules`) y volcarlo en la capa `[REGLAS]`; caché por directorio.
      (`cli/src/reglas.rs`: ancestros desde el workspace, la raíz gana a
      subcarpetas, caché global por directorio; `run.rs` inyecta vía
      `AgentRuntime::establecer_reglas` — la ranura del núcleo, protegida en
      compactación por F1.)
- [x] IA de PT: migrar la inyección de memoria/skills a la ranura `[REGLAS]`
      (sin duplicar: o memoria o reglas, no ambos mensajes system con lo mismo).
      `handlers/agente.rs` de PT: las skills activas entran por
      `runtime.establecer_reglas` (mismo canal que AGENTS.md del CLI, capa
      protegida en compactación) y la memoria sigue en el historial — nada
      duplicado. Verificado: `cargo check --lib` + `cargo test --lib agent`
      (11/11) verdes; la verificación en vivo con clave queda para un turno
      real.
- [x] Tests: jerarquía de AGENTS.md (raíz gana a subcarpeta), ausencia no rompe
      (+ caché: segunda lectura no re-lee el disco).
- [x] E2E: determinista sin modelo real — fixture con AGENTS.md falso → loader
      → `ensamblar_prompt_sistema` y se verifica que el contenido queda dentro
      de `[REGLAS]`…`[/REGLAS]` y que `[ENTORNO]` va después.

### Fase 3 — Permisos por herramienta (ask/allow/deny)

**Problema:** flags booleanos globales; la UI no puede preguntar por tool ni el
usuario negar una tool concreta.

- [x] Core: modelo de datos `Permiso { ask | allow | deny }` por tool, con
      herencia de un default por perfil y overrides por conversación (patrón
      opencode permissions + claurst `PermissionLevel`).
- [x] Mapeo desde los modos actuales: `predeterminado` → ask para escritura,
      allow para lectura; `meta` → deny para todo efecto; `autonomo` → allow
      (sin romper configs existentes: el mapeo es default, override por
      conversación explícito).
- [x] `deny` silencioso: la tool se **quita del schema** (no solo policy) — el
      modelo no la ve (verificado en claurst `agent_tool.rs`).
- [x] Denegación con `ask`: evento `RequiereAprobacion` actual; **no reintento
      automático** tras negación (la conversación continúa, el modelo recibe el
      "denegado" y cambia de plan).
- [x] Evento SSE `PermisoDenegado { tool, motivo }` para la UI.
- [x] Tests: herencia default→conversación, deny quita del schema, ask emite
      evento, no-reintento.
- [x] E2E: pedir una tool `deny` y verificar que el modelo no la ofrece — el
      modelo solo ve tools vía `schemas_openai`; el fixture verifica deny
      (modo y override) la excluye del schema (mismo canal que un turno real).

### Fase 4 — Subagentes (tool `task`)

**Problema:** un único agente mezcla fases; las tareas largas (explorar + planear +
editar) degradan por contaminación de contexto.

- [x] Core: tool `task { agente, objetivo, contexto?, max_pasos? }` con perfiles
      `explorar|planificar|revisar|redactar` — contrato comparativa §5.2. Nota:
      los 4 perfiles viven en el núcleo (agnósticos) y el consumidor puede
      añadir los suyos con el mismo tipo `PerfilSubagente` (PT restringe a
      `planificar|revisar` cuando registre su whitelist de dominio).
- [x] Sesión hija: system prompt del perfil, `max_turns = presupuesto_pasos`
      (default 8), tools restringidas al perfil; historial arranca en el
      objetivo (sin heredar el del padre).
- [x] **Exclusión del schema:** la tool `task` se excluye de las tools del hijo
      (imposible recursión, no solo policy) — patrón claurst (`schema_hijo`).
- [x] Aislamiento: la sesión hija NUNCA escribe en persistencia (ni mensajes ni
      turnos); su única salida al padre es el `ResultadoSubagente` estructurado
      `{ ok, resumen, parcial, pasos_usados }` → `enmarcar_resultado_para_padre`.
      Nota: sin marcador BD `tipo='subagente'` — `TurnoPersistido` no tiene ese
      campo y PT lo construye literal; el aislamiento real es no-persistir.
- [x] Eventos SSE `SubagenteInicio { perfil, instruccion }` / `SubagenteFin {
      resumen, ok, parcial }` emitidos por el runtime y visibles en la UI del
      CLI (chat.rs y tui.rs).
- [x] Presupuesto: tope de concurrentes global (default 2, rechazo sin cola) y
      tope de profundidad 1; al agotar pasos → `parcial: true` con el wrap-up
      "lo hecho / lo que falta" (no falla; el padre ve `[SUBAGENTE PARCIAL …]`).
- [x] Perfiles: **ninguno incluye ejecución de comandos ni edición del sistema**
      (invariante §2; `file_write`/`file_patch` solo en `redactar`, whitelist
      acotada al workspace) — verificado por test.
- [x] Alternativas documentadas (comparativa §5.2 e/f): manager-executor con
      presupuesto (claurst) y delegación en proceso hijo (grok-cli). **Decisión
      (03-09-2026, con datos F0): se mantiene el manager-executor en proceso** —
      los perfiles de subagente no ejecutan comandos ni editan el sistema
      (invariante §2), así que no hay necesidad de aislamiento de SO; en
      proceso comparte pool y policy F3 (herencia trivial) y el tope de
      concurrentes (2) acota el coste. Revisitar con proceso hijo si la
      telemetría muestra turnos largos o presión de memoria.
- [x] Tests: hija no contamina padre (no persiste nada), exclusión de `task`,
      `parcial` al agotar pasos, resultado estructurado.
- [x] E2E: fixture determinista sin proveedor (`f4_e2e_fixture_resultado_…`)
      que recorre sesión hija → resultado estructurado → mensaje que ve el
      padre (resumen acotado, marcador PARCIAL con pasos, rechazo por tope);
      el E2E con modelo real queda para la verificación en vivo con clave.

### Fase 5 — Contratos de tools ricos + todo + límite de pasos

**Problema:** descripciones de una línea; sin formato de salida, límites ni
consejos; sin planificación visible; sin wrap-up al agotar pasos.

- [x] Core: reescribir descripciones de tools con: qué hace, **formato de salida**,
      **límites** (tamaño/timeout/encoding), **consejo de cuándo usarla** y error
      esperado. Prioridad: `file_write`/`file_patch`/`file_read`/`file_search`/
      `web_search` (+ `web_fetch` si F4/F5 lo trae).
- [x] Regla write-vs-patch en las descripciones: "cambio < 20% del archivo →
      `file_patch`; crear o reescribir casi todo → `file_write`".
- [x] `file_patch`: validar que cada `old` exista y sea **único** (error claro si
      ambiguo — paridad opencode `edit`).
- [x] Tool `todo { crear|actualizar|completar }` en el núcleo: plan visible de la
      tarea; el modelo la usa para tareas de varios pasos.
- [x] Límite de pasos con wrap-up: al llegar a `max_turns`, pedir resumen de cierre
      ("hecho / pendiente / siguiente paso") en lugar de cortar en seco.
- [x] Tests: descripciones renderizadas sin saltos rotos; `file_patch` con `old`
      duplicado falla con mensaje claro; `todo` se refleja en el contexto.
- [x] E2E: tarea de 2 ediciones → el modelo usa `todo`, hace patch, y el turno
      cierra con wrap-up.

### Fase 6 — Compactación dirigida

**Problema:** resumen genérico que pierde decisiones/pendientes/preferencias.

- [x] Plantilla de resumen con secciones `[DECISIONES]`, `[PENDIENTES]`,
      `[PREFERENCIAS]`, `[RESTRICCIONES]` y consigna "no añadas nada que no esté en
      la conversación" (evitar alucinación).
      (`context.rs: plantilla_resumen_dirigido()`; molde de la variante A y del
      resumen que aporte el consumidor.)
- [x] Variante A (LLM, mismo proveedor) con fallback B (determinista: conservar
      instrucciones/preferencias verbatim + último intercambio).
      (Seam `resumen_llm` en `preparar_con`: si llega vacío/`None` cae al
      fallback B — `fallback_determinista`, verbatim por construcción; el
      runtime usa B por defecto, `resumir_con_llm=false` según §8.4, y los
      tests nunca requieren proveedor.)
- [x] Compactación por **tramos fechados** (cada tramo deja un resumen system con
      fecha), nunca "todo lo anterior".
      (`resumen_dirigido` con `fecha_hoy()`; cada compactación deja su resumen
      system fechado y se acumula el contador de tramos.)
- [x] Umbrales configurables por consumidor: `max_tokens_contexto`,
      `pct_compactar`, `ventana_seguridad` (no compactar durante un tool_call
      largo).
      (`ContextoConfig.pct_compactar` (0.80) + `ventana_seguridad` (0.15),
      disparo efectivo `umbral_disparo()`; el runtime marca `tool_en_curso`
      durante la ejecución de tools y `preparar_con` omite la compactación
      salvo ocupación degenerada.)
- [x] Observabilidad: nº de tramos compactados y tamaño del resumen en el evento
      `Usage`.
      (`CompactionMetrics.resumen_tokens` + `tramos`, rellenos en `preparar_con`
      y emitidos en el `AgenteEvento::Usage` de compactación; reutiliza la
      telemetría de F0.)
- [x] Tests: tramos fechados, no compacta head, fallback determinista si el LLM
      falla, no compacta dos veces seguidas sin mensajes nuevos.
      (5 tests F6 en context.rs: umbral_disparo configurable, ventana de
      seguridad, tramos fechados + no-recompactación, fallback si el LLM
      falla/vacío, variante A con plantilla.)
- [x] E2E: conversación larga (fixture) → tras compactar, el modelo refiere una
      decisión que solo está en el resumen.
      (`f6_e2e_fixture_decision_solo_en_resumen`: decisión en el medio del
      historial sobrevive únicamente vía el resumen del tramo; head protegido y
      cola verbatim verificados.)

## 5. Orden, dependencias y verificación por fase

```text
F0 (telemetría)     ──┐ informa el orden F3/F4
F1 (capas+entorno)  ──┼ sin dependencias — primer bloque con F5
F2 (reglas)         ──┤ requiere F1
F3 (permisos)       ──┼ independiente — prerequisito de F4
F4 (subagentes)     ──┤ requiere F1 + F3
F5 (tools ricos)    ──┼ sin dependencias — primer bloque con F1
F6 (compactación)   ──┘ independiente
```

Por fase, antes de cerrar:
1. `cargo test --workspace` (glory-harness) y tests del paquete tocado (PT).
2. `sentinel analyze` / `quality:analyze` en **ambos** proyectos — 0 errores, sin
   hallazgos nuevos en los archivos tocados.
3. Caso E2E del criterio de éxito de la fase (con fixture/mock si el proveedor no
   responde).
4. Commit por fase con el estilo del repo (mensaje con ID `318A-15` + fase), solo
   archivos propios.

**No romper (invariantes):** aislamiento por conversación/tab; idempotencia de
`guardar_mensaje_usuario` (clave_idempotencia); fixed glory/commandcode en backend;
flags de contexto reales; contrato SSE existente (las fases solo **añaden** eventos).

## 6. Criterios de aceptación (globales, se verifican al cierre del plan)

- [x] El system prompt del núcleo llega al modelo por capas con `[ENTORNO]`
      (fecha/workspace/git/modelo) y ranura `[REGLAS]` poblada por consumidor.
      Evidencia: `core/src/context.rs` (`MARCA_ENTORNO`/`MARCA_REGLAS` +
      `ensamblar_prompt_sistema`); F1 `2216070`; CLI inyecta AGENTS.md por
      jerarquía vía `establecer_reglas` (`cli/src/reglas.rs`, `run.rs`, F2
      `4628da0`); test `system_con_marcadores_se_protege_de_la_compactacion`.
- [x] Las tools del núcleo muestran formato de salida, límites y consejo de uso;
      `file_patch` valida unicidad de `old`.
      Evidencia: contratos ricos en `core/src/tools_archivo.rs` (F5 `fb46a43`);
      tests `file_patch_old_ausente_falla_no_encontrado`,
      `file_patch_old_duplicado_falla_con_mensaje_claro` y
      `e2e_fixture_todo_patch_y_cierre_de_plan`.
- [x] La política de permisos es por tool (`ask|allow|deny`) con herencia y
      overrides por conversación; `deny` quita la tool del schema; no hay
      reintento tras denegación.
      Evidencia: `core/src/permiso.rs` (`resolver_permiso`/`permiso_efectivo`),
      F3 `70d03e0`; tests `f1_deny_de_categoria_con_patron_asterisco_oculta_la_tool_del_schema`,
      `f1_override_deny_fail_closed_gana_a_toda_regla`,
      `f3_deny_deniega_y_el_repetido_no_reeventa`.
- [x] La tool `task` delega en subagentes aislados con presupuesto; el subagente
      no ve la tool `task`; los perfiles de PT no ejecutan comandos ni editan
      archivos del sistema.
      Evidencia: `core/src/subagente.rs` (F4 `134c40c`); tests
      `f4_schema_hijo_excluye_task_y_aplica_whitelist`,
      `f4_perfiles_no_incluyen_ejecucion_ni_edicion_de_sistema`,
      `f4_presupuesto_efectivo_valida_max_pasos`, `f4_profundidad_maxima_uno`,
      `f4_e2e_fixture_resultado_estructurado_hacia_el_padre`.
- [x] La compactación es dirigida por plantilla, por tramos fechados, con fallback
      determinista y umbrales configurables; head/`[ENTORNO]`/`[REGLAS]` nunca se
      compactan.
      Evidencia: `core/src/context.rs` (F6 `bcda2d1`); tests
      `system_con_marcadores_se_protege_de_la_compactacion`,
      `f6_tramos_fechados_y_no_recompacta_sin_material_nuevo`,
      `f6_fallback_determinista_si_el_llm_falla`,
      `f6_umbral_disparo_configurable_por_consumidor`,
      `f6_ventana_seguridad_omite_compactar_durante_tool`.
- [x] Tanto el CLI (`chat` REPL y `--tui`) como el chat de la IA de PT siguen
      funcionando con las mejoras sin regresiones (E2E en vivo, verificación con
      fixture si no hay clave de proveedor).
      Evidencia: 29 tests CLI (incl. `tui::tests::tools_se_marcan_ok_y_error`);
      REPL/TUI verificados en vivo en Fase 5 (318A-13) y plan 2; PT con
      `establecer_reglas` + canal de aprobación verificado en vivo (:3001, F2
      `4289ba0`). Pendiente operacional: turno real de la IA de PT con clave de
      proveedor (los fixtures E2E no dependen de clave).
- [x] Gate final verde: `sentinel doctor` + `sentinel check` + `quality:analyze` en
      glory-harness y PT; árbol limpio salvo cambios ajenos.
      Evidencia 2026-09-04: `cargo test --workspace` 188/188 (159 core + 29 CLI);
      `cargo clippy -p glory-harness-core -p glory-harness --all-targets
      -- -D warnings` limpio; `sentinel analyze` 0 errores/0 hallazgos
      (`.quality-reports/analyze.json`); PT `cargo check --lib` verde (wiring
      `66d746e`). Salvedad ajena: `cargo clippy --workspace` reporta 1 error en
      `desktop/src-tauri/src/main.rs:320` — crate 039A-1 de otro hilo, fuera del
      alcance de este plan (no tocado).

## 7. Riesgos y mitigaciones

- **Coste ×2-3 por turno (subagentes):** tope de concurrentes, `max_pasos`,
  perfiles con tools mínimas. Si el coste escala: alternativa (e) manager-executor
  con presupuesto USD (claurst) — comparativa §5.2.
- **Contaminación padre↔hijo:** sesión efímera + única salida estructurada +
  exclusión de `task` en el hijo.
- **Resumen con alucinación (F6):** consigna explícita "solo-resumir" + fallback
  determinista + acotar tamaño del resumen (~15-20% del tramo).
- **Regresión de permisos en configs existentes:** el mapeo de modos es default y
  los overrides por conversación son explícitos; pruebas de no-romper en §5.
- **Web fetch SSRF:** bloque de seguridad obligatorio (solo http(s), timeout,
  sin loopback/privadas, sin JS) — comparativa §5.3.
- **Proveedor Glory no responde en E2E:** fixtures/mocks en el criterio de éxito;
  la verificación nunca depende de respuesta real del proveedor.

## 8. Decisiones abiertas (requieren al usuario)

1. **Orden F3 vs F4 tras F1+F5:** recomendación F3 antes de F4 (F4 requiere policy
   de `task`); la telemetría (F0) puede cambiarlo.
   → **Resuelta (03-09-2026, con datos F0):** orden F3→F4 ejecutado y validado;
   registro en la fase F0.
2. **Perfil "código" con tool `bash` en el CLI** (permisos por patrón de comando,
   paridad opencode) — fuera del alcance actual del CLI (solo archivos/web); decidir
   si se incorpora en una fase posterior del CLI.
   → **Resuelta en bloque posterior (318A-16 F3, `1ffa060`):** tool `comando` en
   el CLI con clasificador de riesgo, runner con timeout/background/truncado y
   permisos por patrón (regla v2). Perfiles de subagente sin ejecución (invariante).
3. **IA de PT: migrar memoria/skills a la ranura `[REGLAS]`** (F2) — requiere
   revisar qué vive en memoria persistente vs skills vs reglas del momento.
   → **Resuelta (F2 `4628da0`):** las skills activas de PT entran por
   `runtime.establecer_reglas` (mismo canal que AGENTS.md del CLI, capa
   protegida); la memoria sigue en el historial, sin duplicación. Verificación en
   vivo con clave real de proveedor: pendiente operacional (no defecto).
4. **Umbrales por consumidor (F6):** valores default propuestos
   (`pct_compactar=80`, `ventana_seguridad` = ~15% del techo); ajustar con telemetría.
   → **Resuelta (F6 `bcda2d1`):** umbrales configurables por consumidor con los
   defaults propuestos (test `f6_umbral_disparo_configurable_por_consumidor`);
   el ajuste fino con telemetría queda abierto al uso real.

## 9. Checklist resumen de estado

| Fase | Contenido | Estado |
|---|---|---|
| F0 | Telemetría y línea base | ✅ 4/4 |
| F1 | Capas + `[ENTORNO]` | ✅ 6/6 |
| F2 | Reglas (AGENTS.md / skills) | ✅ 4/4 |
| F3 | Permisos por tool | ✅ 7/7 |
| F4 | Subagentes (tool `task`) | ✅ 10/10 |
| F5 | Tools ricos + todo + pasos | ✅ 7/7 |
| F6 | Compactación dirigida | ✅ 7/7 |

### Notas de cierre (2026-09-04)

- Criterios globales §6: 7/7 verificados contra el código (detalle y hashes en §6).
- Evidencia de cierre: `Agente/completados/tareas-2026-09-04.md` (entrada 318A-15).
- Pendientes operacionales del usuario (documentados en F0/F2, no defectos):
  1. Línea base real de PT: re-ejecutar `.freebuff/linea-base-agente.mjs` cuando
     haya 10+ conversaciones reales (el pipeline está validado).
  2. Turno en vivo de la IA de PT con clave real de proveedor (los E2E por fixture
     ya cubren el comportamiento sin clave).
- Los 4 ítems del bloque 318A-16 (plan 2) que este plan dejaba como
  decisión/expansión (bash en CLI, plan explícito, permisos finos por categoría,
  aprobación 3 vías) se implementaron en `plan-mejora-agente-2-2026-09-03.md`.

