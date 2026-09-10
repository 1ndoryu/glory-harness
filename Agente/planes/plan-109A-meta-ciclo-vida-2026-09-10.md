# Plan 109A-5 — Meta con ciclo de vida (tareas visibles + cierre con evidencia)

> ID roadmap: **109A-5** · Fecha: 2026-09-10 · Estado: activo (plan creado 10-09;
> F1 y F2 HECHAS 10-09, F3–F4 pendientes)
> Origen: petición usuario — la representación de meta en Synara es mejor
> (tareas visibles + pie "Goal achieved in 1m 17s"); revisar cuál asegura mejor
> el cumplimiento y planificar las mejoras en GH con Synara como inspiración.
> Conclusión de la revisión (10-09): Synara asegura mejor el *cumplimiento
> declarado* (ciclo de vida + tareas visibles + cierre registrado); GH asegura
> mejor la *no-violación* (deny a nivel de schema, fail-closed). Este plan
> porta lo primero sin perder lo segundo.

## 1. Estado actual (verificado 10-09)

### GH hoy: meta = prefijo condicional + denegación (sin ciclo de vida)

- Texto libre por sesión (`SesionComun.meta: Mutex<Option<String>>`,
  `desktop/src-tauri/src/main.rs:94-95`); `actualizar_meta` normaliza
  (trim, vacío→None) (`workspaces.rs:227-241`); web igual (`cli/src/comandos/web.rs:394`).
- Inyección frágil: `preparar_turno` antepone `[META: …]\n<mensaje>` **solo si
  `modo == "meta"`** (`cli/src/servicio/sesion.rs:386-391`); fuera de ese modo
  la meta guardada se ignora en silencio.
- Garantía negativa fuerte (se conserva): `permiso_por_modo("meta")` = deny a
  todo efecto, tool fuera del schema (`core/src/herramientas/tool.rs:313,845`),
  `PermisoDenegado` sin reintento (`core/src/nucleo/runtime/turno/permisos.rs:120-123`).
- Piezas positivas **desconectadas**: `ToolTodo` (`core/src/herramientas/todo.rs:20-99`,
  store compartida `TodoCompartida`, el resultado devuelve el plan completo al
  contexto) pero efímera (sin BD en v1), solo estados pendiente/completada (sin
  `en_curso`) y **la UI nunca la renderiza** (sin evento ni componente);
  planificador descompone en pasos verificables (`core/src/nucleo/subagente.rs:59`);
  wrap-up exige `HECHO/PENDIENTE/SIGUIENTE PASO` (`core/src/nucleo/runtime/mod.rs:397`).
- Panel meta (`desktop/ui/src/componentes/panelMeta.ts`, 198 líneas): textarea
  + estado/tiempo/tokens + play/pausa; el tiempo mide el **turno en curso**, no
  un reloj de persecución con pausa; visible solo en modo meta o si hay meta.
- Contrato de eventos (`core/src/contrato/evento.rs:31-…`): `PlanPropuesto`,
  `SubagenteFin`, `Telemetria` antes de `Done`. **No existe** evento de logro
  de meta ni de actualización de tareas.

### Synara (inspiración, rutas verificadas)

- Contrato: `ThreadGoalAchievement { goal, achievedAt, elapsedMs, turnId }`
  (`packages/contracts/src/orchestration.ts:709-720`, máx 20); el pie se ancla
  al mensaje terminal del turno que logró la meta.
- Decider `resolveThreadGoalPatch` (`apps/server/src/orchestration/decider.ts:364-427`):
  `goalAchieved` precede a todo (registra logro con tiempo neto de pausas +
  limpia la meta en el mismo evento); meta nueva inicia el reloj; editar
  mantiene reloj+pausa; limpiar resetea; pausar congela (`goalPausedAt`);
  reanudar rebasea `goalStartedAt`.
- Tool `synara_set_thread_goal` (`apps/server/src/agentGateway/Layers/AgentGateway.ts:668-744`):
  capability `thread:write`, `requiresActiveTurn`, ownership
  (`assertCallerMayDriveThread`), `achieved`/`blocked` excluyentes, fallo si no
  hay meta activa; descripción prohíbe inferir metas y fija la regla de bloqueo
  (mismo bloqueador externo 3 turnos seguidos → `blocked: true`; no por
  "difícil o incompleto").
