# Plan: Glory Harness Desktop — interfaz local de escritorio (Tauri 2 + TypeScript vanilla, backend Rust in-process)

- **Fecha:** 2026-09-03
- **ID:** 039A-1 (libre; el 039A-2 lo usa el plan de mejora del agente en `core/src/diff.rs`)
- **Estado:** F1 hecha (lib compartida, 2026-09-04); F2 cubierta por 318A-16 F2 sin hook nuevo; F4 en curso (src-tauri + adaptador TS real). Detour egui nativo probado y retirado el mismo día por decisión del usuario (UI TS). **04-09 (tarde):** corregidos los 7 hallazgos del primer `tauri dev` (ver `hallazgos-primer-tauri-dev-2026-09-04.md`); pendiente E2E en ventana + gate final del bloque.
- **Ampliación 2026-09-03 (pedido del usuario):** dentro de la app Tauri real TODAS las acciones
  de la UI deben funcionar de verdad: nueva conversación, acciones del agente bien renderizadas,
  selector de modelo real, configuraciones funcionales, cancelar ejecución, modos. Mapa
  acción → backend real, comandos a añadir y bloqueos de integración en el anexo **§10**.
- **Decisiones tomadas con el usuario:** UI **Tauri 2 + TypeScript vanilla/Preact** (sin React); backend **in-process** (enlazar `glory-harness-core` + lib de sesión compartida con el CLI; sin sidecar del daemon).
- **Contexto:** el otro agente trabaja en `plan-mejora-agente-2026-09-03.md` (318A-15, núcleo/comportamiento del agente). Este plan toca core solo con un hook **aditivo** (aprobación); se coordina por gate ff-only para no pisarse.

---

## 0. Resumen ejecutivo (para el usuario)

App de escritorio local (Windows-first) para Glory Harness: chat con streaming en vivo,
tarjetas de tools (start/resultado/diff), botones de **Aprobar/Denegar**, historial
**durable en SQLite**, selector de workspace y de provider/modelo. Un solo proceso Rust
(Tauri) con el núcleo enlazado directamente; el único coste estructural es el webview del
sistema (WebView2): **~100–140 MB working set estimado**, arranque <1 s, cero timers en
background (idle CPU ≈ 0). La alternativa sin webview (egui/iced, ~25–50 MB) queda
documentada en §6 como descartada por velocidad de desarrollo de UI.

Por qué in-process en vez del daemon sidecar: un proceso menos (~10–20 MB menos),
aprobación/cancelación directas sin extender el protocolo NDJSON, y la misma lógica de
sesión que ya usa el CLI (extraída a lib compartida, sin duplicar).

## 1. Contexto verificado en código

- `core/` emite el contrato `AgenteEvento` (H3): `token`, `tool_start`, `tool_result`
  (con diff), `requiere_aprobacion`, `permiso_denegado` (318A-15 F3), `usage`,
  `contexto_detalle`, `error`, `done` — exactamente lo que la UI necesita renderizar.
- `cli/`: `run`, `daemon` (NDJSON loopback, token), `chat` REPL/`--tui` (ratatui). La
  lógica de sesión vive en `chat.rs`/`run.rs` (`construir_harness`, `procesar_turno`,
  `historial_desde_persistencia`, `turno_config_default`, `quitar_prefijo_verbatim`).
- `PersistenciaMemoria` implementa el trait `AgentPersistence` (puerto del núcleo);
  la IA nunca persiste por sí misma (R3).
- **Gap 1 — aprobación sin respuesta:** `requiere_aprobacion` se emite, la tool se omite
  y el modelo recibe "sigue pendiente de aprobación"; **no existe** ningún camino para que
  el usuario apruebe o deniegue (`runtime.rs` ~379-430). Un GUI sin botones de aprobación
  queda cojo en modo `predeterminado`.
- **Gap 2 — sin historial durable:** todo muere con el proceso.
- **Gap 3 — sesión acoplada al binario:** la lógica compartida está en el crate bin `cli`.

## 2. Arquitectura

