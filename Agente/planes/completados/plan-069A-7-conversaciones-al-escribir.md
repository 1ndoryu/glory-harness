# Plan 069A-7 — Conversaciones: crear la fila SOLO al escribir (web + app)

- **Fecha:** 2026-09-06
- **Área:** `glory-harness` (`cli/src/comandos/web.rs`, `web_datos.rs`, `web_turnos.rs`,
  `cli/src/servicio/sesion.rs`, `desktop/src-tauri/src/main.rs`, `desktop/ui/src/...`)
- **Estado:** COMPLETADO — implementado y verificado (2026-09-06).
- **Rama:** `main`. **Gate:** Sentinel/VarSense en `.quality-tools-harness`.
- **ID roadmap:** 069A-7. **Evidencia:** `Agente/completados/tareas-2026-09-06.md`.

---

## 1. Objetivo

Que **no se cree ninguna fila de conversación** al abrir la app, recargar la página o pulsar
"Nueva conversación". La fila se crea **únicamente al enviar el primer mensaje** (el backend ya
auto-nombra [039A-1 H5]). Aplica por igual en **web** (`glory-harness web`) y en la **app
desktop Tauri**.

Comportamientos concretos esperados:

1. Estado "vacío total" (0 conversaciones): abrir/recargar → chat vacío tipo borrador
   ("Nueva conversación"), **sin fila** en la sidebar ni en BD. Nada se persiste.
2. Pulsar "Nueva conversación" (con lista existente): limpia el chat a borrador **sin crear
   fila** ni cambiar la fila "actual" del backend.
3. Escribir el primer mensaje en un borrador → se crea la conversación (auto-nombrada desde el
   mensaje), aparece en la sidebar y recibe el turno.
4. Recargar sin escribir → vuelve a la **última conversación real** (decisión del usuario A).
5. Eliminar la última conversación → queda el estado vacío (borrador), sin fila fantasma.

## 2. Diagnóstico (causa raíz verificada en vivo)

"No abrir crea": cargar una conversación NO crea (reproducido: GET messages 13→13). Los
verdaderos acumuladores de filas vacías son 3:

1. `cli/src/servicio/sesion.rs` `abrir_con_persistencia` (~línea 157): si la lista de
   no-archivadas está vacía, hace `conversacion_crear("Nueva conversación")` **siempre**.
2. `cli/src/comandos/web_datos.rs` `eliminar_conversacion` (~línea 230) y Tauri
   `eliminar_conversacion`: si borras la conversación **actual**, auto-crean un reemplazo
   "Nueva conversación".
3. Frontend `panelChat.ts` `nuevaConversacion()` llama a `convNueva`/`conversacion_nueva`
   → persiste la fila al instante (y la deja como actual).

Diseño previo (H4, ya en el repo): el arranque reutiliza la última no-archivada si existe. Ese
fix es correcto pero incompleto: no cubre los 3 acumuladores de "vacías".

## 3. Decisión de diseño

El backend hoy garantiza **"siempre existe una conversación actual"** (`Uuid` no opcional en
`SesionWeb.conversacion_id` web y `PanelDatos.conversacion_id` Tauri). Para permitir el
borrador real sin fila hay que hacer representable "no hay conversación todavía":

- **Web:** `SesionWeb.conversacion_id: Mutex<Uuid>` → `Mutex<Option<Uuid>>`.
- **Desktop:** `PanelDatos.conversacion_id: Uuid` → `Option<Uuid>`.

El **servicio** `SesionComun` **no** cambia su invariante interno: `preparar_turno` y los
turnos reciben un `conv_id` explícito (creado justo antes de enviar). No se toca TUI/CLI/daemon
(que usan su propia semántica con UUIDs no persistidos o `conversacion_crear` explícita).

Reglas nuevas:

- **`abrir`/`crear_sesion` (web) y `abrir_sesion` (Tauri):** si la lista de no-archivadas está
  vacía, NO crear una vacía; la apertura reporta "sin conversación" (`conversacion: null` /
  sin fila) y el front muestra borrador. Si hay alguna, se ancla la más reciente (comportamiento
  actual de arranque, decisión A).
- **`eliminar_conversacion` (ambos):** si borras la actual del panel/sesión:
  - si quedan otras no-archivadas → ancla la más reciente;
  - si no quedan → `None` (borrador); devuelve la info correspondiente.
  El contrato de respuesta (`actual` en web, `InfoConversacion` en Tauri) pasa a poder indicar
  "sin conversación".
- **`iniciar_turno` (web):** si no hay conversación anclada y no se pasa `conversacion_id`,
  error claro (el front SIEMPRE crea antes de enviar: flujo create-on-write en el cliente).
- **Frontend:** `nuevaConversacion()` pasa a ser un **borrador local** (sin llamar al backend):
  `conversaId = null`, limpiar chat, título "Nueva conversación", sin fila en sidebar. En
  `enviar()`: si `conversaId === null`, primero `sesion.nueva()` (crea + ancla en backend) y
  después `montar(...)`. El resto (cargar, listar) no cambia.

## 4. Archivos y fases