- Tareas visibles normalizadas multi-proveedor (`TodoWrite`/`TaskCreate`/
  `cursor/update_todos` → evento runtime task-list con estados
  pendiente/en_curso/completada + resume entre turnos):
  `claudeTaskTracker.ts`, `CursorAcpExtension.ts:87-92`, `runtimeTaskList.ts`.
- Evidencia visible: header con reloj vivo + pausa (`ComposerGoalHeader.tsx`) y
  pie durable `Goal achieved in ${formatClockDuration(elapsedMs)}` con
  `title=goal`, separado de las acciones con divisor
  (`MessagesTimeline.tsx:2538-2555`).

## 2. Objetivo

La meta en GH pasa de "texto + modo que deniega" a **objeto con ciclo de vida
por conversación**: fijar/pausar/reanudar/lograr con validación fail-closed,
tareas visibles atadas a la meta, reloj de persecución con pausa ajustada y
pie con evidencia ("Meta lograda en 1m 17s"). La garantía negativa actual
(deny + schema) se conserva intacta.

## 3. Alcance / no alcance

- Alcance: ciclo de vida en backend + persistencia por conversación, evento de
  tareas + filas visibles, evento de logro + pie con tiempo, reloj con pausa,
  regla de bloqueo, migración del modo global coordinada con 109A-4 F4.
- No alcance: verificación ground-truth del logro (igual que Synara, el cierre
  es declarado por agente/usuario; se documenta la honestidad, no se vende
  verificación independiente); nuevos comandos `/` más allá de `/meta`
  (dueño: 109A-4); memorias por proyecto (dueño: 109A-2/3); cambios de política
  de permisos (deny intacto).

## 4. Dependencias y coordinación

- 109A-4 F4 (retiro de `meta` del modo global + `/meta <texto>` override):
  F1 de este plan (backend) es independiente y puede ir antes; F3 (pie) y F4
  (bloqueo) tocan `panelMeta.ts`/`entradaTipos.ts`/`opciones.ts` y van
  **después** de F4 de 109A-4 para no pisarse. Registrar el orden en roadmap.
- 109A-1 (gancho pre-compact): sin interacción (logros ≤20 y acotados en
  longitud entran al contexto como texto normal).

## 5. Fases (cada una cierra con verificación propia)

### F1 — Meta como objeto con ciclo de vida (backend, base)

- Estructura por conversación `MetaConversacion { texto, iniciada_en,
  pausada_en, logros: Vec<LogroMeta> }` (`LogroMeta { meta, lograda_en,
  elapsed_ms, turno_id }`, máx 20, texto acotado); función pura
  `resolver_meta(comando, estado, ahora)` (patrón `resolver_permiso`/
  `decidir_permiso`): fijar inicia reloj; editar mantiene reloj+pausa; limpiar
  resetea; lograr registra achievement con elapsed neto de pausas + ancla turno
  + limpia; pausar congela; reanudar rebasea.
- Cablear `actualizar_meta` extendida (fijar/pausar/reanudar/lograr) con
  validación fail-closed (lograr/pausar sin meta activa → error explícito, no
  silencio); persistencia por conversación en SQLite (migración: columnas
  `meta_texto/meta_iniciada_en/meta_pausada_en/meta_logros JSON` en
  conversaciones; legado NULL = sin meta).
- Tests unitarios de `resolver_meta` (los 7 casos del decider Synara) +
  integración persistencia; clippy 0. Sin UI nueva (el panel actual sigue
  leyendo el texto).

**Estado: HECHO (10-09).** Evidencia:

