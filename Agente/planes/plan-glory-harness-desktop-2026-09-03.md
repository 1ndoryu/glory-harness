# Plan: Glory Harness Desktop — interfaz local de escritorio (Tauri 2 + TypeScript vanilla, backend Rust in-process)

- **Fecha:** 2026-09-03
- **ID:** 039A-1 (libre; el 039A-2 lo usa el plan de mejora del agente en `core/src/diff.rs`)
- **Estado:** F1 hecha (lib compartida, 2026-09-04); F2 cubierta por 318A-16 F2 sin hook nuevo; F4 en curso (src-tauri + adaptador TS real). Detour egui nativo probado y retirado el mismo día por decisión del usuario (UI TS).
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
- [ ] Tests: guardar/listar ordenado; reabrir la BD conserva el historial; idempotencia.

### Fase 4 — App Tauri + sesión GUI

- [ ] Crates `desktop/` (tauri 2, ventana única, `withGlobalTauri` mínimo).
- [ ] Comandos IPC: `sesion_nueva`, `sesion_cargar`, `enviar_mensaje`, `cancelar_turno`,
      `aprobar_tool`, `listar_conversaciones`, `elegir_workspace`, `config_guardar/leer`.
- [ ] Eventos Tauri: re-emisión de `AgenteEvento` + `aprobacion_pendiente { id, tool, args }`.
- [ ] Runtime tokio acotado (`worker_threads ≤ 2`); cancelar = AbortHandle + drop receiver.

### Fase 5 — UI (Vite + TS vanilla/Preact)

- [ ] Sidebar de conversaciones + vista de chat (streaming con append directo de tokens).
- [ ] Tarjetas de tools: start/resultado con diff colapsable; errores; barra de contexto
      (desglose `ContextoDetalle`); tarjeta de aprobación con Aprobar/Denegar.
- [ ] Selector de workspace (picker nativo vía Tauri), selector provider/modelo,
      indicador de modo (`predeterminado`/`autonomo`), botón cancelar por turno.
- [ ] Rendimiento: append por mensaje (ref), virtualización de mensajes largos (window
      simple), `content-visibility: auto`, sin timers, bundle < 60 KB gz.

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
| F4 | App Tauri + sesión GUI + IPC | ◐ en curso (src-tauri in-process + adaptador real.ts, turno real verificado vía CLI) |
| F5 | UI Vite + TS vanilla/Preact | ◐ en curso por el usuario (adaptador real cableado en main.ts; mock solo con VITE_MOCK=1) |
| F6 | Empaquetado, medición RAM y gate | ☐ 0/3 |