```text
glory-harness/
  core/      + hook de aprobación aditivo (AprobadorTool en TurnoConfig; None = hoy)
  cli/       binario + lib (src/lib.rs): sesión, construir_harness, persistencia — compartida
  desktop/   crate Tauri `glory-harness-desktop` (miembro del workspace)
    src/sesion.rs             sesión GUI: runtime + aprobador + cancelación
    src/persistencia_sqlite.rs  AgentPersistence sobre rusqlite (WAL)
    src/ipc.rs                comandos/eventos Tauri ↔ webview
    ui/                       Vite + TypeScript vanilla/Preact (sin React)
```

Flujo de datos (un solo proceso):

```text
Rust: sesion.rs ejecuta el turno → AgenteEvento → tauri::Emitter → webview
webview: store zustand vanilla actualiza la UI; tokens se APPENDEN al nodo del mensaje
         (ref directo, sin re-render por token)
Aprobación: runtime pide → sesion.rs genera id → evento con id a la UI → botón →
            invoke("aprobar_tool", {id, ok}) → oneshot → runtime ejecuta o deniega
Cancelar: AbortHandle/drop del receiver (el runtime ya corta al cerrarse el canal)
```

Persistencia: `PersistenciaSqlite` (rusqlite bundled — sin SQLite de sistema, lazy;
WAL, `synchronous=NORMAL`, `cache_size` acotado, `busy_timeout`) implementando el trait
`AgentPersistence` existente. BD en `%APPDATA%/glory-harness/glory-harness.db`.
Tablas: `conversaciones`, `mensajes`, `turnos`, `acciones`, `memoria`, `skills` (base),
`tareas` (mínimo del trait; las de scheduler se dejan no-op correctos si no se usan).

## 3. Presupuesto de RAM y reglas de rendimiento

- Objetivo: **≤ ~140 MB working set** total (WebView2 ~60–100 MB + proceso Rust ~10–20 MB
  + UI ~5 MB). Medir con el Task Manager al cierre de F6 y documentar.
- Regla del área: **cero timers en background**, escaneos bajo demanda, idle CPU ≈ 0.
- UI: bundle < ~60 KB gz, sin librerías de componentes; CSS con tokens propios
  (`ui/src/styles/variables.css`), clases en español camelCase (convención del área).
- Build siempre en `C:\tmp` (`CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness`),
  respetando el límite de 7 GB; nunca compilar dentro del árbol.

## 4. Fases

### Fase 1 — Refactor CLI a lib+bin (sin cambio de comportamiento)

- [ ] `cli/src/lib.rs` público: `OpcionesRun`, `construir_harness`, `turno_config_default`,
      `quitar_prefijo_verbatim`, `historial_desde_persistencia`, `procesar_turno`,
      `TurnoResultado`, `PersistenciaMemoria`, `cargar_env_usuario`.
- [ ] `main.rs` queda fino (dispatch); `daemon.rs`/`chat.rs`/`tui.rs` siguen como módulos.
- [ ] Tests: los existentes pasan sin cambios; 49/49 del workspace verde.

### Fase 2 — Aprobación real en core (aditivo, coordinado con 318A-15)

- [ ] `TurnoConfig.aprobador: Option<Arc<dyn AprobadorTool>>` (`None` = comportamiento actual).
- [ ] `trait AprobadorTool { async fn decidir(&self, tool, argumentos) -> VerdictoAprobacion }`
      con `VerdictoAprobacion::{Aprobar, Denegar}`.
- [ ] En el bucle: política `ask` → emitir `RequiereAprobacion` → `await aprobador` →
      `Aprobar`: ejecutar la tool · `Denegar`: `PermisoDenegado { motivo: "denegada_por_usuario" }`
      + mensaje al modelo (sin reintento, ya implementado en F3 de 318A-15).
- [ ] Timeout configurable (default 10 min) → al expirar se omite la tool (como hoy).
- [ ] Tests: aprobar ejecuta; denegar emite `PermisoDenegado` y no reintenta; `None` = hoy.

### Fase 3 — PersistenciaSqlite

