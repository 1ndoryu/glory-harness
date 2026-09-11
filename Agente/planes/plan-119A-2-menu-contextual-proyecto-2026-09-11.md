# Plan 119A-2 — Menú contextual de proyecto (referencia Paseo)

> ID: **119A-2** · Fecha: 2026-09-11 · Estado: **activo (F1 implementado y verificado 11-09 — front PASS + Rust vía PASS 119A-3 mismo árbol; pendiente verificación funcional en Tauri + commit; F2–F4 sin implementar)**.
> Origen: captura del usuario con el menú de proyecto de Paseo (RESTAURANTE):
> Open in Finder · Open in Kanban · Copy Path · Start dev · Move to space › ·
> Edit name · Pin project · Archive threads · Delete threads · Remove.

## 1. Objetivo

Cada grupo de proyecto de la sidebar (`Proyectos`, `sidebar.ts:220
añadirGrupoProyecto`) abre con clic derecho un menú contextual con las
opciones de la referencia, adaptadas a lo que GH realmente tiene. Sin
nuevo chrome: se reutiliza `menu.ts` (`abrirMenuContextual` /
`crearItemMenu`), como ya hacen las celdas de conversación
(`sidebarCeldas.ts:87`) y el botón `+` de tabs.

## 2. Punto de partida verificado (11-09)

- El grupo de proyecto solo tiene clic izquierdo (activar,
  `sidebar.ts:231`); **no hay `contextmenu` en proyectos** (solo en
  celdas, `sidebarCeldas.ts:154`).
- Backend existente (Tauri + HTTP): `workspaces_listar`,
  `workspace_crear_o_activar`, `workspace_activar_por_ruta`,
  `workspace_renombrar`, `workspace_eliminar`
  (`desktop/src-tauri/src/proyecto/workspaces.rs:61-226`,
  `cli/.../web_datos/areas.rs`, rutas en `cli/.../web/mod.rs:506`).
  `workspace_eliminar` deja las conversaciones sin área (carpeta «Sin
  proyecto»), no las borra.
- Archivar/eliminar conversación existen por unidad (F4/F5); **no hay**
  variante por proyecto, ni `fijado`, ni espacios, ni kanban, ni runner
  dev, ni «revelar en Explorador» (`workspace_abrir_con` abre un
  *archivo* con su app, no revela la carpeta).

## 3. Alcance / no alcance

**Entra:** menú en el grupo de proyecto con las 10 opciones mapeadas
abajo; backend solo donde falta (F2–F4); confirmación en las
destructivas (Delete threads, Remove); mismo menú en Tauri y web
(lo que no exista en web avisa, no falla en silencio).

**No entra:** el kanban como vista nueva, los espacios como modelo y el
runner dev (van a F5 como decisiones del usuario); watcher/terminal
siguen diferidos (089A-9).

## 4. Mapeo opción → implementación

| # | Opción (Paseo) | En GH |
|---|---|---|
| 1 | Open in Finder | F2: comando nuevo `workspace_revelar` (abre la raíz en el Explorador). Solo Tauri; en web aviso como Files/Git (089A-10). |
| 2 | Open in Kanban | **Eliminada** (decisión del usuario 11-09: «eso no va»). |
| 3 | Copy Path | F1: solo frontend (portapapeles + fallback, nunca mudo). |
| 4 | Start dev | **Diferida** (decisión del usuario 11-09). |
| 5 | Move to space › | **Diferida** (decisión del usuario 11-09). |
| 6 | Edit name | F1: existe `workspace_renombrar`; falta la UI (edición inline como filas, o reutilizar `modalProyecto`). |
| 7 | Pin project | F3: columna `fijado` + orden (fijados primero) + comando toggle. Migración SQLite como 109A-2. |
| 8 | Archive threads | F4: comando batch `conversaciones_archivar_por_proyecto` (transaccional; mejor que N llamadas desde el front). |
| 9 | Delete threads | F4: comando batch + **confirmación explícita** (destructiva). |
| 10 | Remove | F1: existe `workspace_eliminar`; falta cablearlo al menú + confirmación (las conversaciones pasan a «Sin proyecto»). |

