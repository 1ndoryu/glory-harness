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
- Estado por fase (detalle en el plan 039A-1): F1 ☑ · F2 ☑ (318A-16) · F3 ☑ · F4 ☑ (backend, 17 comandos) · F5 ☑ (cableado) · F6 ☑ (bundle). Correcciones del primer `tauri dev` (H1–H7) cerradas el 04-09 (ver `Agente/completados/tareas-2026-09-04.md`).

## Siguiente bloque ejecutable

**Bloque A — Poner a funcionar la UI con la app Tauri real (debug primero).**
Orden propuesto (dependencias de abajo arriba):

1. **F3 PersistenciaSqlite** — historial durable (prerrequisito de conversaciones reales; B4). ✔ (04-09)
2. **Comandos de conversación** en `desktop/src-tauri` (anexo §10.1). ✔ (04-09)
3. **Bloqueos B1 (capabilities) y B2 (detección `__TAURI__`)**. ✔ (verificados en el primer `tauri dev`)
4. **Cablear la UI** (`main.ts`/`sidebar.ts`/`modal.ts`). ✔ (04-09, F5) + panel meta real ✔ (`5dcefe4`)
5. **Primer `tauri dev` en debug** (build en `C:\tmp`): verificar ciclo real completo. ◐ hecho —
   el arranque real señaló 7 hallazgos (H1–H7), ya corregidos. 
6. **Cierre del bloque (pendiente):** E2E en la ventana real (enviar → streaming tokens → tools →
   aprobación → `turno-fin`, todo visible) + `tsc --noEmit` + build UI + `cargo test --workspace` +
   gate + commit.

> Confirmado con el usuario: **revisar/ajustar el plan primero** (hecho, anexo §10) y **debug
> primero** (no release). El panel «modo meta» real (anexo §10.5.2) entró en este Bloque A y ya
> quedó implementado (commit `5dcefe4`).

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
- [x] **Panel «modo meta» real** (04-09, commit `5dcefe4`): `actualizar_meta` en caliente,
      tiempo de turno, tokens desde `AgenteEvento.Usage`, política de play/pausa; retirado el
      boceto temporal `panelMetaBoceto.*` (anexo §10.5.2).
- [x] **B1 — capabilities** `desktop/src-tauri/capabilities/default.json` (`core:default`, ventana `main`) — verificadas en el primer `tauri dev` (04-09).
- [x] **B2 — detección de entorno** (`esEntornoTauri`/`__TAURI__`) — verificada en el primer `tauri dev` (04-09).
- [x] **F5 — Cablear UI → backend** (04-09, commit `8a1310c`): sidebar real (nueva/listar/
      cargar/renombrar/archivar/eliminar + `sustituir`), historial persistido al arrancar
      (reabre donde se quedó), modelo/modo/razonamiento en config persistida, deriva
      allowlist commandcode→glory, meta del boceto → `actualizar_meta` con puerta de modo en
      backend, `reconfigurar_sesion` al cambiar modelo/modo. Tipos `AgenteEvento` fieles al
      núcleo. El primer `tauri dev` (04-09) señaló 7 hallazgos (H1–H7) ya corregidos en el
      bloque siguiente; falta el ciclo E2E completo en ventana (enviar → streaming → tools →
      aprobación → `turno-fin`).
- [x] **F6 — Empaquetado + RAM + gate** (04-09, commit `8a8768c`): `tauri build` ok — exe
      release + MSI (`Glory Harness_0.1.0_x64_en-US.msi`) + NSIS setup en
      `C:\tmp\glory-target\glory-harness\release\bundle\`. Iconos válidos generados
      (`tauri icon`, array `icon` en config). RAM arranque release: WS 40 MB (objetivo
      ≤140 MB ✓). `install-release.bat` (copia el release a `%LOCALAPPDATA%\GloryHarness`).
      Gate `039A-1` **PASS** (0 errores, warnings preexistentes).

> **Hecho (04-09, correcciones del primer `tauri dev`):** los 7 hallazgos del primer arranque
> real quedaron corregidos (H1–H7, bloque 039A-1). Detalle en
> `Agente/documentacion/hallazgos-primer-tauri-dev-2026-09-04.md` y evidencia en
> `Agente/completados/tareas-2026-09-04.md`. Pendiente del bloque: E2E en la ventana real
> (enviar → streaming → tools → aprobación → `turno-fin`) y gate final.
>
> **Corrección de causa raíz H2 (04-09, tras el primer fix):** el usuario confirmó que la
> respuesta del asistente seguía sin aparecer EN VIVO (solo al recargar). Causa: el frontend
> `real.ts` leía el discriminante `evento` pero el backend serializa `AgenteEvento` con tag
> `tipo` (`core/src/evento.rs`) → el switch no pintaba ningún evento. Fix aplicado en
> `desktop/ui/src/tauri/real.ts` (`evento` → `tipo`; type-check limpio). **Pendiente validar en
> vivo:** relanzar `tauri dev` y confirmar que el streaming de tokens aparece en la ventana.
>
> **Hecho (UI, 03-09):** los botones del nav «Agentes» / «Flujo» / «Complementos» ahora
> muestran «próximamente» en el chat (`onAccionNav` en sidebar + aviso en main.ts; §10.4).
> «Nueva conversación» queda reservado a la acción real con backend (F3), no a «próximamente».
>
> **Boceto (03-09, §10.5.1):** panel «modo meta» corriendo visto sobre la UI real
> (`panelMetaBoceto.ts/.css` montado dentro de `#entrada`, antes de `.caja`; 1 línea: meta
> editable + estado · tiempo · tokens + play/pausa). Es TEMPORAL y simulado; el panel real
> (meta editable en caliente, tokens/tiempo del turno, play/pausa real) quedó implementado en
> §10.5.2 (commit `5dcefe4`) y el boceto se retiró.

