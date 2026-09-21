# Plan 219A-1 — Botones icono y títulos centralizados (2026-09-21)

## Objetivo
Eliminar las 6 construcciones artesanales de botón-icono (cada panel con su
clase, tamaño y hover) y unificar títulos de panel en `--sm`, con un único
constructor + una única hoja de estilo. Canon elegido por el usuario: el de la
barra superior (**28×28 sin borde**). Alcance de migración: TODOS los botones
de chrome (barra, cabecera del chat, tabs, Consola, Files, Navegador).

## Por qué el gate no lo vio (verificado 21-09)
- VarSense SÍ corre dentro del gate (`sentinel analyze`, etapa `sentinel` en
  `scripts/quality/stages.json`, `analyzers.varsense.enabled: true` en
  `sentinel.config.json`). No es un problema de configuración.
- Sus reglas son léxicas (tokens, hardcode, clases huérfanas, inline, prosa):
  las clases artesanales cumplen todas → 0 hallazgos. Ningún tool del gate
  evalúa coherencia visual entre ficheros. El fix es arquitectónico (este
  plan), no de reglas. Por eso NO se añade regla "prohibir clases
  `*-accion`": daría falsos positivos (hooks posicionales legítimos como
  `.files-visor-lista`).

## Inventario (verificado 21-09)
| Sitio | Clase actual | Caja / borde / icono | Destino |
|---|---|---|---|
| barra superior (`barraSuperior.ts:47`, css:63) | `.barra-boton` | 28×28, sin borde, `.ic` 16px | canon → migrar a compartido |
| cabecera chat (`cabecera.ts:75,85`, `layout.css:341`) | `.cab-boton` (+`.cab-cerrar`) | 24×24, sin borde, hover INVERTIDO | compartido + variante `invertido-hover` (affordance 039A-3) |
| tabs (`panelDerecho.ts`, `tabs.css:44`) | `.tab-mas` | 30px, sin borde, `opacity:.65` idle | compartido (se pierde el dim idle; intencional) |
| Consola (`panelConsola.ts`, `consola.css:35`) | `.consola-accion` | 24×24, borde transparente + borde en hover | compartido |
| Files (`panelFiles.ts`, `files.css:12`) | `.files-accion` | 24×24, borde visible (salvo 3 con `border:0`) | compartido; se borran los `border:0` |
| Navegador (`panelNavegadorControles.ts`, `navegador.css:63`) | `.nav-btn` | transparente, borde en hover, estado `.activo` | compartido + `.activo` |

Títulos: `.consola-titulo --md` → `--sm` (= conversación); Cambios `--xs` →
`--sm`; Files sin título (no se añade: cambia layout, fuera de alcance).

## Fases
- **F1 — Compartido.** `componentes/chromePanel.ts`: `crearBotonIcono({icono,
  etiqueta, alPulsar, activo?, invertidoHover?, deshabilitado?, id?})` (siempre
  `type=button`, `title`+`aria-label`, icono 16px vía `icono(nombre)`).
  `estilos/botonIcono.css`: `.boton-icono` 28×28 sin borde, `.ic` 16px,
  hover/focus-visible/disabled (patrón barra), `.activo` (seleccionado +
  invertido), `.invertido-hover:hover`, `[hidden]{display:none}`. Import en
  `index.css`.
- **F2 — Migración.** Reescribir los 6 sitios con el constructor; borrar las 6
  reglas viejas; retocar `layout.css:341-377` (selectores `.cab-boton` →
  `.cabecera-chat .boton-icono`, conservar hack `[hidden]` P6b + margen del ×
  como regla posicional) y `tema.css:64,89` + `tema-synara.css:80,108`
  (mismo renombre). Conservar hooks posicionales (`.files-visor-lista`).
  Fuera de alcance (segunda pasada, anotar en spec): botones de contenido
  (`.msg-mas`, `.pie-copiar`, `.control`, `.conv-mas`, `.prog-*`, `.tab-x` si
  existe como tal, botonera de ventana `ventana.ts` — patrón caption nativo
  089A-3).
- **F3 — Spec + verificación.** Spec en
  `Agente/documentacion/ui-chrome-botones-titulos-2026-09-21.md` (canon,
  constructor, variantes, qué queda fuera). Verificación: `type-check`,
  `build`, VarSense UI (0 errores nuevos), visual en vivo (vite + captura de
  los 6 sitios). Commit con ID `219A-1` (incluye el lote visual Consola sin
  commit del 20-09).

## DoD
`crearBotonIcono` es la única forma de construir un botón icono de chrome;
`grep` de `consola-accion|files-accion|nav-btn|tab-mas|barra-boton|cab-boton|cab-cerrar`
en `desktop/ui/src` = 0 (salvo menciones en spec/completados); títulos de
panel en `--sm`; build + type-check verdes; sin regresión visual en vivo.