- Dominio en `cli/src/servicio/meta.rs` (única máquina de estados):
  `EstadoMeta`, `MetaActiva`, `LogroMeta`, `resolver_meta` (fijar/editar sin
  reiniciar reloj, limpiar, pausar, reanudar rebaseando, lograr con elapsed
  neto + ancla de turno + limpieza), `elapsed_ms`, límite `MAX_LOGROS_META`
  aplicado al escribir **y** al leer, `MAX_META_CHARS`, `comando_desde_payload`
  (contrato del panel: texto = fijar, vacío/ausente = limpiar; `accion`
  explícita para el resto; combinaciones ambiguas rechazadas) y
  `aplicar_en_borrador` (solo fijar/limpiar sin conversación).
- Persistencia en `cli/src/persistencia_sqlite{.rs,/conversaciones.rs}`:
  migración `meta_texto/meta_iniciada_en/meta_pausada_en/meta_logros` (legado
  NULL = sin meta), lectura por conversación propia y guardado atómico en una
  sola sentencia; `desde_persistida`/`a_persistida` traducen sin tocar dominio.
- Servicio en `cli/src/servicio/sesion.rs`: `meta_leer` (distingue "sin meta"
  de "conversación inexistente", que es error) y `meta_aplicar(&mut self, …)`
  (el `&mut` obliga a retener el mutex de la sesión durante la lectura y la
  escritura: dos transiciones del proceso no pueden intercalarse).
  `preparar_turno` lee la meta **durable de la conversación** antes de escribir
  nada y solo cae al borrador en memoria si esa conversación no tiene meta;
  el prefijo `[META: …]` sigue sujeto a `modo == "meta"` (F4 retira el modo).
- Transportes: `PATCH /api/v1/session/:id/meta` acepta `accion`
  (`fijar|limpiar|pausar|reanudar|lograr`) además del contrato antiguo, aplica
  a la conversación de la sesión y, en borrador, a la memoria; errores con
  código estable (`sin_meta_activa`/`meta_ya_pausada`/`turno_requerido`/…). El
  comando Tauri `actualizar_meta` acepta `accion`/`turno_id`/`conversacion_id`
  y mantiene el borrador cuando no hay conversación (el panel global todavía no
  la identifica: eso es F3).
- Tests: 9 unitarios nuevos en `meta.rs` (payload, borrador, transiciones
  inválidas) + 3 de integración en `sesion.rs` (aislamiento entre
  conversaciones, ciclo completo con logro e historial que sobrevive a reabrir)
  + 3 de handler en `web.rs` (durable, ciclo fail-closed, borrador).
- Verificación: `cargo clippy --all-targets` 0 avisos, `cargo test --workspace
  --lib` 365 verdes (102 CLI + 263 core), `cargo check -p
  glory-harness-desktop --all-targets` OK. Ejecutado como etapa temporal del
  gate (`.quality-reports/tmp-stages-rust.json`, no versionada): el guard
  bloquea `cargo` directo y el gate del proyecto aún no tiene etapa Rust.
- Hallazgo separado (no de este bloque): `scripts/quality/stages.json` y
  `scripts/quality/sentinel-sccache.mjs` están modificados/sin rastrear en el
  árbol (ajenos) y la etapa `sccache` falla con `sccache-no-configurado`, que
  es un hallazgo real del entorno (Rust sin `rustc-wrapper=sccache`); se deja
  como trabajo independiente.

### F2 — Tareas visibles atadas a la meta

- `ItemTodo` gana estado `en_curso` (pendiente/en_curso/completada, paridad
  `normalizeRuntimeTaskStatus`); nuevo evento aditivo `TareasActualizadas
  { items }` tras cada acción `todo` (consumidores viejos lo ignoran);
  instrucción de sistema: con meta activa, descomponer en todos antes de
  actuar; resume entre turnos por conversación (definir semántica de reset al
  cerrar/lograr la meta en F2, no dejarla implícita).
- UI: filas de tareas en el transcript (nuevo `componentes/tareasMeta.ts`,
  ≤300 líneas; iconos Lucide de `iconos.ts`, tokens en `variables.css`, sin
  literales); estados pendiente/en curso/completada con la misma marca
  `[ ]/[/]/[x]` que `a_texto` ya usa.
- Verificación: tests de transiciones + evento; `tsc` EXIT 0 + `vite build`;
  E2E navegador (todos visibles y actualizándose en vivo).

