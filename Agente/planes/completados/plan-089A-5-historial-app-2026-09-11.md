# Plan 089A-5 — Historial de la app (atrás/adelante estilo Synara) — 2026-09-11

## Objetivo

Atrás/Adelante de la barra superior funcionales con la misma lógica que Synara
(`synara/apps/web/src/appNavigation.ts` + `AppNavigationButtons.tsx`): gobiernan la
**navegación de la app** (conversación + área de trabajo juntas), no el panel activo
ni las áreas por separado. Resuelve la decisión 089A-5(i) del roadmap.

## No alcance

- Historial interno del panel Navegador (ya existe: 109A-11 `panelNavegadorHistorial.ts`).
- Terminal en el launcher: **no aplica** — no existe panel Terminal en GH (tabs:
  `files`/`git`/`navegador`/`chat:<id>`; launcher: Files, Git local, Navegador, Chat
  lateral). La condición de la tarea («cuando exista ese panel») es falsa; no se
  construye un terminal dentro de este bloque.
- Persistir el historial entre sesiones (Synara tampoco lo persiste: es estado del
  history en memoria).

## Diseño

- Nuevo `desktop/ui/src/dominio/historialApp.ts` (puro, sin DOM): dos pilas como
  `panelNavegadorHistorial.ts` + `puedeAtras()`/`puedeAdelante()` para habilitar los
  botones sin mutar (Synara deriva `canGoBack`/`canGoForward` del estado).
  Entrada = `{ conversaId: string | null; proyectoRuta: string | null }`
  (`null` = borrador/sin área).
- Regla del vacío inicial: no se fabrica un destino de «atrás» desde el estado
  previo a la primera navegación (si no, Atrás quedaría habilitado apuntando a la
  nada al abrir la app). Una vez navegada, una entrada con `conversaId: null`
  (borrador de «nueva») sí se registra.
- Cableado en `orquestador/barraLateral.ts` (los dos puntos de navegación de la app
  viven ahí): `onSeleccionar` (conversación), «nueva» real (borrador),
  `onSeleccionarProyecto` (área). Internas `irAConversacion`/`irAProyecto` sin
  registro para que el restore no se auto-registre (sin flags de reentrancia).
- `VistaBarraDeps` gana `onAtras`/`onAdelante`; `BarraLateral` expone
  `irAtrasHistorial()`/`irAdelanteHistorial()`; `main.ts` los une (cierre de runtime,
  mismo patrón que `alternarSidebar`). Tras cada cambio: `barra.setPuedeNavegar`.
- Restore con área `null` (modo mock / estado transitorio): se omite la activación
  de área —no se puede resolver— y se restaura solo la conversación.

## Fases verificables

- F1: `dominio/historialApp.ts` + auto-prueba funcional (compilar el módulo suelto
  con tsc a `C:\tmp` y correr aserciones con node: registrar/atras/adelante,
  truncado del adelante, tope 50, regla del vacío, `puede*`).
- F2: cableado (tipos, barraLateral, vistaBarra, main) + `tsc --noEmit` + `vite build`.
- F3: gate `089A-5` PASS + commit + roadmap + completada.

## Gate y DoD

- Gate `sentinel check 089A-5 --stages scripts/quality/stages.json` PASS.
- DoD: botones habilitan/deshabilitan según haya atrás/adelante; Atrás restaura la
  visita anterior (conversación y/o área); navegar tras retroceder invalida el
  adelante; sin errores de tsc ni del build.
