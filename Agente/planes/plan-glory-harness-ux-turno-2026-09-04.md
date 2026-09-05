---
Fecha: 2026-09-04
ID: 039A-3
Estado: EN EJECUCIÓN — bloque completo aprobado por el usuario ("empieza con el plan,
completalo todo", 04-09). 039A-3a + 039A-3b (P1-P6). P1 HECHO (persistir uso real + pie de
turno, con retoques visuales del usuario: botón copiar icono sin borde, sin línea separadora,
sin palabras "modelo"/"contexto", opacidad 0.6). P2 HECHO (editar/volver a punto: rewind
conversacional transaccional por `rowid` + menú por mensaje; commit `039A-3 (P2)`).
P3-P6 en curso.
Plan base: plan-glory-harness-desktop-2026-09-03.md (039A-1, fases F1-F6 + anexo §10)
Tipo: ampliación del desktop (UI + backend Tauri + core opcional)
Revisión: supervisor_thinker — VEREDICTO VIABLE CON RESERVAS; decisiones cerradas en §6 (Fase 0)
Referencias: opencode dialog-message.tsx (Revert/Copy/Fork), claurst file_history.rs + run.rs/TurnComplete, plan-bloque3-auditoria-referencias-2026-09-04.md (Fase 6 checkpoint/undo, fila #15 coste/uso)
---

# 039A-3 — UX de turno completa + navegación/conversación múltiple + respaldo de archivos

## 0. Resumen ejecutivo (para el usuario)

Este bloque convierte el chat del desktop en una experiencia "tipo task/opencode" con todo el
ciclo de una conversación:

1. **Pie de turno** al final de cada respuesta del asistente: tokens enviados/recibidos, modelo
   usado, uso de contexto, botón **copiar** (copia desde el último mensaje del usuario hasta el
   último mensaje de la IA).
2. **Volver a un punto / editar mensaje**: cada mensaje de usuario es un punto. Puedes editar un
   mensaje (se abre en la caja, listo para reenviar) o "volver a ese punto" (borra el hilo
   posterior a ese mensaje). Al editar+enviar, el hilo posterior se borra y se reenvía desde ahí.
   **Esto deshace el hilo** (mensajes/turnos/acciones posteriores se eliminan).
3. **Respaldo de archivos (segunda alternativa a git)**: cuando el harness modifica un archivo
   guarda automáticamente una copia previa (con hash) dentro del workspace donde trabaja. Así el
   "volver a punto" puede restaurar archivos de forma **segura**: si el archivo fue editado por
   otra fuente o a mano desde entonces, NO se toca y se avisa (nunca se borra un archivo).
4. **Botón ⋯ junto al título** del chat (acciones de la conversación, extensible).
5. **Sidebar colapsable y redimensionable**: se contrae por completo y aparece un botón a la
   izquierda (junto al título del chat) para expandir; también se puede arrastrar para cambiar el
   ancho.
6. **Dos conversaciones a la vez (máx. 2)**: desde el menú contextual se abre un segundo chat en
   un panel lateral (mecanismo de panel al que se añadirán más cosas en el futuro). Solo 2 chats.
7. **Indicador circular de contexto** junto al botón enviar (círculo sin relleno cuya línea se
   llena según el límite de contexto).
8. **Config de ventana de contexto**: visible y editable en Configuración, **default 150k**
   (inyectado por el desktop, sin cambiar el default del core 128k para no afectar a task).

**NO alcance (ahora):** deploy, escritura externa, SSH. El respaldo NO revierte archivos
automáticamente al "volver a punto" (solo si el usuario lo pide, pieza 3); no hay multi-tab
ilimitado; **no hay turnos simultáneos** (M1: 1 runtime; si un panel corre, el otro muestra
"termina el turno activo"). El core se toca SOLO para el hook opcional no-op de respaldo en
`SandboxArchivos` y la exclusión de `.glory-harness/` del sandbox (seguridad); enganchar en el
consumidor es inviable (el sandbox lo crea el core y el contenido previo vive en las tools).

**Entrega recomendada por la revisión:** separar en **039A-3a** (P1-P4: pie, editar/volver a
punto, vault, ⋯+sidebar) y **039A-3b** (P5-P6: 2 paneles M1, indicador circular, config 150k)
para no acoplar el riesgo del vault al del multi-panel. Pendiente de confirmar con el usuario.

---

## 1. Contexto verificado en código (estado real)

### Frontend (`desktop/ui/src`)
- Layout: `#app` (flex-col) > `#cuerpo` (flex-row) > `#sidebar` (260px fija) + `#chat`.
  `#chat` = `#cabecera-chat` + `#mensajes` + `#entrada`. `#cabecera-chat` solo tiene
  `<span class="titulo">` (sin botón ⋯). `#mensajes` es flex-col centrado max-width 860px.
  (main.ts ~30-60; layout.css 5-16, 32-38, 141-166)
- `montarSidebar` → `{ raiz, seleccionar, sustituir }`; menú por conversación (⋯/contextmenu) con
  Copiar ID, Renombrar, Archivar, Eliminar. `sidebar.ts`.
- `montarEntrada` → `Entrada { raiz, medir, setCorriendo, setModeloNombre, setModelo,
  setRazonamientoValor, getRazonamiento, setModo, enfocar, getCorriendo, getModo }`.
  Botón `#btn-enviar` (24×24, cuadrado) al final de `.controles` con `margin-left:auto`.
- `real.ts` (adaptador): `montar(contenedor, texto, opts, fin)`, escucha `agente-evento` (switch
  `ev.tipo`) y `turno-fin` (`cerrar(ok,error)`). Acumula `uso` (tokensPrompt/Complecion,
  ocupacionPct) y expone `usoUltimoTurno()`. En `case 'done'` solo limpia
  `asistente/herramienta`. NO hay pie por turno en real (solo en la simulación).
- No existe editar mensaje, ni botón copiar de conversación, ni ⋯ en cabecera, ni footer de uso.
- Helpers: `el`, `qs`, `vaciar`, `scrollAlFinal` (dom.ts); `abrirMenuContextual/crearItemMenu/
  crearSeparadorMenu/cerrarMenuActual` (menu.ts); `copiarAlPortapapeles` (privada en sidebar.ts).
- Los mensajes son nodos DOM sin identidad (`data-id`) ni contenedor de acciones.

### Backend Tauri (`desktop/src-tauri/src/main.rs`) + SQLite
- Estado: una sola sesión, una sola `conversacion_id` actual, un turno activo (`estado.turno`).
- Comandos IPC (17): `abrir_sesion`, `enviar_turno`, `cancelar_turno`, `reconfigurar_sesion`,
  `responder_aprobacion`, `pendientes_aprobacion`, `conversacion_nueva`, `listar_conversaciones`,
  `cargar_conversacion`, `renombrar_conversacion`, `archivar_conversacion`,
  `eliminar_conversacion`, `proveedores_disponibles`, `config_leer`, `config_guardar`,
  `elegir_workspace`, `actualizar_meta`. (generated_handler main.rs:944-963)
- SQLite (`cli/src/persistencia_sqlite.rs`): `conversaciones`, `mensajes` (id, conversacion_id,
  rol, contenido, creado_en), `turnos` (id, conversacion_id, user_id, estado, resumen,
  provider, modelo, tokens_prompt, tokens_complecion, tools_ejecutadas, duracion_ms, error),
  `acciones` (id, turno_id, tool, ok, resumen, argumentos_json, diff, creado_en), `config`,
  `memoria`, `skills`, `tareas`, `tarea_logs`. Historial **append-only** (sin editar/rewind).
- **Hallazgo**: el runtime guarda `turnos.tokens_prompt/complecion` = **0** (nunca se actualizan
  con el uso real) y `provider/modelo` = los **solicitados** (`turno_config`), no los reales tras
  fallback. El uso/modelo reales solo viajan transitorios en `AgenteEvento::Usage`.
- Config persistida real: `user_id`, `workspace`, `nivelRazonamiento`. NO se persisten
  provider/modelo/modo/temperatura/ventana.

### Core (`core/src`)
- `AgenteEvento` (`#[serde(tag="tipo", rename_all="snake_case")]`): Token, ToolStart, ToolResult
  (con diff), RequiereAprobacion, PeticionAprobacion, PermisoDenegado, SubagenteInicio/Fin,
  PlanPropuesto, Usage (tokens_prompt/complecion + ocupacion_pct + provider/modelo opcionales),
  Contexto, ContextoDetalle (max_ventana, reserva_salida, secciones, total_entrada,
  ocupacion_pct), Telemetria, Error, Done. `core/src/evento.rs`.
- `ContextoConfig` (`core/src/context.rs:47-80`): `max_ventana` default **128_000**,
  `reserva_salida` 20_000. Único límite fijo por runtime (no por modelo). No configurable por IPC.
- El sandbox lo crea el core (`AgentRuntime::nuevo` → `sandbox_desde_entorno`, runtime.rs:267-271,
  1384-1402) exigiendo `AGENTE_MODO == "local"` (fijado por `construir_harness_con`, cli/run.rs).
- `SandboxArchivos::escribir` (`core/src/sandbox.rs:185-201`) es el punto único de escritura de
  `file_write`/`file_patch`; `file_write`/`file_patch` ya capturan `previo`/`original` en memoria
  antes de escribir y producen `diff_lineas`. (`tools_archivo.rs:181-199`, `246-277`).
- `construir_harness_con` (`cli/src/run.rs:85-187`) arma la sesión del desktop; `TurnoConfig` y
  `contexto` son públicos y mutables antes de `AgentRuntime::nuevo` → punto natural para inyectar
  150k sin tocar el default del core.

### Referencias de UX (data/referencias-cli/, solo lectura)
- opencode `dialog-message.tsx:27-132`: 3 acciones por mensaje → Revert (`session.revert`,
  "undo messages and file changes"), Copy (`message.copy`), Fork (`session.fork`). Es el modelo
  de "volver a punto / editar".
- claurst `file_history.rs:17-197`: `FileHistoryEntry { path, before_hash, after_hash,
  before_text?, after_text?, binary, turn_index, timestamp_ms, tool_name }` + `state_at_turn`
  (before_text de la 1ª mod con turn_index >= rewind) + `snapshots_for_turn`. Modelo de datos
  para nuestro vault de respaldos.
- claurst `run.rs:230-234`: suma input+output+cache por turno en `context_used_tokens`;
  `render.rs:2594-2644`: footer con `{used_k}k/{total_k}k` + % hasta auto-compact.
  Es el modelo del pie de turno + indicador de contexto.

---

## 2. Arquitectura propuesta

### 2.1 Modelo de datos (SQLite) — ampliaciones
- `turnos`: se añade **persistir el uso real** (actualizar `tokens_prompt/complecion` reales y
  `provider/modelo` reales tras el turno). Se rellena desde el `Usage`/`Telemetria` reales.
  (Pieza P1 + P5 lo consumen.)
- Nueva tabla `turno_uso` (o columnas): no hace falta si `turnos` ya guarda tokens+modelo; el
  pie de turno puede leer el `turno` más reciente de la conversación. **Decisión**: usar `turnos`
  existente rellenando los campos reales con el Usage real (P1); `duracion_ms` ya existe.
- El vault de respaldos **NO usa tabla SQLite** (ver 2.3): índice en disco (JSONL por
  conversación) + árbol `.glory-harness/backups/`. Suficiente para 1-2 usuarios y sin consultas
  relacionales que justifiquen una migración de esquema.
- (Reutilizable) `mensajes` ya tiene `id`, `rol`, `contenido`, `creado_en` → base para editar y
  para el "punto" (mensaje de usuario).

### 2.2 Estado del desktop: de una conversación a un "panel de chats"
- Hoy: 1 `Sesion` con 1 `conversacion_id`. Para 2 chats simultáneos hay que pasar a **N
  sesiones/conversaciones activas** (N ≤ 2). Diseño mínimo:
  - `Sesion` pasa a tener 1 conversación activa cada una, pero el `Estado` global mantiene un
    **mapa de paneles** (hasta 2): cada panel tiene su `Sesion` (runtime propio o compartido +
    `conversacion_id` + `turno_id` propio). El daemon (`cli/src/daemon.rs`) ya es multi-sesión
    (mapa session_id → Sesion) → reutilizar ese patrón.
  - **Decisión de runtime (M1, cerrada)**: **1 runtime + estados por conversación, SIN turnos
    simultáneos**. El desktop es de 1-2 usuarios con operación serial humana; al enviar en un
    panel se desactiva el otro ("termina el turno activo", patrón ya existente en `enviar_turno`).
    Cero concurrencia real, cero duplicación de runtime/aprobaciones/cancelación/contexto.
    **M2 (2 runtimes reales, patrón daemon) queda descartado**: duplica `abrir_sesion_interna`,
    aprobaciones, `reconfigurar_sesion` y el flag de turno, y exige demultiplexar 2 streams vivos
    en el `EventEmitter` de Tauri — coste sin modelo de carga que lo justifique.
    Refactor previo (Fase 1): extraer `PanelManager` (mapa `panel_id` → conversación/turno)
    manteniendo 1 panel; etiquetar `agente-evento`/`turno-fin` con `panel_id` en el emisor
    (main.rs).
- `enviar_turno`/`cancelar_turno`/`cargar_conversacion`/etc. reciben un identificador de panel
  (`panel_id` o `conversacion_id` explícito) en vez de asumir "la actual". Los eventos
  `agente-evento`/`turno-fin` llevan el `panel_id` para que la UI sepa a qué chat va.
- La UI: `#cuerpo` pasa de `#sidebar + #chat` a `#sidebar + #paneles` donde `#paneles` es un
  flex de 1-2 `#chat`. El "panel lateral" es un segundo `#chat` (extensible en el futuro).

### 2.3 Vault de respaldos de archivos (segunda alternativa a git)
- **Ubicación (decisión usuario): dentro del área donde trabaja el harness** → en el workspace,
  carpeta oculta `.glory-harness/backups/` (gitignored, EXCLUIDA del sandbox de escritura del
  agente). **Exclusión = BLOQUEANTE de seguridad**: hoy el sandbox (sandbox.rs) solo excluye
  secretos y no excluye `.glory-harness/`; si el agente pudiera escribir ahí podría envenenar
  sus propios respaldos. Se añade la regla en `es_secreto`/`resolver_para_escribir` (core).
- **Modelo mínimo (decisión cerrada, sin SQLite)**: árbol `.glory-harness/backups/<hash>/<ruta>`
  con el contenido previo **completo en bytes** + **índice de log en disco** (JSONL por
  conversación: `{ conversacion_id, turno_id, ruta_relativa, before_hash, after_hash,
  timestamp_ms, tool_name }`). El historial de respaldos es un log; no hay consulta relacional
  que justifique una tabla. Dedup por `before_hash` (si el contenido ya está respaldado, no
  re-copiar). **Retención/GC**: conservar respaldos del último N turnos por conversación (p.ej.
  limpiar al hacer rewind los respaldos de los turnos borrados) — política definida en P3.
- **Contenido previo COMPLETO en bytes** (decisión crítica): el respaldo debe capturar el previo
  con `std::fs::read` del archivo existente, NO el string truncado a 1MB que las tools usan para
  el diff — si no, restaurar un archivo >1MB produciría un archivo corrupto.
- **Punto de captura (decisión cerrada: opción B)**: enganchar en `SandboxArchivos::escribir`
  (punto único, sandbox.rs:185-201), con un **trait opcional no-op**
  `Option<Arc<dyn RespaldoArchivos>>` inyectado por el consumidor. El hook recibe la `relativa`
  **ya validada por el sandbox** (se llama tras `resolver_para_escribir`) y el contenido previo.
  - Por qué NO opción C (capturar en tools_archivo.rs): deja fuera `aplicar_plan` (plan.rs:122-
    136) y `todo`, que también escriben vía `sandbox.escribir` → un respaldo que no cubre TODA
    escritura del agente es un respaldo mentiroso. Además, **tools_archivo.rs lo está editando
    otro agente en paralelo** (working tree sucio) → opción C colisionaría.
  - Por qué NO interceptar en el consumidor: el sandbox lo crea el core (`runtime.rs:270` →
    `sandbox_desde_entorno`) y el contenido previo vive en las tools; no hay punto de captura en
    el consumidor sin tocar el core igualmente.
  - No rompe a task: implementación por defecto no-op; test en core de que el hook no-op no
    cambia el comportamiento de `escribir`.
- **Comportamiento ante fallo del hook**: **fail-open con log** (un respaldo que falla no debe
  tumbar el turno del agente), salvo que el fixture demuestre lo contrario.
- **Restaurar/deshacer (semántica que el usuario marcó como la que va a fallar)**:
  - Al "volver a punto" NO se restauran archivos automáticamente (decisión usuario: rewind solo
    BD + restaurar a voluntad). El tramo borrado queda marcado con sus archivos tocados.
  - Restaurar = acción explícita. **Comprobación de fuente (decisión cerrada): comparar el hash
    ACTUAL contra el `after_hash` del ÚLTIMO respaldo GLOBAL de esa ruta** (cualquier
    conversación/turno), NO contra el último respaldo del tramo.
    - Por qué GLOBAL y no del tramo: si T1 escribe A→B y T2 escribe B→C, volver al punto de T1 y
      restaurar comparando contra "lo que dejó el tramo" (B) vs el estado actual (C) daría un
      falso "cambió fuera". Contra el último respaldo global (C, dejado por T2, que SÍ es el
      harness) la fuente es el harness → se restaura al previo de T1 (A).
    - Si coincide → el harness fue la última fuente → restaurar el `before_text` del punto.
    - Si NO coincide → el archivo fue editado por otra fuente o a mano después → **NO tocar** y
      avisar ("no se restaura X: cambió fuera del harness"). Sin opción "forzar" en v1.
    - Nunca se borra un archivo; restaurar = escribir el contenido previo respaldado (crear el
      archivo si no existe es decisión de la acción, con aviso). Si la ruta ya no existe en el
      workspace, tratar "no existe" como estado a decidir (recrear con aviso, no automático).
- Esto cubre "deshacer solo lo editado en ese hilo": el vault guarda por `conversacion_id` +
  `turno_id` + `ruta`, y la comprobación de fuente usa el **último respaldo global por ruta**.

### 2.4 Pie de turno + copiar (referencia opencode Copy, claurst TurnComplete)
- Al final de cada respuesta del asistente (tras `done`/`turno-fin` ok) se añade un bloque
  `.pie-turno` con:
  - tokens enviados (prompt) / recibidos (compleción) — del `Usage` real (hoy transitorio; se
    persiste en `turnos` en P1 para sobrevivir recarga).
  - modelo usado (real, del `Usage.provider/modelo`; hoy el turno guarda el solicitado → P1 lo
    corrige).
  - uso de contexto: `{usado_k}k/{total_k}k` + % (de `ContextoDetalle.ocupacion_pct` /
    `max_ventana`). Total por defecto = la config 150k del desktop.
  - botón **Copiar**: copia desde el último mensaje de usuario hasta el último mensaje de la IA
    (texto plano, incluido el pie: tokens/modelo). Reutiliza `copiarAlPortapapeles` (extraerlo a
    util compartida).
- El pie es **estático** (se repinta al recargar desde `turnos` + mensajes), a diferencia del
  streaming.

### 2.5 Volver a punto / editar mensaje (referencia opencode Revert + claurst file_history)
- Cada mensaje de usuario (y quizá cada mensaje) gana un menú de acciones (reutilizar `menu.ts`):
  - **Editar**: pone el texto del mensaje en el textarea de la entrada (en modo "edición").
    Al enviar: si el mensaje era de un punto medio, **borra el hilo posterior** a ese mensaje y
    reenvía desde ahí (rewind de BD + envío). El texto editado reemplaza al original.
  - **Volver a este punto**: borra el hilo posterior a ese mensaje (mensajes/turnos/acciones de
    ese tramo) y deja la conversación en ese estado. (Restaurar archivos = acción separada, 2.3.)
- Backend: nuevo comando `rewind_conversacion { conversacion_id, hasta_mensaje_id }` (o
  `hasta_turno`) que, en una transacción, borra `mensajes` con `creado_en > punto`, `acciones`
  de `turnos` posteriores, y `turnos` posteriores; y devuelve los archivos del tramo (para el
  respaldo). Y `editar_mensaje { conversacion_id, mensaje_id, nuevo_texto }` + rewind.
- **Importante (lo que el usuario advirtió)**: el rewind de BD es seguro porque el historial es
  append-only y los `id`/`creado_en` son estables. El riesgo NO está en borrar mensajes (es lo
  que se pide) sino en **asumir que se pueden revertir archivos** → eso se resuelve con el vault
  y la comprobación de fuente (2.3). Nunca un rewind de conversación borra archivos por sí solo.

### 2.6 Botón ⋯ junto al título (cabecera)
- `montarCabeceraChat` gana un menú `⋯` (`#btn-conv-mas`) a la derecha del título con acciones de
  la conversación (renombrar, archivar, eliminar, copiar conversación, y futuras: fork/export).
  Reutiliza `menu.ts` y los callbacks existentes de la sidebar (o se centralizan en main).

### 2.7 Sidebar colapsable + redimensionable + botón expandir
- `#sidebar` pasa de `width:260px` a `width:var(--sidebar-ancho)` controlado por JS
  (default 260, clamp p.ej. 180-420) + arrastre en el borde derecho (drag handle) → ajusta la
  variable/ancho en vivo.
- Colapso total: un botón de colapsar (en el pie de la sidebar o el borde). Al colapsar,
  `#sidebar` se oculta (width 0 / display none) y aparece **un botón a la izquierda, junto al
  título del chat**, para expandir (`#btn-abrir-sidebar` en `#cabecera-chat`).
- Estado persistido en `config` (`sidebar_ancho`, `sidebar_colapsada`).
- CSS: mover el ancho a una variable `--sidebar-ancho` en variables.css o layout.css.

### 2.8 Dos conversaciones (panel lateral, máx 2)
- `#cuerpo > #paneles` (flex) con 1-2 `#chat`. El segundo se abre desde el menú contextual de una
  conversación en la sidebar → "Abrir en panel lateral" (o un botón). Solo si hay <2 chats.
- Cada `#chat` tiene su cabecera con su título y su ⋯. El segundo panel es "lateral" y se puede
  cerrar (botón × en su cabecera). Mecanismo de panel pensado para crecer (futuro: más tipos de
  panel).
- Backend: paneles = hasta 2 `Sesion` activas (2.2). La UI etiqueta los eventos con `panel_id`.

### 2.9 Indicador circular de contexto (junto a enviar)
- En `#entrada .controles`, justo antes o junto a `#btn-enviar`, un `<svg>` círculo sin relleno
  (`stroke`) cuya circunferencia se rellena según `ocupacion_pct` (del `ContextoDetalle` /
  `usage`). Estética monocromo (solo stroke, sin relleno). 0% → vacío, 100% → círculo completo.
- Reutiliza el dato que ya actualiza `uso.ocupacionPct` en `real.ts`.

### 2.10 Config ventana de contexto (default 150k)
- Nueva opción en Configuración (panel Contexto): "Ventana de contexto" (número/select),
  default **150000**. Persistida en `config` (`contexto_max_ventana`).
- Al construir la sesión (`construir_harness_con`, cli/run.rs:85-187), el desktop inyecta
  `TurnoConfig.contexto.max_ventana = 150_000` (o el valor configurado) antes de
  `AgentRuntime::nuevo`. `config` es pública y mutable antes del runtime; también `reconfigurar_
  sesion` lo reconstruye → aplicar el mismo valor al reconfigurar. **No se cambia el default
  del core** (128k) para no afectar a task.
- **Fuente de verdad única del % (decisión cerrada)**: el `ContextoDetalle` del evento ya trae
  `max_ventana` (150k configurado) y `ocupacion_pct` (calculado por el runtime sobre la
  `ventana_efectiva` = max_ventana − reserva_salida 20k = 130k). El pie y el indicador circular
  usan SIEMPRE los valores del `ContextoDetalle` (total a mostrar = `max_ventana`; % =
  `ocupacion_pct`) para no mostrar dos porcentajes distintos como si fueran el mismo.
- **Tope blando**: 150k es lo configurado por el desktop; si el modelo real tiene menos ventana,
  el % puede no reflejar el tope real del proveedor. Se documenta como tope blando (no se valida
  contra el catálogo en v1, decisión de bajo riesgo registrada). Consecuencia conocida: subir la
  ventana retrasa la compactación (el umbral se calcula sobre `max_ventana`, context.rs:280+).

---

## 3. Restricciones / dependencias / riesgos

- **Dependencias**: SQLite (rusqlite) ya está; sin nuevas dependencias pesadas. UI sin framework.
- **Riesgo alto (usuario avisó): backup/rewind de archivos.** Mitigación: vault por
  conversación/turno + hash + comprobación de fuente antes de restaurar; nunca borrar archivos;
  rewind de BD separado de restauración de archivos.
- **Riesgo: tocar core (sandbox hook) puede afectar a task.** Mitigación: hook opcional (trait con
  implementación no-op por defecto); feature/parámetro; validar con `cargo test --workspace` y
  el gate. Si el riesgo se confirma, caer a opción C (tools_archivo) o a interceptar en el
  consumidor.
- **Riesgo: 2 sesiones simultáneas** (runtime/estado/turno). Mitigación: patrón del daemon
  (mapa de sesiones), locks por sesión, eventos etiquetados con panel_id; solo 2.
- **Riesgo: `tokens`/`modelo` del turno hoy = 0 / solicitado.** P1 corrige la persistencia con el
  uso real; el pie depende de eso.
- **Riesgo: rendimiento del vault** (muchas escrituras). Mitigación: solo guardar previo cuando
  cambia (dedup por hash), ruta eficiente en disco, sin bloquear el turno (o async).
- **Riesgo: editar/rewind con turno en curso.** Bloquear rewind/editar si hay turno activo en ese
  panel (igual que hoy `enviar_turno` rechaza con turno activo).

---

## 4. Fases (checklists) con evidencia mínima por fase

> **Fase 0 (decisiones) y Fase 1 (refactor de estado) son PRERREQUISITO.** La Fase 1 es un
> refactor de responsabilidades del backend (PanelManager + etiquetado de eventos) que debe
> preceder a las features para no acumular responsabilidades en `enviar_turno`. Ver §6.

### Fase 0 — Decisiones cerradas (sin código; ver §6)
- [ ] Adoptar M1 (1 runtime, turnos no simultáneos) — **cerrada**. [§6.1]
- [ ] Comprobación de fuente = último respaldo GLOBAL por ruta — **cerrada** (usuario: "lo que
      sea mejor"). [§6.2]
- [ ] Hook opción B (trait opcional no-op en `SandboxArchivos::escribir`) — **cerrada**. [§6.3]
- [ ] Excluir `.glory-harness/` del sandbox (core) — **cerrada** (bloqueante seguridad). [§6.4]
- [ ] Previo completo en bytes (no truncado a 1MB) — **cerrada**. [§6.5]
- [ ] Vault mínimo sin SQLite (árbol + JSONL) + retención/GC — **cerrada**. [§6.6]
- [ ] Orden del rewind por `rowid`/orden monotónico, no por `creado_en` — **cerrada**. [§6.7]
- [ ] Persistir uso real desde main.rs (sin tocar core) — **cerrada**. [§6.8]
- [ ] Pie de turno en `main.ts`/`mensajes.ts`, no en `real.ts` — **cerrada**. [§6.9]
- [ ] Config 150k: inyectar en `construir_harness_con`; fuente única = `ContextoDetalle`. [§6.10]
- [ ] Confirmar con el usuario: 039A-3a (P1-P4) vs bloque completo 039A-3. [§6.11]

### Fase 1 — Refactor de estado (prerrequisito, P5-M1)
- [ ] Extraer `PanelManager` (mapa `panel_id` → conversación/turno) manteniendo 1 panel.
- [ ] Etiquetar `agente-evento`/`turno-fin` con `panel_id` en el emisor (main.rs).
- [ ] Unit: turno en panel A bloquea envío en panel B (M1); eventos llegan etiquetados.

### P1 — Persistir uso/modelo real del turno + pie de turno (039A-3a) — HECHO (04-09)
- [x] Backend main.rs: acumular el Usage real por `turno_id` (los eventos Usage ya llegan; cada
      `llm_llamada` emite Usage real, runtime.rs:847; un turno con N tools emite N Usage parciales
      → SUMAR tokens y quedarse con el último provider/modelo). Al recibir `turno-fin` ok, hacer
      `UPDATE turnos SET tokens_prompt=?, tokens_complecion=?, provider=?, modelo=? WHERE id=?`.
      **No tocar core**: `tokens_prompt_total` (runtime.rs:377) es `let` inmutable = 0 y nunca se
      acumula; arreglarlo en core solo beneficiaría a task (fuera de alcance).
- [x] Front: `real.ts` ya acumula `uso`; notificar a `main` el cierre con
      `{tokensPrompt, tokensComplecion, modelo, ocupacionPct, maxVentana}`.
- [x] Front: bloque `.pie-turno` renderizado en `main.ts`/`mensajes.ts` (donde se decide el fin de
      turno), NO dentro de `real.ts` (solo acumula uso). Contenido: tokens enviados/recibidos,
      modelo usado, uso de contexto `{usado_k}k/{total_k}k` + % y botón Copiar. Copiar = texto
      desde el último user hasta el último assistant. Añadir `data-id`/contenedor a los mensajes
      (`mensajes.ts` + `pintarHistorial`).
- [x] Repintar el pie al recargar (desde `turnos` con uso real + mensajes) — ampliar
      `cargar_conversacion`.
- [x] Mock de navegador: simular el pie para verlo sin Tauri.
- [x] Evidencia: unit del UPDATE de uso; E2E `tauri dev` con fallback de proveedor (cambiar a un
      modelo caído y ver el pie con el modelo/tokens reales); type-check + build.
      — Retoques visuales del usuario (04-09): botón copiar = icono sin borde; sin línea
      separadora superior; sin palabras "modelo"/"contexto"; opacidad 0.6. Verificado en mock.

### P2 — Acciones por mensaje: editar / volver a punto (039A-3a) — HECHO (04-09)
- [x] Front: menú de acciones por mensaje (editar / volver a este punto / copiar), reutilizando
      `menu.ts`; `data-id` en `crearMensajeUsuario` + botón `⋯` flotante (hover/foco).
- [x] Backend: comando `rewind_conversacion` **transaccional** anclado en **orden monotónico
      (`rowid`)**, NO en `creado_en` (precisión 1 s rompe el filtrado si dos mensajes comparten
      segundo). Borra mensajes posteriores + turnos posteriores + acciones de esos turnos; devuelve
      la `CargaConversacion` recortada (pintar sin recargar). "Mostrar archivos del tramo"
      queda para P3 (vault) — aquí no se devuelven archivos.
- [x] Backend: DECISIÓN de implementación: NO hay comando `editar_mensaje` separado. El orden
      transaccional del editar+enviar se resuelve con `rewind_conversacion(mensaje_id, editar=true)`
      (borra el objetivo y el hilo posterior) + reenvío: el texto editado se monta como el nuevo
      "user" del punto (se cumple el objetivo sin comando extra ni duplicación de lógica).
- [x] Al editar + enviar: rewind `editar=true` + reenvío (el texto editado reemplaza al original).
- [x] Volver a punto: rewind `editar=false` (conserva el mensaje objetivo) + repintar con la carga
      devuelta. La restauración de archivos del tramo es P3 (acción separada).
- [x] Bloquear rewind/editar si hay turno activo (guard en `volverA`/`empezarEdicion`/comando).
- [x] Evidencia: unit `rewind_conserva_y_edita_tramo_posterior` + `rewind_rechaza_ajeno_o_no_usuario`
      (cli, verdes); E2E editar+reenviar en navegador (mock: flujo `ponerEnEdicion` → enviar →
      `onEnviar(texto, id)` → cancelar). E2E `tauri dev` pendiente (requiere app real con ids).

### P3 — Vault de respaldos + restaurar seguro (039A-3a; la pieza delicada)
- [ ] Core: hook opcional no-op en `SandboxArchivos::escribir` + exclusión de `.glory-harness/`
      del sandbox + test (el no-op no cambia `escribir`; el agente NO puede escribir bajo
      `.glory-harness/`).
- [ ] Desktop: implementar el trait → árbol `.glory-harness/backups/<hash>/<ruta>` (previo
      completo en bytes) + índice JSONL por conversación. Dedup por `before_hash`. Retención/GC:
      limpiar respaldos de turnos borrados al rewind.
- [ ] Fail-open con log si el hook falla (no tumba el turno).
- [ ] Front/backend: acción "restaurar archivos de este tramo" con comprobación de fuente contra
      el **último respaldo global por ruta**; re-validar la ruta contra el sandbox antes de
      escribir; si cambió externamente → NO tocar y avisar. Sin "forzar" en v1.
- [ ] Evidencia (fixture funcional obligatorio): (1) dos turnos tocan el mismo archivo → rewind
      al 1º + restaurar restaura correctamente (sin falso "cambió fuera"); (2) edición manual
      externa entre el turno y la restauración → avisa y NO toca; (3) archivo >1MB (previo
      completo); (4) hook no-op no rompe `escribir` (test core).

### P4 — Botón ⋯ en cabecera + sidebar colapsable/redimensionable + botón expandir (039A-3a)
- [ ] Front: menú ⋯ en cabecera (renombrar/archivar/eliminar/copiar + futuras), reutilizando
      `menu.ts`; extraer `copiarAlPortapapeles` (hoy privada en sidebar.ts) a util compartida.
- [ ] Front: variable `--sidebar-ancho` + drag para redimensionar (clamp 180-420) + botón
      colapsar + botón expandir junto al título (`#btn-abrir-sidebar` en `#cabecera-chat`);
      persistir ancho/colapsada en config (`sidebar_ancho`, `sidebar_colapsada`).
- [ ] Evidencia: type-check + build; ver en navegador (mock).

### P5 — Dos conversaciones (panel lateral máx 2) (039A-3b; requiere Fase 1)
- [ ] Backend: `PanelManager` (mapa hasta 2 paneles); comandos aceptan `panel_id`; eventos llevan
      `panel_id`. Turnos NO simultáneos (M1): al enviar en un panel se desactiva el otro.
- [ ] Front: `#cuerpo > #paneles` con 1-2 `#chat`; abrir 2º desde menú contextual de la
      conversación; cerrar panel; cabecera/entrada por panel. Estado vacío/ocupado del 2º panel
      definido; la sidebar mantiene "activa" la conversación del panel enfocado. Responsive: a
      ancho pequeño los 2 chats se apilan o se prohíbe abrir el 2º.
- [ ] Evidencia: E2E `tauri dev` (2 conversaciones, turnos seriales, eventos al panel correcto).

### P6 — Indicador circular de contexto + config ventana 150k (039A-3b)
- [ ] Front: SVG círculo de contexto junto a enviar (stroke sin relleno; `stroke-dasharray` según
      `uso.ocupacionPct` del `ContextoDetalle`).
- [ ] Config: opción "Ventana de contexto" default 150000, persistida.
- [ ] Backend: inyectar `contexto.max_ventana` configurado (default 150k) en `construir_harness_con`
      y en `reconfigurar_sesion` (sin tocar default del core). Fuente única = `ContextoDetalle`.
- [ ] Evidencia: type-check/build; ver el círculo llenarse; config persiste (150k llega como
      `ContextoDetalle.max_ventana`).

---

## 5. Criterios de aceptación / DoD
- [ ] `npm run type-check` + `npm run build` (desktop/ui) limpios.
- [ ] `cargo test --workspace` (core/cli) verde (el hook de respaldo es opcional/no-op → no rompe
      a task).
- [ ] Gate Sentinel del bloque (039A-3) PASS con reporte.
- [ ] Verificación funcional: mock de navegador (pie de turno, editar/volver a punto, ⋯, sidebar
      colapsable, indicador circular, config 150k) y `tauri dev` (2 chats, rewind real, respaldo
      de archivos real con fixture).
- [ ] El rewind de BD no borra archivos por sí solo; la restauración comprueba fuente y avisa.
- [ ] Documentación actualizada (roadmap/plan/completados) con evidencia.

---

## 6. Decisiones cerradas (Fase 0) — resultado de la revisión de supervisor_thinker

Estas decisiones sustituyen a los "riesgos abiertos" del borrador. Se cerraron con criterio de
arquitectura y validación contra el código real.

### 6.1 M1 vs M2 (2 chats) → **M1: 1 runtime, turnos NO simultáneos**
Modelo de carga: desktop local de 1-2 usuarios, operación serial. M2 (2 runtimes) duplica
`abrir_sesion_interna`, aprobaciones, `reconfigurar_sesion` y el flag de turno, y exige
demultiplexar 2 streams vivos en el EventEmitter de Tauri (hoy los eventos NO llevan `panel_id`;
main.rs:488/508/511/515 emiten globales). M1 = refactor a `PanelManager` + etiquetar eventos con
`panel_id`; al enviar en A se desactiva B (patrón ya existente). Riesgo de eventos cruzados:
cero concurrencia real.

### 6.2 Comprobación de fuente → **último respaldo GLOBAL por ruta**
NO el `after_hash` del tramo. El caso "T1 A→B, T2 B→C; volver a T1" comparando contra el tramo
(B) vs actual (C) daría falso "cambió fuera". Contra el último respaldo global (C, dejado por
T2 = harness) se restaura a A. Confirmado con el usuario ("lo que sea mejor").

### 6.3 Punto de captura del vault → **opción B: hook en `SandboxArchivos::escribir`**
Trait opcional no-op (`Option<Arc<dyn RespaldoArchivos>>`) inyectado por el consumidor. Cubre
file_write, file_patch, `aplicar_plan` (plan.rs:122-136) y `todo` — TODA escritura vía
`sandbox.escribir`. Opción C (tools_archivo) dejaría fuera aplicar_plan/todo (respaldo mentiroso)
Y colisiona con el otro agente que edita `tools_archivo.rs` en paralelo. Interceptar en el
consumidor es inviable (sandbox lo crea el core). No rompe a task (no-op por defecto) + test.

### 6.4 Excluir `.glory-harness/` del sandbox → **cerrada (bloqueante seguridad)**
Hoy el sandbox solo excluye secretos; el agente podría escribir en `.glory-harness/backups/` y
envenenar sus respaldos. Regla nueva en `es_secreto`/`resolver_para_escribir` (core) + test.

### 6.5 Contenido previo → **completo en bytes (`fs::read`)**
NO el string truncado a 1MB que las tools usan para el diff: restaurar un archivo >1MB con el
previo truncado produciría un archivo corrupto.

### 6.6 Vault mínimo → **árbol + índice JSONL, SIN tabla SQLite**
`respaldos_archivo` (tabla) es sobreingeniería: el historial es un log, sin consultas
relacionales que la justifiquen. Árbol `.glory-harness/backups/<hash>/<ruta>` + JSONL por
conversación. Dedup por `before_hash`. Retención/GC: limpiar respaldos de turnos borrados al
rewind.

### 6.7 Orden del rewind → **`rowid`/orden monotónico, no `creado_en`**
`creado_en` tiene precisión de 1 s (rfc3339 Secs en `guardar_mensaje`); dos mensajes en el mismo
segundo romperían el filtrado. Anclar el rewind en el orden de inserción.

### 6.8 Persistir uso real → **en main.rs, SIN tocar core**
`llm_llamada` (runtime.rs:847) ya emite Usage real por llamada. Un turno con N tools emite N
Usage parciales → main.rs suma tokens y conserva el último provider/modelo; al `turno-fin` ok
hace UPDATE a `turnos`. `tokens_prompt_total` (runtime.rs:377) es `let` inmutable = 0 (nunca se
acumula); arreglarlo en core solo beneficiaría a task → fuera de alcance.

### 6.9 Pie de turno → **renderizar en `main.ts`/`mensajes.ts`, no en `real.ts`**
`real.ts` no conoce la estructura persistida de mensajes ni puede copiar "último user → último
assistant" de forma fiable sin `data-id`/contenedor. El adaptador solo acumula uso y notifica a
main; el pie (estático, repintable) se renderiza donde se decide el fin de turno.

### 6.10 Config 150k → **inyectar en `construir_harness_con` (+ `reconfigurar_sesion`)**
`config` es pública/mutable antes de `AgentRuntime::nuevo`. Fuente de verdad única: el
`ContextoDetalle` del evento (`max_ventana` = configurado; `ocupacion_pct` = calculado sobre la
`ventana_efectiva` = max_ventana − reserva 20k). Pie e indicador usan SIEMPRE esos valores. 150k
es tope blando (no validado vs catálogo en v1); retrasa la compactación (umbral sobre
`max_ventana`).

### 6.11 Entrega → **aprobado bloque COMPLETO 039A-3 (P1-P6) el 04-09**
El revisor recomendaba separar 039A-3a (P1-P4) de 039A-3b (P5-P6); el usuario eligió al aprobar
la ejecución ("empieza con el plan, completalo todo") ejecutar el bloque completo. Se mantiene
el orden de riesgo: P1 (pie) → P2 (editar/volver) → P3 (vault) → P4 (⋯+sidebar) → P5 (2 paneles)
→ P6 (indicador + 150k). P1 completado y validado (mock + type-check + cargo check); P2
completado y validado (unit rewind + type-check + build + flujo mock).

### 6.12 Entorno → **preservar cambios ajenos**
Otro agente trabaja en paralelo en core/cli (working tree sucio: daemon/run/runtime/
tools_archivo/tools_web/ports/guardas/fetch). NO tocar/stage/descartar sus archivos. Esto
refuerza la opción B (hook en `sandbox.rs`, que el otro agente no toca) y exige commits por
bloque solo con archivos propios.

### 6.13 Nota sobre `panelMeta` y tokens de `enviar_turno`
El panel meta actual ya muestra tiempo y tokens desde `AgenteEvento.Usage` (commit `5dcefe4`); el
pie de turno (P1) reutiliza esa misma fuente y la persiste para sobrevivir recarga. Sin cambios
adicionales en `real.ts` para el pie (solo notificación a main).

---

## 7. No alcance
- Deploy/push/escritura externa (requieren autorización aparte).
- Restaurar archivos automáticamente al volver a punto (solo bajo demanda, decisión usuario).
- Más de 2 chats simultáneos.
- Fork/export de conversación (se deja como acción futura del ⋯; opencode sí lo tiene pero queda
  fuera de este bloque para no ampliarlo).
- Temas/atajos/multi-cuenta (Bloque 3 del plan de auditoría, separado).

---

## Checklist resumen (para el roadmap)

**Fase 0 — Decisiones (cerradas en §6):** M1 · fuente global por ruta · hook opción B ·
exclusión `.glory-harness/` · previo completo · vault sin SQLite · orden por `rowid` · uso en
main.rs · pie en main/mensajes · config 150k en construir_harness_con.

**Fase 1 — Refactor de estado (prerrequisito):** PanelManager + etiquetar eventos con `panel_id`.

**039A-3a (núcleo de valor, primero):**
1. P1 pie de turno + persistir uso/modelo real.
2. P2 editar / volver a punto (rewind BD transaccional por `rowid`).
3. P3 vault de respaldos + restaurar seguro (la pieza más delicada; fixture obligatorio).
4. P4 ⋯ cabecera + sidebar colapsable/redimensionable.

**039A-3b (independiente, después):**
5. P5 dos conversaciones (panel lateral máx 2, M1).
6. P6 indicador circular de contexto + config ventana 150k.

**Estado**: EN EJECUCIÓN — bloque completo aprobado por el usuario (04-09). P1 (pie de turno +
persistir uso/modelo real) HECHO y verificado en mock; P2 (editar/volver a punto) HECHO y
verificado (unit + type-check + build + flujo mock); resto de fases (P3-P6) en curso.