## Planes activos

- `Agente/planes/plan-glory-harness-desktop-2026-09-03.md` (039A-1) — **activo**, en curso; las
  fases F1–F6 y el Bloque A (backend + cableado + panel meta real) están cerrados; los 7
  hallazgos del primer `tauri dev` quedaron corregidos (H1–H7). Siguiente hito: E2E en la
  ventana Tauri real y gate final del bloque.
- `Agente/planes/plan-glory-harness-ux-turno-2026-09-04.md` (039A-3) — **activo, EN EJECUCIÓN**
  (04-09): bloque completo aprobado (P1-P6). P1 (pie de turno + persistir uso/modelo real)
  HECHO (mock + type-check + cargo check). P2 (editar/volver a punto: rewind transaccional por
  `rowid` + menú por mensaje) HECHO (unit + type-check + build + mock; commit `039A-3 (P2)`).
  P4 (⋯ cabecera + sidebar colapsable/redimensionable + botón expandir) HECHO (type-check +
  build + navegador mock; commit `039A-3 (P4)`).
  P5 (2 paneles, M1) HECHO (05-09, commit `903a54b`): backend ya estaba (045f7e1, `panel_id`
  por panel); front gestor de 1-2 `.chat` — fábrica `panelChat.ts` + orquestador delgado en
  `main.ts` + "Abrir en panel lateral" (sidebar ⋯, D4) + responsive <900px apila los 2 chats.
  Verificado con navegador mock (2 paneles, turno global M1, foco al cerrar lateral, guard
  responsive). Evidencia en `Agente/completados/tareas-2026-09-05.md`.
  P6-front (indicador circular de contexto + config "Ventana de contexto" 150k) HECHO (05-09,
  commit `769492d`): SVG `.ctx-indicador` en `entrada.ts` + CSS; hook `onContexto` en `real.ts`
  (fuente única = `ContextoDetalle`/`usage`) → `setContexto` en `main.ts`/`panelChat.ts`;
  opción `contexto_max_ventana` en `opciones.ts` persistida vía config; rama mock pinta 7%/150k.
  Verificado con navegador mock (círculo llenándose + config visible). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6-front menú hover de detalle del indicador HECHO (05-09, commit `a69c2a3`): al pasar el
  cursor por el círculo se abre el pequeño menú `.ctx-detalle` con el detalle del uso de la
  ventana (usados de M + %, reserva de salida, entrada del turno; "sin datos" si no hay turno).
  Se elimina el `title` nativo; `EstadoContexto` propaga el detalle completo desde el backend.
  Verificado con navegador mock (principal y lateral). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  En curso: P3 vault de respaldos. Pendiente P6-backend (inyectar `contexto.max_ventana` 150k en
  `construir_harness_con`/`reconfigurar_sesion` de `cli/run.rs`) — BLOQUEADO por el agente
  paralelo que toca `cli/`/`core/` (sin tocar el default del core 128k).
  Pendiente de P5: E2E `tauri dev` (2 conversaciones reales, eventos al panel correcto) —
  bloqueada mientras el agente paralelo (318A-17) toca `cli/`/`core/`.
  ⚠️ Core (`sandbox.rs`) se toca en P3 (hook opcional + exclusión `.glory-harness/`) — coordinar
  con el agente paralelo (318A-17) que trabaja en core/cli y commitea en `main`.
- `Agente/planes/plan-saneamiento-calidad-2026-09-05.md` (059A-1) — **activo** (05-09): deuda de
  Sentinel (0 errores/21 warnings/10 archivos, baseline congelado en
  `Agente/completados/analyze-baseline-2026-09-05.json`) + auditoría SOLID/rendimiento + brechas
  del gate. S0 ✔ (reglas del juego verificadas en fuente `0559576`). En curso: S1 (falsos
  positivos/mejoras en glory-sentinel) → S2–S4 (splits y organización) → S5–S6 (auditorías) →
  S7 (reglas nuevas) → S8 (cierre). No toca desktop/`persistencia_sqlite.rs` (039A-3).

## Notas

- Build siempre en `C:\tmp` (`CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness`), nunca en el árbol.
- F5 UI la está construyendo el usuario; este roadmap refleja la integración con backend real.
- Divergencia conocida (04-09, para el dueño del núcleo/CLI): el runtime solo persiste turno +
  respuesta; el mensaje del usuario lo debe persistir el consumidor — el desktop lo hace, pero
  `chat`/`run`/`tui` del CLI hoy no llaman a `guardar_mensaje` para el usuario (su historial
  entre turnos pierde el lado usuario). No se tocó por ser carril ajeno.
