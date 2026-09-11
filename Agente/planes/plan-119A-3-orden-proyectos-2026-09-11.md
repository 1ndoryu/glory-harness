# Plan 119A-3 — Ordenar proyectos e hilos (referencia Paseo)

> ID: **119A-3** · Fecha: 2026-09-11 · Estado: **activo (F1 hilos implementado
> y gate PASS 11-09; pendiente verificación en `tauri dev` + commit)**.
> Origen: segunda captura del usuario (menú `Sort projects` de Paseo):
> `Sort projects`: Last user message · Created at · Manual (✓) /
> `Sort threads`: Last user message (✓) · Created at.

## 1. Objetivo

Botón de orden en la cabecera `Proyectos` de la sidebar (junto al `+`
de `sidebar.ts:141`) con el menú de la referencia: criterio para
proyectos y criterio para hilos, con ✓ en el activo y preferencia
persistida. Valores por defecto como Paseo: proyectos = Manual, hilos =
Last user message.

## 2. Punto de partida verificado (11-09)

- Proyectos: el backend ordena por `creada_en DESC`
  (`cli/src/persistencia_sqlite/workspaces.rs:92`); la tabla solo tiene
  `creada_en` —**no hay última actividad ni orden manual**— y el tipo UI
  `Workspace` solo trae `creada_en` (`dominio/tipos.ts:39`).
- Hilos: las conversaciones se listan por `actualizada_en DESC`
  (`conversaciones.rs:83,126,204,211`); pero el tipo UI `Conversacion`
  **no trae ningún timestamp** (`tipos.ts:26`), así que el front hoy no
  puede ordenar nada por sí mismo.
- La sidebar pinta los grupos en el orden que recibe
  (`sidebar.ts:260`) y no hay botón de orden en la cabecera.
- Interacción con 119A-2 F3 (Pin, aún sin implementar): cuando exista
  `fijado`, los fijados van primero y el criterio ordena **dentro** de
  cada grupo (fijados / resto). 119A-3 no depende de F3: si no hay
  `fijado`, el criterio ordena toda la lista.

## 3. Alcance / no alcance

**Entra:** botón + menú, los 5 criterios, persistencia de la
preferencia, campos que faltan (`ultima_actividad` por proyecto,
timestamps de conversación en el front, columna `orden_manual`),
arrastre para el orden manual, mismo comportamiento en Tauri y web.

**No entra:** Pin (119A-2 F3); cambiar el orden por defecto del backend
para otros consumidores (el criterio vive en el front; el backend solo
expone los campos).

## 4. Fases verificables

- **F1 — Hilos (sin backend nuevo salvo exponer campos):** `creada_en` +
  `actualizada_en` en los listados que llegan al front (Tauri
  `conversaciones_*` + HTTP `api.ts`, mismo origen que ya ordena por
  `actualizada_en`); menú `Sort threads` con Last user message /
  Created at aplicado dentro de cada grupo (y «Sin proyecto»). Verificación:
  crear/mensajear reordena según el criterio; `tsc` + `vite build` OK.
- **F2 — Proyectos por actividad y creación:** `ultima_actividad` por
  proyecto (MAX de `actualizada_en` de sus conversaciones, calculado en
  el backend para no traer todo al front) + menú `Sort projects` con
  Last user message / Created at. Verificación: el proyecto con el
  mensaje más reciente sube primero; sin mensajes vale `creada_en`.
- **F3 — Manual:** columna `orden_manual` (migración como 109A-2),
  criterio Manual en el menú (por defecto para proyectos, como Paseo) y
  reorden por arrastre en la sidebar con persistencia inmediata (sin
  estado a medias: o se guarda el orden o se avisa). Verificación:
  arrastrar persiste al recargar; Tauri y web coinciden.
- **F4 — Preferencia:** el criterio elegido (proyectos + hilos)
  persiste en la persistencia existente de la sidebar y se restaura al
  arrancar. Verificación: recargar mantiene criterios y marca ✓.

## 5. Reglas de UI

Menú con `menu.ts` (misma infraestructura que 119A-2 F1); ✓ Lucide en
el criterio activo (no texto); clases en español; sin `innerHTML`; todo
fallo (persistencia, lectura de campos) con aviso visible, nunca mudo.

## 6. Gate y Definition of Done

- `tsc --noEmit` EXIT 0 + `vite build` OK por fase; componentes ≤300 líneas.
- Verificación funcional en la ventana `tauri dev` real (arrastre incluido)
  + comprobación en modo web (criterios F1/F2 vía HTTP).
- Gate canónico `sentinel check 119A-3 --stages
  scripts/quality/stages.json` PASS antes del commit; commit en español
  con ID; evidencia en `Agente/completados/tareas-2026-09-11.md`.

## 7. Estado y siguiente paso

**F1 hilos implementado 11-09** (recorte viable del reto: Sort threads,
sin drag; Sort projects = F2–F4). Cambios:
- Rust: `InfoConversacion.creada_en` (`persistencia_sqlite.rs:226`) +
  las 3 queries (`conversaciones.rs:82,122,209,216`) + constructor desktop
  (`chat/conversaciones.rs:92`). Revisión estática: los 4 literales
  actualizados, índices SELECT coherentes (3 = `creada_en`), sin
  destructuring exhaustivo en el repo.
- Front: `realTipos.ts:86` (`creada_en`), `tipos.ts` (`creadaEn/actualizadaEn`
  + icono `ordenar`), `sidebarOrdenHilos.ts` nuevo (criterio/comparador/
  botón), `sidebar.ts` (botón `.prog-orden` + `ordenarGrupo` en `pintarLista`
  + `ponerCriterioHilos`), `sesionVista.ts` (mapeo), CSS `layout.css`/`tema.css`.
  `tsc` + `vite build` OK.
- Extra: `onRenombrarProyecto/onEliminarProyecto` extraídos a
  `orquestador/barraLateralProyecto.ts` (barraLateral superaba 300 líneas
  efectivas por F1 de 119A-2; el gate lo marcó como warning).
- Gate 11-09 08:05 (tras liberar cachés hermanos → 8,27 GB):
  `sentinel check 119A-3` **PASS** — coverage/sccache/sentinel/rust
  (rust 262 s, 441 tests OK, shell Tauri incluido; el warning
  `limite-lineas` desapareció tras la extracción).
  Reporte: `.quality-reports/check/119A-3/latest.md`.

**Siguiente paso:** verificación en ventana `tauri dev` real (pendiente de
disco: la pasada consumió ~2,2 GB, quedan 6,06 GB) → commit + evidencia en
`Agente/completados/tareas-2026-09-11.md` (evidencia ya registrada).
Orden respecto a 119A-2: independiente.
