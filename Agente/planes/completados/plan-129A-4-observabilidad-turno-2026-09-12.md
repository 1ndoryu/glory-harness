# Plan 129A-4 — Observabilidad por turno + aprobación fail-loud

Fecha: 2026-09-12. Estado: completado (gate PASS 12-09). Roadmap: `129A-4` (retirada al cerrar).

## Objetivo

No ir a ciegas: cada turno desktop deja un log consultable (eventos +
respuestas de aprobación + panics), y una respuesta de aprobación a un `id`
desconocido hace ruido en vez de perderse en silencio.

## Alcance / no alcance

- Sí: tabla `eventos_turno` + vínculo `peticion_turno`, wiring desktop
  (`reenviar_eventos`, `responder_aprobacion`), log de proceso + panic hook,
  comando `log_turno` + visor mínimo, tests, gate, commit.
- No: cambios en `core` (sin trait nuevo), ni web/CLI/TUI (adoptan después
  usando los mismos métodos), ni rotación avanzada de logs.

## Fases verificables

- F1: `cli/src/persistencia_sqlite/eventos_turno.rs` — tablas en ESQUEMA
  (`CREATE TABLE IF NOT EXISTS`, se auto-aplica al abrir) + 4 métodos +
  struct serializable + tests roundtrip. Se omiten `Token`/`RazonamientoDelta`
  (volumen; el texto vive en `mensajes`).
- F2: desktop `turno.rs` — `reenviar_eventos` recibe `turno_id` +
  persistencia, persiste cada evento (best-effort con `eprintln!`) y vincula
  petición→turno; `sesion.rs` — `responder_aprobacion` registra ok/fallo con
  turno resuelto por vínculo + `eprintln!` en fallo.
- F3: `desktop/src-tauri/src/log.rs` — `instalar()` (dir logs, rotación 1MB,
  panic hook con timestamp) + `anotar()`; llamada al inicio de `main()`.
- F4: comando `log_turno` + registro en `invoke_handler` + visor mínimo UI.
- F5: gate `sentinel check 129A-4` + commit + completada + releer roadmap.

## DoD

- `log_turno(<turno 1889f535…>)` devuelve la secuencia real del incidente.
- Responder un `id` desconocido deja fila `respuesta_aprobacion_fallida` +
  línea en `errores.log` + `Err` al front (ya existía).
- Gate PASS asociado al commit que se integra.
