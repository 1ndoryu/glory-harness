# Plan 089A-3 — Barra superior global estilo Synara (2026-09-08)

Referencia: `area-trabajo/synara` (Electron, `apps/web/src`).
Ficheros clave estudiados:
- `components/DesktopWindowControls.tsx`: botonera fija arriba-derecha, 46px de
  alto, 3 botones de 46px (minimizar, maximizar/restaurar, cerrar), glifos
  nativos Segoe Fluent Icons (`\uE921\E922\E923\E8BB`), planos sin radio/borde,
  cerrar con hover rojo `#c42b1c`.
- `components/chat/RightDock.tsx` (`RightDockLauncher`) + `rightDockPaneMeta.tsx`:
  launcher centrado con botones full-width (icono + etiqueta), orden
  Review, Terminal, Browser, Files, Side chats, Source control (con gating).
  → Esto es la PARTE 2 (089A-4), no entra aquí.

## Alcance parte 1 (este plan)

Barra superior GLOBAL a todo el ancho (por encima de sidebar + paneles +
panel derecho), con zona de arrastre y botonera min/max/cerrar como Synara.

1. Nuevo `desktop/ui/src/componentes/barraSuperior.ts`:
   - Altura 46px, full-width, primera hija de `#app` (antes de `#cuerpo`).
   - Izquierda: toggle sidebar + toggle panel derecho + botón visor (se MUDAN
     desde la cabecera del principal; callbacks `onToggleSidebar`,
     `onTogglePanelDerecho`, `onAbrirVisor` + setters
     `setSidebarAbierta`/`setPanelDerechoAbierto` espejo de la cabecera).
   - Centro: zona libre arrastrable (marca `glory-harness`).
   - Derecha: `crearControlesVentana()` (retag a estilo caption).
   - Arrastre + botonera solo bajo Tauri (`esEntornoTauri`); en web la barra
     queda con solo los toggles.
2. Nuevo `desktop/ui/src/estilos/barraSuperior.css`: `.barra-superior`,
   `.caption-boton` (46px, plano, glifo 10px Segoe), hover gris, cerrar rojo.
3. `ventana.ts`: `crearControlesVentana()` usa glifos Segoe en vez de
   `icono()` SVG; mantiene cableado minimize/toggleMaximize/close + refresco
   restore + deshabilitado con aviso (nunca mudo).
4. `cabecera.ts`: QUITAR bloque 089A-1 (import + arrastre + botonera) y los
   botones mudados a la barra (toggle sidebar, visor, toggle derecho del
   principal). La cabecera queda con título + ⋯ (+ × en laterales). Las
   opciones y setters se conservan como no-op para no tocar `panelChat.ts`.
5. `main.ts` (edits quirúrgicos delimitados `[089A-3]`): montar la barra antes
   de `#cuerpo`; `aplicarSidebar` y `pintarToggleDerecho` también reflejan en
   la barra.
6. `layout.css`: SIN TOCAR (tiene cambios ajenos sin commitear); `#app` ya es
   `flex-direction: column`, la barra entra en flujo.

## Decisiones / gotchas

- Orden min/max/cerrar coincide Paseo ↔ Synara; se conserva.
- Excepción a "sin rojo hover" (identidad del proyecto): el usuario pidió los
  botones "como esa referencia" y Synara usa `#c42b1c` en cerrar. Se aplica
  fiel y se registra aquí como decisión explícita del usuario.
- No se tocan `panelChat.ts`, `panelDerecho.ts`, `mensajes.ts`, `sidebar.ts`
  ni `layout.css` (mezclan trabajo ajeno sin commitear).
- `main.ts` solo bloques nuevos delimitados; no reordenar ni tocar lo ajeno.

## Verificación

`tsc --noEmit` + `vite build` en `desktop/ui`; relanzar Tauri dev
(`CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness`); comprobar visual:
barra 46px full-width, arrastre mueve la ventana, min/max/restore/cerrar
nativos, toggles espejo funcionan; commit solo archivos propios.

## DoD

Barra global visible y funcional + cabecera sin duplicados + build verde +
commit. El launcher (parte 2) queda para 089A-4.