## 5. Fases verificables

- **F1 — Menú + lo que ya existe:** `contextmenu` en
  `.proyecto-grupo-boton` → Copy Path, Edit name, Remove (con
  confirmación). Iconos 100% Lucide (`iconos.ts`); clases en español.
  Verificación: clic derecho en ventana Tauri real abre el menú; cada
  acción funciona; `tsc` + `vite build` OK.
- **F2 — Open in Finder:** `workspace_revelar` en Tauri (+ transporte y
  tipos); en web aviso visible. Verificación: abre el Explorador en la
  raíz (Tauri); en web sale el aviso, sin error mudo.
- **F3 — Pin:** migración (`fijado INTEGER`), `listar` ordenado,
  `workspace_fijar` en Tauri + `PATCH /workspaces/:wid` en CLI, item con
  estado (fijar/quitar). Verificación: el orden persiste al recargar.
- **F4 — Archive/Delete threads:** comandos batch + confirmación en
  Delete. Verificación: archivar oculta todas las del proyecto;
  eliminar las borra tras confirmar; resto de proyectos intacto.
- **F5 — Decisiones (resueltas 11-09):** Kanban **eliminada**; Start dev
  y Spaces **diferidas**. Ninguna de las tres aparece en el menú.

## 6. Gate y Definition of Done

- `tsc --noEmit` EXIT 0 + `vite build` OK por fase; componentes ≤300
  líneas; sin `innerHTML`; errores con aviso visible (toast), nunca
  consola muda.
- Verificación funcional en la ventana `tauri dev` real (el menú solo
  existe con DOM real; el modo web cubre F1/F3/F4 vía HTTP).
- Gate canónico `sentinel check 119A-2 --stages
  scripts/quality/stages.json` PASS antes del commit; commit en español
  con ID; evidencia en `Agente/completados/tareas-2026-09-11.md`.

## 7. Estado y siguiente paso

**F1 implementado 11-09:** `contextmenu` en `.proyecto-grupo-boton`
→ Copy Path (portapapeles + fallback) / Edit name (inline, Enter/Escape/blur)
/ Remove (dos pasos: Remove → «Quitar … del área» + Cancelar). Nuevo
`componentes/sidebarMenuProyecto.ts` (menú + renombrado); cableado
`onRenombrarProyecto`/`onEliminarProyecto` en `sidebar.ts` +
`orquestador/barraLateral.ts` (backend + resync + aviso visible);
estilo del input en `estilos/layout.css`. `tsc --noEmit` EXIT 0 +
`vite build` OK (109 módulos, 1.33s).
**Siguiente paso:** verificación funcional en ventana `tauri dev` real (clic derecho + cada acción; pendiente de disco) → commit en español con ID (evidencia ya en `Agente/completados/tareas-2026-09-11.md`). Handlers extraídos a `orquestador/barraLateralProyecto.ts` (límite 300 líneas).
**F2 implementado 11-09:** `workspace_revelar` (Tauri, por id, fail-closed si la carpeta falta; registrado en `main.rs`) + `workspaceRevelar` en `Transporte` (invoke Tauri; en web rechaza con aviso) + `revelar` en `sesion.workspaces` + ítem `Open in Finder` primero en el menú + `onRevelarProyecto` en `sidebar.ts`/`barraLateral.ts`/`barraLateralProyecto.ts` (sin resync, no muta estado). `tsc` EXIT 0 + `vite build` OK (111 módulos) + gate `sentinel check 119A-2` PASS (457 tests ok).
F2–F4 sin implementar. Decisiones F5 cerradas el 11-09: Kanban eliminada,
Start dev y Spaces diferidas.