- [ ] `cli/src/persistencia_sqlite.rs` implementando todo `AgentPersistence` (rusqlite
      bundled; WAL; `PRAGMA cache_size=-2000`; `busy_timeout=5000`; `synchronous=NORMAL`).
- [ ] `conversaciones` con título/fecha para la lista lateral; `mensajes` ordenados por
      `creado_en`; skills base por usuario (paridad `con_skills_base`).
- [ ] **CRUD de conversaciones**: el trait `AgentPersistence` (`core/src/ports.rs`) YA tiene
      `listar_mensajes`/`conversacion_tocar`; comprobar si necesita métodos para crear/listar/
      renombrar/archivar/eliminar conversaciones (la lista lateral sale de la BD, no del mock).
- [ ] **Persistir el mensaje de usuario en `enviar_turno`** (hueco B6): el runtime del núcleo
      guarda SOLO la respuesta del asistente (`guardar_mensaje` rol `assistant` en
      `core/src/runtime.rs` ~776). El mensaje del usuario NO se persiste por el núcleo: el
      consumidor lo guarda (el REPL del CLI no persiste mensajes de usuario a propósito).
      La app Tauri DEBE guardar el mensaje de usuario en `enviar_turno` (vía puerto) antes de
      ejecutar; sin esto el historial de la conversación queda sin la pregunta del usuario y
      `listar_mensajes` no reconstruye el hilo al recargar.
- [ ] **Estado `cancelado`** (hueco B6): `cancelar_turno` hoy hace `handle.abort()` (aborta la
      tarea) SIN emitir `turno-fin` ni `finalizar_turno`; el `TurnoPersistido.estado` queda
      `"ejecutando"` en la BD. Añadir `finalizar_turno(turno_id, "cancelado", …)` en el abort.
- [ ] Tests: guardar/listar ordenado; reabrir la BD conserva el historial; idempotencia.

### Fase 4 — App Tauri + sesión GUI

- [ ] Crates `desktop/` (tauri 2, ventana única, `withGlobalTauri` mínimo).
- [ ] Comandos IPC **reconciliados con el backend ya commiteado** (`desktop/src-tauri/src/main.rs`):
      implementados → `abrir_sesion`, `enviar_turno`, `cancelar_turno`, `responder_aprobacion`,
      `pendientes_aprobacion`.
- [ ] Comandos IPC **a añadir** para cubrir el alcance del usuario (anexo §10): `conversacion_nueva`,
      `listar_conversaciones`, `cargar_conversacion`, `renombrar_conversacion`,
      `archivar_conversacion`, `eliminar_conversacion`, `proveedores_disponibles`, `config_leer`,
      `config_guardar`, `elegir_workspace` (picker nativo).
- [ ] Eventos Tauri: re-emisión de `AgenteEvento` + `turno-fin` (ya emitidos por `enviar_turno`);
      los ids de conversación se resuelven en Rust (persistencia), nunca se fabrican en la UI.
- [ ] Runtime tokio acotado (`worker_threads ≤ 2`); cancelar = AbortHandle + drop receiver.

### Fase 5 — UI (Vite + TS vanilla/Preact)

- [ ] Sidebar de conversaciones + vista de chat (streaming con append directo de tokens).
- [ ] Tarjetas de tools: start/resultado con diff colapsable; errores; barra de contexto
      (desglose `ContextoDetalle`); tarjeta de aprobación con Aprobar/Denegar.
- [ ] Selector de workspace (picker nativo vía Tauri), selector provider/modelo,
      indicador de modo (`predeterminado`/`autonomo`), botón cancelar por turno.
- [ ] Rendimiento: append por mensaje (ref), virtualización de mensajes largos (window
      simple), `content-visibility: auto`, sin timers, bundle < 60 KB gz.
- [ ] **Cableado total a backend real:** cada acción de la UI dispara un comando real y no
      muta solo estado local (mapa acción → backend en el anexo §10).

### Fase 6 — Empaquetado, medición y gate