**Estado: HECHO (10-09).** Evidencia:

- Contrato aditivo en `core/src/contrato/evento.rs`: `EstadoTareaVisible`
  (`pendiente|en_curso|completada` en snake_case), `TareaVisible { id, texto,
  estado }` y variante `TareasActualizadas { items }`; los consumidores viejos
  la ignoran (el reenvío SSE de `cli/src/comandos/web_turnos.rs` es genérico
  por `serde_json::to_value(&ev)`, sin cambios).
- Dominio en `core/src/herramientas/todo.rs`: `EstadoTodo::EnCurso` con marca
  `[/]` (`a_texto` mantiene el formato que ya consumían los tests), `en_curso`
  exclusivo (los demás vuelven a `Pendiente`), `visibles()` y `vaciar()`.
- Emisión: `AgentRuntime::emitir_tareas(tx, solo_si_hay)` publica tras cada
  acción `todo` (`turno/permisos.rs`, con la lista completa aunque quede vacía)
  y al abrir turno (`turno/mod.rs`, solo si hay tareas) para dar el resume.
- **Defecto real encontrado y corregido en F2 (alcance del plan):** la lista
  vivía en el `registry` del `AgentRuntime`, que es **por sesión**, no por
  conversación, así que al cambiar de hilo la UI mostraba el plan del anterior
  y el modelo podía seguir editando tareas ajenas. Se añadió
  `PlanesConversacion { activa, listas: HashMap<Uuid, ListaTodo> }` en
  `core/src/nucleo/runtime/mod.rs` con `cargar_plan_de(conversacion_id)`
  (adopta la store si es el primer turno del runtime, sin vaciarla; si no,
  guarda la anterior y extrae la nueva) y `olvidar_tareas(conversacion_id)`.
  Semántica de reset (antes implícita): **al cerrar o lograr la meta** se
  olvida el plan de esa conversación (`sesion.rs::meta_aplicar`, warning si la
  lista está bloqueada); cambiar de conversación no lo borra, lo conserva.
- UI: `desktop/ui/src/componentes/tareasMeta.ts` (94 líneas, estado vivo
  `crearTareasViva()` con cabecera icono `flujo` + contador `hechas/total` con
  `aria-live="polite"` y `<ol class="tareasLista">`; marcas `[ ]/[/]/[x]` con
  los mismos glifos que `a_texto`; iconos Lucide de `iconos.ts`), estilos en
  `estilos/tareasMeta.css` con tokens (sin literales), cableado en
  `tauri/aplicarEventos.ts` (crea el nodo una vez, lo re-ancla al final del
  transcript en cada actualización, hace scroll y se oculta con lista vacía) y
  tipos en `dominio/tipos.ts`/`realTipos.ts`/`turnoReal.ts`.
- Tests: 4 nuevos en `runtime/mod.rs` (`f2_el_plan_no_se_filtra_entre_conversaciones`,
  `f2_volver_a_una_conversacion_restaura_su_plan`,
  `f2_olvidar_tareas_solo_borra_la_conversacion_indicada`,
  `f2_emitir_tareas_publica_la_lista_completa_y_respeta_el_silencio`) + tests de
  serialización del contrato y de transiciones de `en_curso` en `todo.rs`.
- Verificación: `cargo test -p glory-harness-core --lib` **290 verdes** (0
  fallos), `cargo test -p glory-harness --lib` **124 verdes**, `cargo clippy
  -p glory-harness-core -p glory-harness --all-targets -- -D warnings` limpio,
  `npm --prefix desktop/ui run build` EXIT 0 (99 módulos, 159.03 kB JS + 48.63
  kB CSS).
- **E2E navegador real (10-09, `glory-harness web --puerto 8799` + build
  recién compilado, modelo real):** turno que pide descomponer en 3 pasos →
  7 filas `Actualizando tareas` y el bloque "Tareas" visible actualizándose en
  vivo (`0/3` con `[/]` → `2/3` con `[x][x][ ]`), re-anclado tras el último
  mensaje; conversación **nueva** con un turno trivial → **sin** bloque
  (no se hereda el plan ajeno); vuelta a la primera conversación + turno de
  seguimiento → el bloque **reaparece con el estado 2/3 restaurado**.
