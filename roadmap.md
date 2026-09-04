# Roadmap — glory-harness

> Proyecto: Glory Harness (Rust workspace: `core` + `cli` + `desktop/src-tauri`; UI Tauri 2 +
> TypeScript vanilla en `desktop/ui`). Rama primaria: `main`. Gate: Sentinel/VarSense en
> `.quality-tools-harness` (autoridad de cierre). Convención documental del área: planes en
> `Agente/planes/`, evidencia en `Agente/completados/`, documentación en `Agente/documentacion/`.

## Contexto y fuentes

- **Plan maestro de escritorio (UI real + backend Tauri):** `Agente/planes/plan-glory-harness-desktop-2026-09-03.md`
  (ID **039A-1**). Contiene fases F1–F6 y el anexo **§10**: mapa de cada acción de la UI →
  backend real (nueva conversación, acciones del agente, selector de modelo real,
  configuraciones, cancelar ejecución, modos) + bloqueos de integración B1–B4.
- Estado por fase (detalle en el plan 039A-1): F1 ☑ · F2 ☑ (318A-16) · F3 ☑ · F4 ☑ (backend, 17 comandos) · F5 ☑ (cableado; verificación visual diferida al final) · F6 ☑ (bundle).

## Siguiente bloque ejecutable

**Bloque A — Poner a funcionar la UI con la app Tauri real (debug primero).**
Orden propuesto (dependencias de abajo arriba):

1. **F3 PersistenciaSqlite** — historial durable (prerrequisito de conversaciones reales; B4).
   Incluye: CRUD de conversaciones (crear/listar/renombrar/archivar/eliminar), persistir el
   mensaje de usuario en `enviar_turno` y marcar `cancelado` al abortar (huecos B6, ver plan).
2. **Comandos de conversación** en `desktop/src-tauri` (anexo §10.1): `conversacion_nueva`,
   `listar_conversaciones`, `cargar_conversacion`, `renombrar_conversacion`,
   `archivar_conversacion`, `eliminar_conversacion`, `proveedores_disponibles`,
   `config_leer`/`config_guardar`, `elegir_workspace`. Requiere que la sesión use
   `PersistenciaSqlite`, no `PersistenciaMemoria` (hueco B5).
3. **Bloqueos B1 (capabilities) y B2 (detección `__TAURI__`)** — sin ellos no llegan eventos ni
   se activa `USA_REAL`.
4. **Cablear la UI** (`main.ts`/`sidebar.ts`/`modal.ts`): nueva conversación, listado/renombrar/
   archivar/eliminar reales, selector de modelo real, modos/razonamiento, persistencia de config,
   cancelar — quitar los `TODO(backend)`.
5. **Primer `tauri dev` en debug** (build en `C:\tmp`): verificar ciclo real completo (enviar →
   streaming tokens → tools → aprobación → `turno-fin`).
6. Cierre del bloque: `tsc --noEmit` + build UI + `cargo test --workspace` + gate + commit.

> Confirmado con el usuario: **revisar/ajustar el plan primero** (hecho, anexo §10) y **debug
> primero** (no release). El panel «modo meta» real (anexo §10.5.2) entra en este Bloque A:
> requiere comandos backend nuevos (`actualizar_meta`, política de pausa) además de F3/B1/B2.

## Tareas pendientes

- [x] **F3 — PersistenciaSqlite** (`cli/src/persistencia_sqlite.rs`, 04-09): `AgentPersistence` +
      `ProgramadorTareas` sobre rusqlite bundled (WAL, `%APPDATA%/glory-harness/glory-harness.db`).
      CRUD de conversaciones, mensaje de usuario persistido por el consumidor en `enviar_turno`,
      `cancelado` al abortar (B5/B6 cerrados). Sesión Tauri usa SQLite con `user_id` estable
      (tabla `config`); respaldo en memoria con aviso si la BD no abre.
