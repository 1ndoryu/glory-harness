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

## F1 — resultado (verificado 07-10-2026, fija el diseño)

- Transcript del hijo NO recuperable: `mensajes: Vec<AiMessage>` vive solo
  en memoria de `ejecutar_subagente` (`core/src/nucleo/runtime/subagente.rs:159`)
  y se descarta al terminar; el hijo nunca escribe persistencia por diseño
  (aislamiento, `core/src/nucleo/subagente.rs:6-8`).
- Superviviente único: `resumen_acotado(texto_final)` vía
  `SubagenteFin { resumen, ok, parcial }` + resultado de tool al padre.
- Sin IDs: `SubagenteInicio { perfil, instruccion }` /
  `SubagenteFin { resumen, ok, parcial }` sin UUID ni turno propio (el hijo
  se audita contra el `turno_id` del padre); correlación inicio→fin por orden
  FIFO, ambigua si 2 concurrentes
  (`CONCURRENTES_MAX_SUBAGENTES = 2`, `core/src/nucleo/subagente.rs:183`).
- Actividad viva SÍ visible: el hijo emite `ToolStart`/`ToolResult`/
  `PeticionAprobacion`/`RequiereAprobacion`/`PermisoDenegado` al mismo `tx`
  (`core/src/nucleo/runtime/subagente.rs:344-489`), indistinguibles de las
  del padre en el stream.
- Front en un solo punto: tipos en
  `desktop/ui/src/tauri/tipos/realTiposEventos.ts:37-38`, avisos de una línea
  en `desktop/ui/src/tauri/aplicarEventos.ts:231-235`; el adaptador web
  comparte la misma superficie (`desktop/ui/src/adaptadores/api.ts:321-323`).
- Aplica el fallback registrado: F2 muestra instrucción+estado+resumen en
  vivo; F3 recortada a ficha de solo lectura (perfil+estado+instrucción+
  resumen, tab `subagente:<n>` sin caja de escritura). Transcript completo
  requeriría persistencia en core = alcance nuevo, no se hace.

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
- F4 en curso 07-10: backend por mando (`Arranque up glory-harness`,
  `http://harness.localhost:8799/`, conversación `b802c65c`, turno hijo
  `982efa95` perfil explorar) con la pregunta "dónde se define la tool
  `task`". Tarjeta F2 visible en vivo (`subagente explorar: trabajando`) y
  toggle colapsar/expandir verificado. Gate full PASS tras 1 reintento (un
  flaky ajeno: `runner_real_lee_payload_y_acepta_ajuste`, 346ok/1fail →
  529ok/0fail) y 2 INFO ISP nuevos corregidos (`PanelDerechoSubagentes`,
  `RazonamientoEnCurso`). Falta: fin del hijo → resumen en tarjeta → click
  → tab `subagente:<n>` de solo lectura → captura → commit.

## No alcance

- Enviar mensajes al subagente, reintentar o bifurcar su conversación:
  lectura sola, petición explícita.

## DoD

- Subagente trabajando = tarjeta fija visible; su conversación se lee en el
  lateral sin poder intervenir; al terminar queda el resumen; gate PASS.
