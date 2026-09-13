# Plan 129A-12 — Tema estilo Synara sin dañar los actuales

Fecha: 2026-09-12. Estado: activo. Roadmap: `129A-12`.

## Estado real verificado

- Tokens en `:root` (`desktop/ui/src/estilos/variables.css:16-40`: fondo,
  texto, borde, superficies, diff-adición/eliminación, tenue, foco, font,
  tamaños, espaciados). Tema oscuro = solo overrides
  `html[data-tema='oscuro']` (`estilos/tema.css`, 138 líneas, no toca
  geometría). Fuente Departure Mono; monocromo estricto declarado.
- Selector boolean `temaOscuro` (`dominio/opciones.ts:180`,
  `orquestador/entorno.ts:6 CLAVE_TEMA_OSCURO`, arranque en
  `orquestador/arranque.ts:148`, modal en `vistaModal.ts:216`): un tercer
  tema exige migrar boolean→enum.
- Referencia Synara (`synara/apps/web/src/theme/`): `THEME_SEED_CATALOG`
  por nombre con variantes light/dark: `{accent, contrast, fonts{code,ui},
  ink, surface, opaqueWindows, semanticColors{diffAdded, diffRemoved,
  skill}}` + `theme.logic.ts` (cero-contraste por defecto). Nombres
  existentes: `absolutely`, `ayu`, … (753 líneas de catálogo).

## Decisiones del usuario (12-09, registradas)

- Variante oscura del seed Synara; copia fiel (fuente del seed, local sin
  CDN; acentos + diffs en color); aplicado a toda la app.

## Estado

- F1–F4 implementados 12/13-09, pendientes de gate.
- F1: enum `tema` (claro/oscuro/synara) en `opciones.ts` + `CLAVE_TEMA`,
  `Tema`, `normalizarTema`, `aplicarTema` en `entorno.ts`; migración del
  boolean legacy en `arranque.ts` (reescribe best-effort).
- F2: `estilos/tema-synara.css` (fidelidad seed verificada: los 6 valores
  coinciden con catálogo `synara` variante `dark` del seed).
- F3: `vistaModal.ts` + `arranque.ts` migrados a `claveTema`/`aplicarTema`;
  `tsc` verde; `vite build` OK.
- F4 verificado con Chrome headless + CDP sobre la app real (scripts
  `C:\tmp\gh-tema-shots*.mjs`, PNGs en `C:\tmp\gh-tema-*.png`): fondos
  claro #fff / oscuro #111 / synara #0e0e0e; 6 tokens del seed computados;
  fixture `.conv.sel`/`.add`/`.del`/git-diff con los colores del seed; el
  select real de Apariencia conmuta a `data-tema=synara` de punta a punta.
  Límite: sin backend corriendo (aviso "el backend no arrancó" en las
  capturas); persistencia del tema solo verificada en código, no en arranque
  real con backend.

## Fases verificables

- F1: migrar `temaOscuro: boolean` → `tema: 'claro' | 'oscuro' | 'synara'`
  con migración de la preferencia guardada (`true→oscuro`, resto→claro);
  `data-tema` refleja el enum. Los tres valores pasan por el mismo camino.
- F2: `estilos/tema-synara.css` NUEVO, solo reglas
  `html[data-tema='synara']` mapeando un seed Synara (elegir variante dark o
  light en F2 y registrarlo) → tokens GH (`--fondo/--texto/--borde/
  --superficie-*/--texto-tenue/diff-*`). `variables.css` y `tema.css` NO se
  tocan (salvo importar el nuevo css). Fuente: la que diga el seed
  (si exige otra, se añade con `@font-face` local, sin CDN).
- F3: registrar en ajustes + verificación visual real (`tauri dev`) en
  1440×900, 1024×768 y 390×844: chat, Files, Cambios, ajustes, tarjeta de
  aprobación. Claro y oscuro deben quedar píxel-idénticos a antes
  (comparativa antes/después por pantalla).

## No alcance

- Catálogo multi-tema ni editor de temas: un solo tema nuevo. Retocar
  claro/oscuro: prohibido en esta tarea.

## DoD

- Tercer tema seleccionable con el estilo Synara; claro/oscuro intactos;
  fuentes locales; gate PASS + `tsc` EXIT 0.