- [ ] `tauri build` (NSIS/MSI Windows), iconos, `install-release.bat` al estilo del área.
- [ ] Medir y documentar RAM real (working set) y startup; ajustar si > ~140 MB.
- [ ] Gate: `cargo test --workspace`, `npm run quality:doctor`, `quality:analyze`,
      `tsc --noEmit` + build UI. Evidencia en `Agente/completados/`.

## 5. Criterios de aceptación

- [ ] La app abre en <1 s; RAM medida ≤ ~140 MB; idle CPU ≈ 0 (sin timers).
- [ ] E2E real (o fixture): turno con streaming, tools con tarjetas, aprobar → la tool se
      ejecuta, denegar → `PermisoDenegado` y el modelo cambia de plan, cancelar detiene el
      turno, historial intacto tras reiniciar la app.
- [ ] CLI sin regresión (49/49 + gate PASS); contrato SSE intacto; cambios en core solo
      aditivos (`None` = comportamiento actual).
- [ ] Conversación de la app y del CLI son intercambiables en persistencia (misma BD/trait).

## 6. Alternativa documentada (descartada en v1)

**Native Rust UI (egui/iced):** mismo backend in-process; ~25–50 MB total, arranque
~0.3 s. Descartada porque la velocidad de iteración de la UI de chat (markdown, diffs,
scroll, theming) en React/TS es muy superior, y el webview domina el coste en cualquier
caso. Si la RAM medida supera el objetivo y no se puede recortar, reabrir esta opción.

## 7. Riesgos y mitigaciones

- **WebView2 ausente/antiguo en Windows:** el runtime de Tauri lo instala; documentar en
  el README de la app.
- **Conflicto con 318A-15 en `core/runtime.rs`:** nuestro cambio es aditivo y pequeño;
  commits por fase, gate antes de integrar, ff-only (regla del área).
- **Compilación de SQLite bundled:** tiempo de build extra; aceptable (build en `C:\tmp`).
- **RAM del webview variable:** medir en F6; mitigaciones: in-process ya elegido, nada de
  timers, bundle mínimo; si aún excede → reabrir §6.

## 8. No alcance (v1)

Tray/notificaciones, multi-ventana, MCP/LSP, navegador headless, tool `bash`,
edición visual de archivos, auto-update. Todo ello puede ser plan posterior.

## 9. Checklist resumen

| Fase | Contenido | Estado |
|---|---|---|
| F1 | Refactor CLI → lib+bin (sesión compartida) | ☑ hecha 2026-09-04 (183/183 tests, clippy limpio) |
| F2 | Aprobación real en core (aditivo) | ☑ cubierta por canal 318A-16 F2 (sin hook nuevo) |
| F3 | PersistenciaSqlite (historial durable) | ☐ 0/3 |
| F4 | App Tauri + sesión GUI + IPC | ◐ en curso (backend real: abrir_sesion/enviar_turno/cancelar_turno/responder_aprobacion/pendientes_aprobacion; comandos a añadir en §10) |
| F5 | UI Vite + TS vanilla/Preact | ◐ en curso por el usuario (adaptador real cableado en main.ts; ampliación: cablear TODAS las acciones al backend real — §10) |
| F6 | Empaquetado, medición RAM y gate | ☐ 0/3 |

---

## 10. Acciones de la UI conectadas al backend real (ampliación 2026-09-03)

Alcance pedido por el usuario: dentro de la app Tauri real, cada acción visible tiene que
funcionar de verdad — nueva conversación, acciones del agente bien renderizadas, selector de
modelo real, configuraciones funcionales, cancelar ejecución, modos y acciones varias.

### 10.1 Mapa acción UI → backend real

Leyenda: ✅ ya cableado · ◐ parcial · ⛔ sin backend (hay que añadirlo).

