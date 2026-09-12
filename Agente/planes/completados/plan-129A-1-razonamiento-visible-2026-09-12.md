# Plan 129A-1 — Razonamiento visible como summary (2026-09-12)

## Objetivo
Cuando el modelo razona (`reasoning_effort` low/medium/high), el usuario ve el
pensamiento como un bloque colapsable (summary), en vivo y en el historial.
Hoy el proveedor lo devuelve (`reasoning_content`) pero se pierde en tres
puntos y el front solo lo pinta en el mock.

## Alcance / no alcance
- Sí: `reasoning_content` (OpenAI-compat: deepseek/groq/cerebras/router glory).
  Summary colapsado al cerrar el turno + persistencia para el historial.
- No: bloques `thinking` de Anthropic (otro shape), streaming vivo del
  pensamiento (se emite un evento único al completar; el vivo queda diferido),
  reintentos de prompts.

## Dependencias
Ninguna. Toca `core` (contrato de eventos), `cli` (SSE), `desktop` (Tauri+UI).

## Fases verificables
- **F1 — core captura.** `stream.rs` acumula `delta.reasoning_content`;
  `red.rs` lee `/choices/0/message/reasoning_content` (no-stream);
  `AiStreamResult.razonamiento: String`; `llm_llamada` emite
  `AgenteEvento::Razonamiento { texto }` una vez si no está vacío.
  Verifica: `cargo test -p <core>` + test con fixture JSON con reasoning_content.
- **F2 — transporte.** `Razonamiento` → cable SSE (`cli`) + evento Tauri
  (`desktop/src-tauri`). Reutiliza el camino de `Token`.
  Verifica: test del mapeo en cada emisor.
- **F3 — front.** El adaptador real alimenta `BloqueRazonamiento` (vivo →
  cerrado con meta de tiempo, contrato ya existente en `tipos.ts` /
  `mensajesBloques.ts`); el mensaje de asistente persiste el razonamiento y
  el historial lo repinta.
  Verifica: `tsc` + `vite build` + ventana `tauri dev` con modelo de
  razonamiento (colapsado abre/cierra, recarga conserva).
- **Gate:** `sentinel check 129A-1 --stages scripts/quality/stages.json` PASS.

## Estado
- F1: pendiente. F2: pendiente (descubrir puntos de mapeo de `Token`).
  F3: pendiente (descubrir reload de historial → bloques).

## Próximo paso
F1: struct + captura + evento + test fixture.

## Definition of Done
Modelo con razonamiento → summary visible en vivo, cerrado con tiempo,
persiste tras recarga; sin razonamiento (modelos no-reasoning) ningún cambio
visible; gate PASS.
