# Plan 119A-5 — Meta: fix `meta_leer` + tareas fijas y colapsables

> ID: **119A-5** · Fecha: 2026-09-11 · Estado: **activo (F1 implementado y verificado 11-09 — front PASS + Rust vía PASS 119A-3 mismo árbol; pendiente verificación funcional en Tauri + commit; F2–F3 sin implementar)**.
> Origen: reporte del usuario — (a) toast `no se pudo leer la meta:
> invalid args 'conversacionId' for command 'meta_leer': command meta_leer
> missing required key conversacionId` al escribir; (b) «empezó una meta sin
> yo empezarla»; (c) las tareas deberían vivir fijas y colapsables como la
> meta. **Solo planificación: sin código tocado.**

## 1. Causa del error (verificada en fuente, sin ejecutar)

- Tauri 2 mapea los args del `invoke` de camelCase (JS) a snake_case
  (Rust). Prueba en el propio repo: `workspace_leer_archivo` se invoca con
  `{ rutaRelativa }` (`transporteTauri.ts:108-113`) contra el parámetro Rust
  `ruta_relativa` (`archivos/filesystem.rs:88-122`) y Files funciona en la
  ventana real.
- `metaLeer` invoca con `{ conversacion_id }` (`transporteTauri.ts:95-96`)
  contra el parámetro `conversacion_id` (`workspaces.rs:325-328`): Tauri
  espera `conversacionId`, no lo encuentra y rechaza → el toast sale de
  `avisarFallo('leer la meta', …)` (`vistaMeta.ts:94-96,116-118`).
- El fallo salta al escribir porque la lectura corre tras cada turno
  (`notificarTurnoFin` → `refrescarEstadoMeta(true)`, `vistaMeta.ts:66`) y al
  cambiar de conversación (`sincronizarPanelMeta`, `vistaMeta.ts:204`).
- El mismo bug afecta a `meta_aplicar` (`transporteTauri.ts:88-94`, también
  manda `conversacion_id` + `turno_id`): en Tauri están rotos fijar, pausar,
  reanudar, lograr y leer durables. En web no (endpoints por sesión, sin id).
- Daño colateral silencioso (claves snake opcionales que Tauri ignora y caen
  a `None`/default): `panel_id` en `enviar_turno`, `cancelar_turno`,
  `conversacion_nueva`, `cargar_conversacion`, `eliminar_conversacion`,
  `restaurar_archivos_tramo`, `rewind_conversacion`; `solo_lectura` en
  `enviar_turno` (el `/meta` solo-lectura en Tauri puede no estar llegando al
  backend). F1 incluye auditarlas todas, no solo la meta.

## 2. «Meta que empieza sola» (explicación, no defecto de backend)

- Tras el primer mensaje la fila de meta APARECE vacía por diseño (069A-11:
  oculta en borrador, `mostrar(hayConversacion)` en `vistaMeta.ts:198-199`).
  No hay meta iniciada: es la caja vacía (`placeholder 'meta…'`) + lectura
  rota que nunca trae estado. F3 lo hace explícito (estado vacío visible).
- Las «tareas que aparecieron» las crea el MODELO con la tool `todo`
  (evento `tareas_actualizadas`, 109A-5 F2), no el usuario ni la meta. Hoy el
  bloque vive en el transcript y cada evento lo MUEVE al final
  (`aplicarEventos.ts:138-151`), así que «persigue» al último mensaje en vez
  de quedarse fijo como la meta. De ahí la petición: F2.

## 3. Fases