| Acción de la UI | Componente / callback | Backend real hoy | Estado | Trabajo pendiente |
|---|---|---|---|---|
| Enviar mensaje | `entrada.onEnviar` → `adaptador.montar` | `enviar_turno(mensaje)` + eventos `agente-evento`/`turno-fin` | ✅ | Verificación E2E real (primer `tauri dev`) |
| Aprobar / Denegar / Permitir | tarjeta de aprobación → `responder_aprobacion` | `responder_aprobacion` + `pendientes_aprobacion` | ✅ | Verificación E2E |
| Cancelar ejecución | botón detener → `adaptador.detener()` | `cancelar_turno()` (aborta el turno) | ✅ | Verificación E2E; botón enviar/detener fiel |
| Acciones del agente (tools) visibles | `real.ts aplicar(ev)` (tool_start/tool_result/permiso_denegado…) | eventos reales | ◐ | Verificar que CADA evento del contrato renderiza (fixture E2E por tipo) |
| **Nueva conversación** (nav sidebar) | `botonNav('Nueva conversación')` — hoy sin handler | no existe | ⛔ | `conversacion_nueva` → limpia `#mensajes`, cabecera «Nueva conversación», la añade arriba y la selecciona |
| **Agentes / Flujo / Complementos** (nav sidebar) | `botonNav(...)` + `onAccionNav` | no existe (fuera de v1) | ✅ (aviso) | Muestran «próximamente» en el chat (§10.4); el panel real queda para un alcance futuro con backend |
| Listado de conversaciones | sidebar lee `datos/conversaciones.ts` (mock) | no existe | ⛔ | `listar_conversaciones` (activas + archivadas) y repintar al arrancar |
| Seleccionar conversación | `sidebar.onSeleccionar(id)` (solo cambia título) | no existe | ⛔ | `cargar_conversacion {id}` → historial en `#mensajes` + título de cabecera |
| Renombrar (menú ⋯) | `onRenombrar(id, titulo)` local (`TODO(backend)`) | no existe | ⛔ | `renombrar_conversacion {id, titulo}` |
| Archivar / Desarchivar | `onArchivar(id, archivada)` local | no existe | ⛔ | `archivar_conversacion {id, archivada}` |
| Eliminar | `onEliminar(id)` local | no existe | ⛔ | `eliminar_conversacion {id}` (+ confirmación) |
| Copiar ID | menú → portapapeles (navigator) | no aplica | ✅ | — |
| Selector de modelo **real** | catálogo estático `PROVEEDORES`/`MODELO_INICIAL` | `abrir_sesion` devuelve solo conteos por proveedor | ⛔ | `proveedores_disponibles` (allowlist real de `core/src/llm.rs`) + presiembra del modelo de sesión; barra y modal comparten |
| Modos predeterminado/meta/autónomo | selector local; se pasa en `montar({modo})` | `abrir_sesion(modo)` / `OpcionesRun` | ◐ | Aplicar el modo elegido al abrir/reabrir la sesión real; indicador fiel |
| Razonamiento low/medium/high | local `razonamientoActual`; no viaja al runtime | no se pasa | ⛔ | Incluirlo en la config de sesión/turno y persistirlo |
| Configuraciones del modal (persistir) | `modal.onCambio` → `// TODO(backend): persistir` | no existe | ⛔ | `config_leer`/`config_guardar` (razonamiento, ejecución, permisos, contexto) |
| Workspace (picker) | campo texto estático | no existe | ⛔ | `elegir_workspace` (diálogo nativo) + reabrir sesión |
| Opciones de ejecución/permisos (maxTurns, timeout, temp, maxTokens…) | solo UI | no aplican al runtime | ⛔ | Mapearlas a `OpcionesRun`/`TurnoConfig` y persistirlas; solo las que el core soporte de verdad |

### 10.2 Bloqueos de integración detectados (verificados contra Tauri 2)

- **B1 — Sin capabilities:** `desktop/src-tauri/capabilities/` no existe y
  `gen/schemas/capabilities.json` = `{}`. Los comandos propios de la app funcionan por
defecto, pero `listen`/eventos (`core:event:listen`) exigen permiso. **Fix:** crear
  `desktop/src-tauri/capabilities/default.json` con `"windows": ["main"]` y
  `"permissions": ["core:default"]`. Sin esto la ventana abre pero la UI no recibe
  tokens/tools/`turno-fin`.
- **B2 — Detección de entorno:** `esEntornoTauri()` usa `window.__TAURI__` y `tauri.conf.json`
  NO tiene `withGlobalTauri`. Con el bundler ESM (`@tauri-apps/api`) la app funciona sin el
