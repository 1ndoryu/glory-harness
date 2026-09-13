# Plan 129A-11 — Subagentes fijos + conversación de solo lectura

Fecha: 2026-09-12. Estado: activo. Roadmap: `129A-11`. Pregunta del usuario
respondida: **sí, GH ya crea subagentes** (ver abajo); lo que falta es
mostrarlos.

## Estado real verificado

- Core: tool `task` ("Delega una tarea acotada a una sesión hija…",
  `core/src/nucleo/subagente.rs:248`), perfiles
  explorar/planificar/revisar/redactar (`:45-75`), tope 2 concurrentes,
  profundidad máxima 1, parciales por presupuesto; eventos SSE
  `SubagenteInicio {perfil, instruccion}` / `SubagenteFin {resumen, ok,
  parcial}` (`core/src/contrato/evento.rs:116-129`); hooks de sesión hija
  (`core/src/nucleo/hooks.rs:80-82`).
- Front: solo dos avisos de una línea (`tauri/aplicarEventos.ts:192-197`,
  tipos en `tauri/realTipos.ts:34-35`). Nada fijo, nada clicable, nada del
  contenido del hijo. Mock `datos/conversaciones.ts:11` ("Subagente explorar")
  sugiere que la idea ya se barajó.

## Decisiones del usuario (12-09, registradas — CAMBIAN el diseño)

- NO va fija: tarjeta FLOTANTE y minimizable sobre el chat principal,
  arriba a la derecha dentro del panel principal; minimalista y pequeña.
- Muestra: archivos de la conversación, imágenes, subagentes en ejecución
  (agrupados en una sola tarjeta).
- Click en un subagente: se abre como una conversación normal en el lateral,
  pero SIN caja de escritura.
- Al terminar: queda colapsada con su resumen.

## Fases verificables

- F1 (técnico, sin UI): determinar qué transcript del hijo es recuperable
  (¿sus eventos llevan turno_id propio? ¿la sesión hija persiste mensajes
  legibles por el padre?) y fijarlo en este plan; si no hay nada persistido,
  la F2 muestra instrucción+estado+resumen final en vivo y la F3 queda
  recortada a eso (decisión registrada, no inventada).
- F2: tarjeta fija por subagente activo (perfil, instrucción recortada,
  estado trabajando/fin/parcial) anclada en el chat mientras trabaja; al
  terminar queda con su resumen (colapsable). Reutiliza el patrón de
  `tareas_actualizadas` (bloque vivo único, `aplicarEventos.ts:201-213`).
- F3: click en la tarjeta → conversación del subagente en el panel lateral
  (tab `chat:<id>` ya soportada por `panelDerecho.ts:260,279`) en modo
  SOLO LECTURA: sin caja de envío (garantía estructural, no solo `disabled`)
  y sin transporte de envío cableado.
- F4: smoke `tauri dev` con un `task` real: tarjeta fija → click → lectura
  del hijo → fin con resumen.

## No alcance

- Enviar mensajes al subagente, reintentar o bifurcar su conversación:
  lectura sola, petición explícita.

## DoD

- Subagente trabajando = tarjeta fija visible; su conversación se lee en el
  lateral sin poder intervenir; al terminar queda el resumen; gate PASS.
