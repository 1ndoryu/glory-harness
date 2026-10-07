# Plan 129A-3 — Aprobación: el turno pausa y continúa (verificación real)

Fecha: 2026-10-07. Estado: activo. Roadmap: `129A-3`.

## Objetivo

Cerrar `129A-3`: la implementación del rediseño 12-09 YA está commiteada
(`bf47423` tarjeta fija + toast, `ad86a3f` pausa/continúa intra-turno sin
reenvío, gate PASS 467 tests). Falta la verificación real con un `task`
de efecto (el fallo original: tarjeta duplicada, `mkdir` no creado y
re-pregunta tras aceptar) + re-gate sobre el árbol actual + docs.

## Alcance / no alcance

- V1 (núcleo): pedir al agente crear un directorio real → tarjeta de
  aprobación → aprobar → la tool ejecuta en el MISMO turno → el directorio
  existe, sin segunda petición y sin reenvío.
- V2 (negativo): pedir otro efecto → rechazar → el modelo recibe la
  denegación sin reintento (sin ejecución).
- No se cambia código salvo que el smoke revele un defecto real; si lo hay,
  se corrige en este plan con evidencia antes/después.
- No se tocan `logs/` (menor preexistente, fuera de alcance).

## Fases verificables

- V1: smoke aprobar — tarjeta fija en slot, "aprobada · ejecutando…" hasta
  `ToolResult`, dir creado en disco, cero re-preguntas.
- V2: smoke rechazar — aviso de denegación al modelo, nada ejecutado.
- V3: `tsc` + `vite build` (si hubo cambio front) y gate
  `sentinel check 129A-3 --full --stages scripts/quality/stages.json` PASS.
- V4: docs (roadmap, completada, este plan → `planes/completados/`) +
  commit + push + limpieza.

## Estado

- En curso 07-10: plan creado, pendiente V1.
- V1 smoke 07-10 REPRODUCE el fallo original en modo web: tarjeta
  `comando (clase: comando:bajo:**)` → Aprobar → tarjeta
  "aprobada · ejecutando…" para siempre, `smoke-129A-3` NO creado en disco.
- Causa raíz: la sesión web corre en entre-turnos por diseño (solo la app
  Tauri activa espera vía `abrir_sesion_interna`); el turno omite la tool y
  cierra, y `POST approvals/:id` (`turnos.rs`) descarta el `bool` de
  `responder_aprobacion` y responde `{ok:true}`; `api.ts` devuelve `true`
  fijo, así que el front (que YA maneja `desperto=false` en
  `aplicarEventos.ts:208-215`) promete "ejecutando…" sin que nadie ejecute.
  Contradice el contrato 129A-5 ("el frente NO debe pintar ejecutando…").
- Fix (sin reenvío, sin cambiar el modo web): el endpoint devuelve
  `{ok, desperto}` y `api.ts` propaga el valor real.
- V3 07-10: `tsc` EXIT 0; gate `sentinel check 129A-3 --full` PASS
  (530 ok, 0 fallidos; 1.º intento: 1 flaky
  `escribir_acepta_en_viva_y_falla_tras_matar`, reintento PASS);
  `vite build` 147 módulos.
- V1 07-10 (navegador real, binario nuevo): Aprobar → tarjeta
  "el turno ya terminó · envía un mensaje nuevo" (rama `desperto=false`);
  follow-up "sí, crea el directorio ahora" → `smoke-129A-3` creado VACÍO
  (0 elementos) sin re-preguntas. Token de una vez entre-turnos confirmado.
- V2 07-10 (API directa + SSE, sin navegador disponible tras reinicio app):
  `POST approvals/{aid} {"approved":false}` → `{"desperto":false,"ok":true}`;
  `smoke-129A-3-V2` NO creado; follow-up informa "la acción fue denegada",
  sin ejecución ni reintento (regla deny de clase).

## Definition of Done

- V1+V2 verificados en navegador real contra backend por mando con
  evidencia (tarjeta, ejecución, disco); gate PASS sin hallazgos nuevos;
  commit pusheado; roadmap al día.