global, pero `__TAURI__` puede no existir → `USA_REAL=false` dentro de la app real. **Fix
  (verificar en el primer `tauri dev`):** añadir `"withGlobalTauri": true` o detectar por otra
  vía (p. ej. invocación disponible / protocolo).
- **B3 — Nombres de comandos divergentes:** el plan (F4) listaba `sesion_nueva/sesion_cargar/
  enviar_mensaje/aprobar_tool` pero el backend commiteado usa `abrir_sesion/enviar_turno/
  cancelar_turno/responder_aprobacion/pendientes_aprobacion`. Este anexo reconcilia: no duplicar
  comandos; donde el plan decía un nombre, rige el backend real.
- **B4 — Persistencia en memoria:** hoy `PersistenciaMemoria` (todo muere con el proceso). La
  durabilidad de conversaciones (crear/listar/renombrar/archivar/eliminar + historial por
  conversación) requiere **F3 `PersistenciaSqlite`**; sin ella, «nueva conversación» no
  sobrevive a un reinicio. Orden recomendado: F3 → comandos de conversación → cablear UI →
  primer `tauri dev`.
- **B5 — La sesión Tauri NO usa `PersistenciaSqlite`:** `desktop/src-tauri/src/main.rs`
  construye la sesión con `construir_harness` (que fija `PersistenciaMemoria::nuevo()` dentro
  de `cli/src/run.rs`). La app necesita una sesión con `PersistenciaSqlite` (o reemplazar la
  persistencia tras abrir sesión); `construir_harness` no deja inyectar persistencia, así que
  hay que añadir un constructor de sesión con persistencia inyectable o sustituirla en
  `abrir_sesion`.
- **B6 — Mensaje de usuario y estado `cancelado` no persisten:** ver huecos en F3 (el runtime
  solo guarda la respuesta del asistente; `cancelar_turno` aborta sin `finalizar_turno`).

### 10.3 Criterio de cierre de la ampliación («todo funciona de verdad»)

- [ ] Primer `tauri dev` (debug) con capabilities + detección OK: enviar → streaming de tokens →
      tarjetas de tools → aprobación → `turno-fin`, todo visible.
- [ ] «Nueva conversación» crea una conversación real (persistida) y limpia la vista.
- [ ] El listado/sidebar sale del backend (no del mock), con renombrar/archivar/eliminar persistidos.
- [ ] El selector de modelo muestra proveedores/modelos reales y el cambio abre la sesión con ese modelo.
- [ ] Cancelar detiene el turno real y el botón vuelve a «enviar».
- [ ] Modos y razonamiento viajan en la sesión/turno y se reflejan fieles en barra y modal.
- [ ] Las configuraciones se guardan/leen (sin `TODO(backend)`).
- [ ] `tsc --noEmit` + build UI + `cargo test --workspace` + gate PASS.

> **Cierre parcial (bloque 039A-1, 04-09, correcciones del primer `tauri dev`):** este bloque
> cerró los 7 hallazgos del primer arranque real (documentados en
> `Agente/documentacion/hallazgos-primer-tauri-dev-2026-09-04.md`, sección «Resumen de
> correcciones»). Quedó cubierto: «Nueva conversación»/listado/renombrar/archivar/eliminar
> reales, historial persistido repintado con herramientas al recargar (H6), modelo real con
> sesión reutilizada (H4), modos y razonamiento viajando en sesión/turno y persistidos (H7),
> config leída/guardada, cancelar real, workspace real (H3), título auto-generado (H5),
> panel meta visible solo cuando aplica (H1) y scroll del chat al enviar (H2). Pendiente de
> este criterio: E2E en la ventana Tauri real (enviar → streaming → tools → aprobación →
> `turno-fin` visible) y el gate final del bloque.

### 10.4 Botones «Agentes», «Flujo», «Complementos» → «próximamente» (hecho, 03-09)

Los tres botones del nav superior del sidebar no tenían ninguna acción. Pedido del usuario:
que dijeran «próximamente» en lugar de no hacer nada.