- [x] **F4 — Comandos IPC de conversación + workspace + config** (backend, 04-09): 16 comandos
      (`abrir_sesion`, `enviar_turno`, `cancelar_turno`, `responder_aprobacion`,
      `pendientes_aprobacion` + `conversacion_nueva`, `listar_conversaciones`,
      `cargar_conversacion`, `renombrar_conversacion`, `archivar_conversacion`,
      `eliminar_conversacion`, `proveedores_disponibles` (allowlist real del núcleo vía
      `catalogo_proveedores`), `config_leer`/`config_guardar`, `elegir_workspace` (rfd),
      `actualizar_meta`). `cargo check` + clippy limpios, 188 tests verdes (29 cli + 159 core).
- [ ] **Panel «modo meta» real** (anexo §10.5): `actualizar_meta` en caliente, tiempo de turno,
      tokens desde `AgenteEvento.Usage`, política de play/pausa (hoy solo `cancelar_turno`) y
      retirar el boceto temporal `panelMetaBoceto.*`. Boceto de aspecto hecho (03-09, §10.5.1).
- [ ] **B1 — capabilities** `desktop/src-tauri/capabilities/default.json` (`core:default`, ventana `main`).
- [ ] **B2 — detección de entorno** (`esEntornoTauri`/`__TAURI__`) en el primer `tauri dev`.
- [x] **F5 — Cablear UI → backend** (04-09, commit `8a1310c`): sidebar real (nueva/listar/
      cargar/renombrar/archivar/eliminar + `sustituir`), historial persistido al arrancar
      (reabre donde se quedó), modelo/modo/razonamiento en config persistida, deriva
      allowlist commandcode→glory, meta del boceto → `actualizar_meta` con puerta de modo en
      backend, `reconfigurar_sesion` al cambiar modelo/modo. Tipos `AgenteEvento` fieles al
      núcleo. **Verificación visual diferida al final** (a petición del usuario): falta ciclo
      E2E en ventana (enviar → streaming → tools → aprobación → `turno-fin`).
- [x] **F6 — Empaquetado + RAM + gate** (04-09, commit `8a8768c`): `tauri build` ok — exe
      release + MSI (`Glory Harness_0.1.0_x64_en-US.msi`) + NSIS setup en
      `C:\tmp\glory-target\glory-harness\release\bundle\`. Iconos válidos generados
      (`tauri icon`, array `icon` en config). RAM arranque release: WS 40 MB (objetivo
      ≤140 MB ✓). `install-release.bat` (copia el release a `%LOCALAPPDATA%\GloryHarness`).
      Gate `039A-1` **PASS** (0 errores, warnings preexistentes).

> **Hecho (UI, 03-09):** los botones del nav «Agentes» / «Flujo» / «Complementos» ahora
> muestran «próximamente» en el chat (`onAccionNav` en sidebar + aviso en main.ts; §10.4).
> «Nueva conversación» queda reservado a la acción real con backend (F3), no a «próximamente».
>
> **Boceto (03-09, §10.5.1):** panel «modo meta» corriendo visto sobre la UI real
> (`panelMetaBoceto.ts/.css` montado dentro de `#entrada`, antes de `.caja`; 1 línea: meta
> editable + estado · tiempo · tokens + play/pausa). Es TEMPORAL y simulado; el panel real
> (meta editable en caliente, tokens/tiempo del turno, play/pausa real) queda planificado en
> §10.5.2 como trabajo del Bloque A.

## Planes activos

- `Agente/planes/plan-glory-harness-desktop-2026-09-03.md` (039A-1) — **activo**, en curso; siguiente
  hito: Bloque A (F3 → comandos → B1/B2 → cablear UI → primer `tauri dev` debug).

## Notas

- Build siempre en `C:\tmp` (`CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness`), nunca en el árbol.
- F5 UI la está construyendo el usuario; este roadmap refleja la integración con backend real.
- Divergencia conocida (04-09, para el dueño del núcleo/CLI): el runtime solo persiste turno +
  respuesta; el mensaje del usuario lo debe persistir el consumidor — el desktop lo hace, pero
  `chat`/`run`/`tui` del CLI hoy no llaman a `guardar_mensaje` para el usuario (su historial
  entre turnos pierde el lado usuario). No se tocó por ser carril ajeno.