- **F1 — Claves camelCase en el borde Tauri (fix del error).**
  En `transporteTauri.ts` (único punto de invocación): `conversacion_id` →
  `conversacionId`, `turno_id` → `turnoId`, `panel_id` → `panelId`,
  `solo_lectura` → `soloLectura`; auditar el resto de invokes del fichero
  contra las firmas Rust (`main.rs`, `chat/*`, `proyecto/*`,
  `archivos/*`) y corregir igual. El tipo interno `ComandoMetaVisible`
  (snake) no cambia: la traducción vive solo en el transporte.
  Verificación: `tsc` + en `tauri dev` real — enviar mensaje sin toast de
  meta; fijar/pausar/reanudar/lograr/leer meta sin error; reenvío lateral y
  `/meta` solo-lectura con efecto real. Prevención: nota en
  `Agente/prevencion/` (las claves opcionales en snake fallan en silencio
  hacia el default; síntoma: «funciona pero en el panel equivocado») y, si
  cabe en la fase, sonda que cruce claves de `invoke` contra parámetros de
  `#[tauri::command]`.
- **F2 — Tareas fijas y colapsables como la meta.**
  Sacar el bloque de `tareas_actualizadas` del flujo del transcript a una
  zona fija de la columna del chat (misma familia visual que `metaLogros`:
  cabecera con icono + título + cuenta `hechas/total`, clic que colapsa,
  estado colapsado por conversación). Un bloque por panel (principal y
  lateral tienen su `turnoReal`/estado), se limpia al cambiar de
  conversación y conserva en memoria el último plan por conversación
  mientras dure la sesión (el backend no lo persiste: tras recargar no hay
  plan que mostrar y el bloque queda oculto, igual que hoy con lista
  vacía). Verificación: plan del modelo visible sin scrollear, colapso
  intacto tras actualizaciones en vivo, cambio de conversación no mezcla
  planes, `tsc` + `vite build`.
- **F3 — La meta distingue «sin meta» de «meta vacía».**
  Estado vacío explícito en la fila (la caja vacía + lectura OK = «sin
  meta», no una meta fantasma) y `confirmarEdicion` (`panelMeta.ts:168-172`)
  solo dispara `onMetaCambiada` si el valor cambió (hoy el blur dispara
  fijar/limpiar aunque no se editara nada). Verificación: primer mensaje sin
  meta aparente iniciada; foco+blur sin cambios no emite comandos.

## 4. Reglas

Sin `innerHTML`; iconos Lucide; clases en español; errores con aviso, nunca
mudos; componentes ≤300 líneas (si la zona fija crece, componente nuevo, no
engordar `tareasMeta.ts` más allá del límite).

## 5. Gate y Definition of Done

- `tsc --noEmit` EXIT 0 + `vite build` OK por fase.
- Prueba funcional en `tauri dev` real (el bug solo existe en el borde
  Tauri; el modo web no lo reproduce).
- Gate canónico `sentinel check 119A-5 --stages
  scripts/quality/stages.json` PASS; commit en español con ID; evidencia en
  `Agente/completados/tareas-2026-09-11.md`.

## 6. Estado y siguiente paso

**F1 implementado 11-09:** claves snake → camel en `transporteTauri.ts`
(`panel_id`→`panelId` ×8, `solo_lectura`→`soloLectura`,
`turno_id`→`turnoId`, `conversacion_id`→`conversacionId` ×2);
`metaAplicar` traduce en la clave del `invoke` sin tocar el tipo interno
snake; auditados todos los `invoke` del front (navegador y resto ya en
camelCase; eventos/payloads del backend conservan snake). Convención
confirmada contra firmas Rust (`turno.rs`, `conversaciones.rs`,
`workspaces.rs`, `sesion.rs`, `navegador/comandos.rs`). Prevención en
`Agente/prevencion/prevencion-claves-tauri-camelcase-2026-09-11.md`.
`tsc` + `vite build` OK. Gate 11-09: front stages PASS sobre el mismo
árbol (cache hit); etapa `rust` cubierta por el PASS 119A-3 (mismo código,
441 tests) — el gate propio no corre por preflight de disco (ver
completadas). Evidencia ya en `Agente/completados/tareas-2026-09-11.md`.
**Siguiente paso:** en `tauri dev` real (pendiente de disco) — enviar
mensaje sin toast de meta; fijar/pausar/reanudar/lograr/leer meta sin
error; `/meta` solo-lectura con efecto → commit. F2 y F3
independientes una vez cerrado F1 (arrancar F1 era el siguiente paso;
F2/F3 sin implementar).