- **UI (hecho, sin backend):** `sidebar.ts` gana el callback `onAccionNav: (accion:
  'agente'|'flujo'|'complementos')` (el botón «Nueva conversación» NO lo usa: sigue
  reservado para la acción real con backend). `botonNav(...)` acepta un `onClick`.
  `main.ts` lo consume: añade un `crearAvisoSistema('X: próximamente', 'sin backend',
  'X no está disponible en esta versión.')` a `#mensajes` y hace scroll al final.
- **Sin CSS nuevo** (reutiliza `.aviso-sistema` existente de `mensajes.css`).
- **Verificado en navegador:** los 3 botones muestran su aviso colapsable al pulsar;
  type-check + build PASS (23 modules, js 30.36 kB).
- **Pendiente de futuro (cuando exista backend/alcance):** sustituir el aviso por los
  paneles reales de Agentes / Flujo / Complementos. No hay backend previsto para ellos
  en v1 (ver §8 No alcance).

### 10.5 Panel «modo meta» en ejecución: boceto temporal + implementación real (03-09)

Pedido del usuario: «ver cómo se verá el modo meta corriendo» — un cuadro que va encima de
la caja de entrada, con la meta, editable a mitad de proceso, tiempo de ejecución, tokens y
botón play/pausa. Primero se pidió como **boceto sobre la UI real** (no HTML suelto); al
validarse el aspecto, se implementa de verdad.

#### 10.5.1 Boceto TEMPORAL sobre la UI real (hecho, 03-09)

Para ver el aspecto se montó un boceto simulado sobre la UI real, claramente marcado para
retirarlo al implementar el panel real:

- **Archivos (toda la superficie de retirada):**
  - `desktop/ui/src/componentes/panelMetaBoceto.ts` — exporta `montarPanelMetaBoceto(): PanelMetaBoceto`
    (`{ raiz, medir() }`). Marcado `data-boceto="meta-2026-09-03"` y cabecera «TEMPORAL».
  - `desktop/ui/src/estilos/panelMetaBoceto.css` — estilos del boceto.
  - `desktop/ui/src/estilos/index.css` — import `@import './panelMetaBoceto.css';` (último, con comentario).
  - `desktop/ui/src/main.ts` — monta `montarPanelMetaBoceto()` **dentro de `#entrada`**, antes de
    `.caja` (heredando así el mismo ancho que la caja); llama a `panelMeta.medir()` tras el montaje.
- **Diseño validado (una sola línea):** campo de la meta a la izquierda (textarea sin borde que
  colapsa a 1 línea y crece a 2-3 al enfocar) + a la derecha `estado · tiempo · tokens` + botón
  negro play/pausa. **Sin notas** explicativas. Ancho idéntico a la caja de abajo.
- **Demo local:** timer que simula `corriendo/pausado`, reloj `mm:ss` y contador de tokens; el
  play/pausa alterna y congela el avance. NO está cableado a backend.
- **Verificado:** type-check + build PASS (24 modules, js 32.96 kB).
- **Retirada:** al implementar el panel real (10.5.2) se borran los archivos de arriba y el import
  de `index.css`; se sustituye por el componente real montado en el mismo sitio (dentro de
  `#entrada`, antes de `.caja`) o donde decida F5.

#### 10.5.2 Implementación real del panel «modo meta» (backend nuevo)

Para que el panel funcione de verdad hace falta backend que hoy **no existe**. Se separa por
capas; todo lo nuevo va marcado como trabajo del Bloque A / ampliación §10.

**UI (componente real `panelMeta`):**

- [ ] Componente real (no boceto) `desktop/ui/src/componentes/panelMeta.ts` + `estilos/panelMeta.css`:
      misma estructura de una línea (meta + estado · tiempo · tokens · play/pausa) validada en
      10.5.1. Se monta **solo cuando el modo activo es `meta`** (misma mecánica que el selector
      de modo); oculta al cambiar a otro modo.
