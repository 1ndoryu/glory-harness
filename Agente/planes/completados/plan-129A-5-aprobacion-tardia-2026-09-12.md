# Plan 129A-5 — Aprobación tardía congela la tarjeta

Fecha: 2026-09-12. Estado: completado. Roadmap: `129A-5`.

## Evidencia (verificada, no hipótesis)

- Conversación `b4eccce1-5b6f-42b7-8284-51c60ac8ce9e`, turno
  `44aaf1d0-2522-426b-bbb2-6f3455e57a46` (`estado='ok'`):
  `peticion_aprobacion` 10:44:49Z → `tool_result(requiere_aprobacion)` →
  `done` 10:44:50Z (`motivo_cierre=respuesta_final`, `denegaciones=1`) →
  `respuesta_aprobacion {aprobar}` 10:44:54Z (4 s DESPUÉS del cierre).
- Una sola fila en `peticion_turno` (c20554f7→44aaf1d0); ninguna
  `respuesta_aprobacion` previa al cierre: el turno nunca esperó.
- Sin `errores.log`: no hay crash ni panic, solo una tarjeta colgada.

## Causa raíz

1. `desktop/src-tauri/src/main.rs:349-357`:
   `fijar_espera_aprobacion_en_turno(true)` vive dentro de `if autocreada`.
   Con conversaciones existentes el flag queda apagado y el desktop corre en
   modo entre-turnos; como 129A-3 eliminó el reenvío del frente, lo aprobado
   tarde no lo ejecuta nadie.
2. `core/src/herramientas/tool.rs:606` `responder_peticion` devuelve `Ok`
   aunque no haya waiter (`habia_espera` solo decide el token); el frente
   interpreta `Ok` como "aprobada · ejecutando…" sin nadie que continúe.

## Fases verificables

- F1: `main.rs` — el flag se fija siempre al abrir sesión desktop (fuera del
  `if autocreada`). Vale para `abrir_sesion` y las 3 llamadas de
  `workspaces.rs` (mismo `abrir_sesion_interna`).
- F2: core `responder_peticion` → `Result<bool, String>` (`true` = se
  despertó a un turno en espera); `runtime.responder_aprobacion` propaga;
  tests `tool.rs` afirman ambos casos (con/sin espera).
- F3: `sesion.rs responder_aprobacion` → `Result<bool, String>` + evento
  `respuesta_aprobacion_sin_espera` cuando aplica; frente
  (`realTipos.ts`, `transporteTauri.ts`, manejador de respuesta) pinta
  "el turno ya terminó — envía un mensaje nuevo" en vez de "ejecutando…".
- F4: gate `sentinel check 129A-5` + commit + completada + releer roadmap.

## DoD

- Reproducir el caso 44aaf1d0 es imposible por construcción: con el flag
  siempre activo el turno pausa hasta la respuesta.
- Responder a una petición sin espera devuelve `false` y la tarjeta lo dice
  en vez de colgarse; gate PASS asociado al commit.