- Límite honesto del E2E: el bloque es estado **vivo** del runtime, así que al
  reentrar en una conversación desde el historial no se repinta hasta el
  siguiente turno, y no sobrevive a reiniciar la app (el plan no se persiste en
  SQLite; eso es alcance de F1 solo para la meta, no para los todos). No se
  declara persistencia de tareas.
- Gate canónico (`sentinel check 109A-5 --stages scripts/quality/stages.json`):
  `coverage` PASS, `sentinel` PASS (**0 errores, 10 warnings, 1 hint** = el
  baseline exacto del repo) y **FAIL solo** por `sccache-no-configurado`, que es
  el hallazgo de entorno ya registrado como tarea independiente "Gate: etapa Rust
  y sccache". Nota de paso: la regla `todo-pendiente` del medidor dispara con
  `//`+`TODO|FIXME|HACK|PENDIENTE|XXX` inmediatos, así que un comentario
  `/// pendiente, …` se contaba como marcador de deuda; se reformuló la frase
  (`/// … se queda` / `/// en pendiente`) para no añadir ruido que enmascare
  deuda real. El hint restante (`core/src/nucleo/context.rs:971`) es preexistente
  en un fichero ajeno a este bloque.

### F3 — Cierre con evidencia (pie "Meta lograda en Xs")

- Nuevo evento aditivo `MetaLograda { meta, lograda_en, elapsed_ms, turno_id }`
  emitido al marcar logrado; la UI pinta el badge en el pie del mensaje
  terminal del turno (icono Lucide meta + `Meta lograda en MM:SS` o `Meta
  lograda` sin reloj; `title`=texto de la meta; divisor previo separándolo de
  las acciones — paridad `MessagesTimeline.tsx:2538-2555`); historial de
  logros consultable (lista en el panel meta expandido; decidir posición en F3).
- El panel meta muestra el **tiempo de persecución** (neto de pausas) además
  del tiempo de turno, y el estado de pausa real de la meta.
- Verificación: tests del evento + render; E2E ventana real (fijar →
  pausar/reanudar congela → lograr pinta badge con tiempo correcto).

### F4 — Regla de bloqueo + migración del modo global

- El agente declara `bloqueada: true` con motivo; el backend cuenta turnos
  consecutivos con el mismo motivo y al tercero pausa la meta (congela reloj)
  + aviso visible; la instrucción prohíbe pausar por "difícil/incompleto"
  (paridad regla Synara).
- Migración coordinada con 109A-4 F4: `modo: meta` guardado →
  `predeterminado` + aviso visible una vez; `/meta <texto>` crea meta con
  ciclo de vida (F1) como override de turno en vez de prefijo ciego.
- Verificación: tests del contador (mismo motivo ×3 pausa; motivo distinto
  resetea); E2E migración; gate PASS.

## 6. Decisiones y riesgos

- Honestidad: el logro es **declarado** (agente o usuario), no verificado
  contra el mundo — igual que Synara. El valor está en el registro durable +
  tiempo + tareas visibles, no en una prueba independiente. Documentarlo así.
- `a_texto` cambia de formato al añadir `en_curso` (`[/]`): auditar
  consumidores del texto del plan antes de cambiarlo.
- `panelMeta.ts` tiene 198/300 líneas: F2–F3 añaden superficie; si roza el
  límite, extraer subcomponente en vez de engordarlo.
- Concurrencia de edición con 109A-4 F4 sobre los mismos ficheros UI: orden
  F1 → (109A-4 F4) → F2 → F3 → F4 de este plan si hay colisión.

## 7. Definition of Done

`cargo clippy` 0 + tests verdes, `tsc` EXIT 0, `vite build` OK, gate PASS,
E2E ventana real (fijar meta → tareas visibles en vivo → pausar/reanudar
congela el reloj → lograr pinta "Meta lograda en Xs" anclado al turno +
historial), roadmap actualizado y completada con evidencia. Sin deuda nueva:
deny de `meta` intacto con sus tests en verde.
