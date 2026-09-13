# Plan 129A-8 — Resumen de cambios al finalizar el turno (estilo Synara)

Fecha: 2026-09-12. Estado: activo. Roadmap: `129A-8`. Pedido anterior no
ejecutado: al crear/modificar archivos no hay resumen al cerrar el turno.

## Estado real verificado

- Al `done` solo se añade el pie de turno (tokens/modelo/velocidad/log,
  `componentes/panelChatHistorial.ts:205 anadirPieTurno`): ningún resumen de
  archivos tocados.
- Los datos ya existen en vivo: `tool_start` guarda `st.rutaHerramienta` para
  `file_write/file_patch` y `tool_result` trae `resumen` + `diff`
  (`tauri/aplicarEventos.ts:101-142`). En recarga, las acciones vienen
  intercaladas por turno (`turno_en`, `panelChatHistorial.ts:117-149`).
- Referencia Synara: resumen de archivos cambiados anclado al final del turno
  (`MessagesTimeline.tsx:2289-2479`), click abre el diff del turno
  (`onOpenTurnDiff`); turnos de solo-cambios colapsados.

## Decisiones del usuario (12-09, registradas)

- Como Synara y VSCode: conteo + lista de archivos con enlace al diff.
- Turno sin cambios: sin bloque.
- Solo automático (nada redactado por el modelo).

## Fases verificables

- F1: acumular por turno en vivo (lista de {ruta, tool, ok} del `tool_result`
  con diff) y al `done` pintar bloque-resumen tras el pie: "3 cambios: 1
  creado · 2 modificados" + lista con enlace "ver en Cambios" (abre la tab
  129A-7 en el archivo). Turno sin cambios: sin bloque (como Synara: las
  secciones solo aparecen con contenido, `panelGit.ts:77-79`).
- F2: recarga: derivar el mismo bloque de las acciones persistidas
  (intercalado por `turno_en` ya implementado) para que el historial viejo
  también lo muestre.
- F3: smoke `tauri dev`: crear + modificar → el resumen aparece al cerrar;
  click abre el diff.

## No alcance

- No pedir al modelo que redacte el resumen: es determinista desde eventos.
  Sin modelo, sin prompt, sin coste.

## DoD

- Crear/modificar archivos → al cerrar el turno aparece el resumen con conteo
  y enlaces al diff; visible también tras recargar; gate PASS + `tsc` EXIT 0.