- [ ] **La «meta» NO existe aún en el modelo**: el `TurnoConfig`/`OpcionesRun` del núcleo no
      tienen campo de objetivo/meta; `construir_harness` solo mapea `provider/modelo/modo`. La
      UI ni siquiera expone el modo `plan` (sus modos son `predeterminado|meta|autonomo`, ver
      `entrada.ts`). Definir qué es «la meta del turno»: ¿la meta inicial que el usuario escribe
      (equivale al mensaje del turno) o un objetivo persistente de la conversación (bloque en el
      prompt del sistema, como `[REGLAS]`/`[ENTORNO]`)? Decidir y añadir el campo a `TurnoConfig`
      (o inyectarlo en el `prompt_sistema`/`preferencias`) ANTES de hablar de `actualizar_meta`.
- [ ] El campo de la meta es **editable a mitad de proceso**: al enviar la meta inicial viaja con
      el turno; editar en caliente → comando `actualizar_meta` (ver backend) y el cambio se aplica
      al siguiente paso.
- [ ] Tiempo de ejecución del turno en curso (`mm:ss`): el núcleo persiste `duracion_ms` en
      `TurnoPersistido` solo al FINAL del turno (no se emite en vivo); la UI hoy no recibe
      ningún evento de duración. Marcar `Instant` inicio del turno en `enviar_turno` y emitir
      (o exponer) la duración en vivo (nuevo evento/estado) o calcular en UI desde `turno-fin`.
- [ ] Tokens: el contrato `AgenteEvento.Usage` expone `tokens_prompt` y `tokens_complecion`
      (ver `core/src/evento.rs`) pero el adaptador `real.ts` solo lee `ocupacion_pct` (no suma
      tokens). El núcleo persiste `tokens_prompt`/`tokens_complecion` en `TurnoPersistido` pero
      NO los emite en vivo como evento agregable. Definir contador del turno: sumar
      `tokens_prompt + tokens_complecion` por `Usage`, o enviar el acumulado desde Rust.
- [ ] Play/pausa real (ver backend). Mientras esté pausado el contador de tiempo se congela.

**Backend (Tauri `desktop/src-tauri` + core según aplique):**

- [ ] **`actualizar_meta`** — comando nuevo: cambia la meta de la sesión/turno **en marcha**
      (p. ej. un `Arc<Mutex<MetaActual>>` compartido que el runtime lee al iniciar cada paso).
      La meta viaja en `OpcionesRun`/`TurnoConfig` al abrir sesión; la edición en caliente solo
      actualiza ese estado.
- [ ] **Pausa/reanudación real** — hoy el backend **solo tiene `cancelar_turno`** (aborta). Para
      play/pausa real hay que decidir y construir backend: o bien pausar = detener el stream con
      retomar (requiere soporte en el runtime del turno, trabajo no trivial), o bien pausar queda
      redefinido en v1 como «detener el turno y reenviar» (reutilizando cancelar + nuevo turno con
      la misma meta). **Decisión pendiente** con el usuario antes de implementar; en el boceto el
      play/pausa es solo visual.
- [ ] Duración del turno: emitir (o calcular en UI desde `turno-fin`) el instante de inicio/fin del
      turno para el reloj.
- [ ] Contador de tokens fiable: verificar qué campos trae `AgenteEvento.Usage` en el contrato real
      y sumarlos de forma consistente (no doble contaje en reintentos).

**Criterio de cierre (10.5):**

- [ ] Con modo `meta` activo y turno real corriendo, el panel muestra la meta (editable), el tiempo
      avanza solo durante el turno, los tokens crecen con cada evento y el play/pausa funciona sobre
      el turno real (según la decisión de pausa adoptada).
- [ ] Retirado el boceto (10.5.1): sin `panelMetaBoceto.*`, sin `data-boceto`, sin import temporal.
- [ ] `tsc --noEmit` + build UI + `cargo test --workspace` + gate PASS.

> **Decisión de alcance:** el panel real es trabajo del Bloque A (requiere B1/B2 y comandos
> backend). Hasta que exista `actualizar_meta` y la política de pausa, el boceto solo sirve para
> validar aspecto; no se integra como si fuera el panel real.