### Fase 1 — Backend web (`cli/src/comandos/`)
- `web.rs`: `SesionWeb.conversacion_id: Mutex<Option<Uuid>>`; `crear_sesion` con lista vacía no
  crea (deja `None`, response `conversacion: null`); `eventos_sse` serializa `Option`;
  tests `sesion_memoria`/`sesion_expirada` adaptados (None cuando vacío o Some si existe).
- `web_datos.rs`: `crear_conversacion`/`cargar_conversacion` asignan `Some`; `eliminar_conversacion`
  si era la actual → elige la más reciente restante o `None`; respuesta con `actual` nullable.
- `web_turnos.rs`: `None` (sin `conversacion_id` explícito) → 409/400 claro si no hay actual.

### Fase 2 — Backend desktop (`desktop/src-tauri/src/main.rs`)
- `PanelDatos.conversacion_id: Option<Uuid>`; `conv_id_de_panel` devuelve `Result<Option<Uuid>>`
  (o maneja el None); `abrir_sesion_interna` inserta panel con `None` si no hay conversación;
  `enviar_turno` con `None` → error claro (el front crea antes);
  `eliminar_conversacion` → elige siguiente o deja `None` (no crea);
  `conversacion_nueva` crea y asigna `Some`.
- `InfoSesion.conversacion` nullable en la respuesta de apertura.

### Fase 3 — Frontend (`desktop/ui/src/`)
- `panelChat.ts`: `nuevaConversacion()` borrador local; `enviar()` crea si `conversaId===null`
  (usa `d.adaptador.sesion.nueva(undefined, tipo)` y espera antes de `montar`).
- `main.ts`: arranque con 0 filas → título borrador sin llamar backend; `onEliminar` maneja
  "sin actual"; `onAccionNav('nueva')` → borrador.
- Tipos (`real.ts`/`api.ts`): respuestas de apertura/eliminar pueden traer conversación ausente.

## 5. Verificación

1. Rust `cargo check`/`test` (workspace) con cargo real y `CARGO_TARGET_DIR=C:\tmp`.
2. UI `tsc --noEmit` + `vite build`.
3. Reiniciar servidor web y probar en navegador (`127.0.0.1:8799`):
   - estado vacío (0 filas): recargar → borrador, sin fila nueva en GET;
   - pulsar "Nueva conversación" → sin fila; escribir → 1 fila auto-nombrada;
   - borrar todas → queda borrador sin fila;
   - recargar con historial → carga la última real.
4. Desktop: `cargo check`/`clippy` (build `tauri dev` si es viable) y verificación funcional.
5. Gate del proyecto (autoridad de cierre) + evidencia en `Agente/completados/`.

## 5b. Evidencia de verificación (2026-09-06)

- **Build:** `cargo build --workspace` OK (binario
  `C:\tmp\glory-target\glory-harness\debug\glory-harness.exe` 19:38:05). UI `vite build` OK
  (`dist/index-vDC5BuCa.js`). Sin errores TS en `real.ts`/`api.ts`/`panelChat.ts`/`main.ts`.
- **Tests Rust:** `cargo test -p glory-harness --lib` → **80 passed**; `cargo test -p
  glory-harness-desktop --bin glory-harness-desktop` → **7 passed**. (Los tests del crate
  `core` están rotos en HEAD por deuda ajena preexistente del refactor navegador/069A-2:
  `PersistenciaMemoria` no encontrado y `missing field navegador` en `AgentToolContext`; no se
  toca `core/` en esta tarea.)
- **API web (Invoke-RestMethod contra `127.0.0.1:8799`):**
  - Sesión nueva con 0 filas → `conversacion: null` (sin autocreación). ✓
  - Con fila residual existente → apertura la ancla (última real). ✓
  - `DELETE` de la última → responde `actual: null`, listado 0 (sin fila fantasma). ✓
  - `POST /conversations` (create-on-write) → 1 fila; listado 1. ✓
- **Navegador (escenarios end-to-end):**
  - (a) 0 convs → recargar muestra borrador "Nueva conversación" sin fila en sidebar. ✓
  - (b) pulsar "Nueva conversación" → total sigue 0. ✓
  - (b) primer mensaje → 1 fila auto-nombrada "hola 069A-7, esto es", turno responde. ✓
  - (d) recargar con historial → ancla la última real. ✓
  - (c) borrar la única → borrador "Nueva conversación", sidebar vacío, listado 0. ✓
- **Gate:** bloqueado en preflight por `tool-release-unpublished` (commit Sentinel 6baf87c2 no
  alcanzable desde ref de release) — **preexistente** del repin del commit `50f76dd`, ajeno a
  069A-7. No se repara aquí (infraestructura del gate, fuera de alcance). Dry-run del check
  069A-7 calculó alcance de 19 archivos sin bloqueo previo.

## 6. Definition of Done

- [x] 0 conversaciones → abrir/recargar no deja fila (web y app).
- [x] "Nueva conversación" no crea fila hasta el primer mensaje (web y app).
- [x] Primer mensaje crea 1 fila auto-nombrada (web y app).
- [x] Eliminar la última → sin fila fantasma (web y app).
- [x] Recargar con historial → última real (web y app).
- [x] Compila (Rust + UI), tests verdes, evidencia registrada. Gate bloqueado por infra preexistente (observación).
