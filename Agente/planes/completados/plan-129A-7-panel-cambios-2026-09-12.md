# Plan 129A-7 — Panel "Cambios": sin git, multi-proyecto, por turno, aceptar/rechazar

Fecha: 2026-09-12. Estado: activo. Roadmap: `129A-7`.

## Estado real verificado (no asumir otro)

- Tab `git` titulada "Git local" (`desktop/ui/src/orquestador/panelDerecho.ts:232`,
  `componentes/panelDerecho.ts:104,118`). Backend `workspace_git_estado`
  (`desktop/src-tauri/src/proyecto/git.rs:56`) ejecuta `git.exe` en la ÚNICA
  raíz activa (`raiz_activa`); sin repo devuelve `aplicable:false` + "el
  workspace no es un repositorio Git". Hoy: depende de git CLI, una sola raíz,
  sin git = mensaje vacío.
- Los cambios del agente YA se capturan por dos vías: en vivo, `tool_result`
  `file_write/file_patch` + diff → `onCambioArchivo` → preview de Files
  (`tauri/aplicarEventos.ts:126-139`, `orquestador/ganchos.ts:66`); en recarga,
  `registrarCambioHistorial` (`componentes/panelChatHistorial.ts:82,92`).
  `AccionRecuperada` trae `turno_en` (string temporal, `tauri/realTipos.ts:123`)
  pero NO `turno_id` (la BD sí lo guarda en `eventos_turno`).
- Rewind parcial existe ("Archivos que tocó el último tramo rebobinado",
  `realTipos.ts:143`): hay precedente de restaurar, no partir de cero.
- Referencia Synara (copiar el diseño, no el código): `turnDiffSummaries` por
  `turnId` con `files[]` (`apps/web/src/components/chat/MessagesTimeline.tsx:1927`),
  `DiffPanel` con `onOpenTurnDiff(turnId, path)` (revisión por archivo y turno),
  checkpoints con `checkpointRef`, turnos de solo-cambios colapsados con resumen
  al final.

## Decisiones del usuario (12-09, registradas)

- Aceptar = solo marcar revisado (sin tocar git).
- Rechazar = restaura directo + aviso (sin confirmación).
- Carpetas agrupadas y colapsadas por defecto; el usuario expande la que le
  interesa (no solo la abierta/activa).
- Cambios viejos de git: se ven como ahora, cambios mínimos.

## Estado

- F1–F4 implementados 12-09 (mañana), pendientes de gate.
- F1: `panelCambios.ts` + `cambios.css` nuevos; tab 'git' titulada "Cambios";
  `panelDerecho.ts` ('Git local'→'Cambios' ×2); git embebido debajo.
- F2: multi-raíz gratis (vault por workspace; el panel sigue la conversación
  activa); F3: `AccionRecuperada.turno_id` (cli) + comandos
  `cambios_archivo`/`rechazar_cambio` + refresco vivo (debounce 800 ms,
  diffs por ruta) + `convId` del panel activo; F4: aceptar→localStorage
  `cambios-revisados:<conv>`, rechazar→vault directo + purga.
- `tsc` bloqueado por trabajo 129A-12 ajeno a medias (ver 129A-12): se hace
  gate tras completar 129A-12.

## Fases verificables

- F1: renombrar a "Cambios" + fuente filesystem primero: la tab muestra los
  cambios del agente de la conversación (ruta, tool, diff, turno) aunque no
  haya git; el estado git pasa a sección secundaria "Git (raíz)". Backend:
  exponer `turno_id` en las acciones recuperadas. Test: área sin git →
  panel útil con los cambios del turno.
- F2: multi-raíz: fan-out de `workspace_git_estado` por cada carpeta del área
  abierta, agrupado por proyecto/carpeta en la misma tab; timeout y techo de
  salida por repo (los actuales: 5 s, 1 MB). Carpetas sin git muestran solo
  cambios del agente. Test: área con 2 repos + 1 carpeta sin git.
- F3: reciente vs previo + "turno X": marca de agua por turno/sesión (el turno
  actual resaltado, previos atenuados, cada cambio con su turno y hora).
  Decisión de diseño en F1 (snapshot al abrir conversación vs diff contra
  HEAD); registrar la elegida. Test: cambio viejo de git atenuado sin turno,
  cambio del agente con "turno … · hora".
- F4: aceptar/rechazar por cambio del agente: rechazar restaura el contenido
  previo (guardar el "antes": el diff ya viaja en el evento; si no basta,
  checkpoint estilo Synara), revalida y marca el cambio; aceptar lo confirma.
  Funciona CON y SIN git (el "antes" lo guarda GH, no el índice). Test:
  rechazar revierte el archivo; aceptar lo deja; sin git igual.

## No alcance (pendiente a futuro, petición explícita)

- Escrituras git (stage/commit/discard vía índice, push/pull): solo lectura
  git en esta tarea. Diseñarlas será otra tarea.

## DoD

- Sin git el panel es útil; con N repos agrupa por proyecto; cada cambio del
  agente dice en qué turno se hizo; aceptar/rechazar revierte real con y sin
  git; gate PASS + `tsc` EXIT 0.
