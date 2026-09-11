# Plan 109A-7 — Sacar los estilos inline del TS (`cssInlineScript`)

**Tarea:** 109A-7 · **Fecha:** 2026-09-11 · **Estado:** cerrado (11-09)
**Regla de medida:** VarSense 2.2.1 @ `88f281f`, `scan --workspace . --format json`.
**Gate de cierre:** `scripts/quality/stages.json` (Sentinel 0.7.8 @ `1587c59`). Ojo: el gate de
este repo **no ejecuta VarSense**, así que el 0 de `cssInlineScript` se demuestra con la medición
directa del binario, no con el reporte del gate.

## 1. Objetivo

Dejar en **0** los 52 hallazgos `cssInlineScript` de `desktop/ui/src/**`, sin cambiar comportamiento
visible, sustituyendo la escritura de estilo desde TS por **clases CSS + variables del sistema**.

## 2. Alcance

Los 13 ficheros y 52 líneas medidos el 10-09 22:20 (tras 109A-6, el reparto no cambió):

| Fichero | N | Líneas |
|---|---|---|
| `componentes/panelMeta.ts` | 8 | 126, 151, 155, 156, 158, 159, 162, 163 |
| `componentes/anotaciones.ts` | 7 | 52, 53, 54, 55, 63, 64, 89 |
| `componentes/selectorModelo.ts` | 7 | 108, 109, 110, 114, 117, 125, 126 |
| `componentes/entradaBarras.ts` | 5 | 154, 159, 160, 162, 163 |
| `componentes/entradaContexto.ts` | 4 | 148, 151, 179, 189 |
| `componentes/menu.ts` | 4 | 83, 102, 106, 139 |
| `plataforma/webview.ts` | 4 | 21, 22, 23 (×2) |
| `componentes/modalProyecto.ts` | 3 | 66, 94, 109 |
| `componentes/panelNavegador.ts` | 3 | 37, 161, 169 |
| `componentes/menuComandos.ts` | 2 | 57, 60 |
| `componentes/panelNavegadorControles.ts` | 2 | 46, 95 |
| `util/portapapeles.ts` | 2 | 20, 21 |
| `componentes/panelChat.ts` | 1 | 110 |

CSS a tocar (por fichero, sin crear hojas nuevas salvo lo indicado en fases):
`panelMeta.css`, `entrada.css`, `navegador.css`, `modalProyecto.css`, `menuComandos.css`, `layout.css`.

**No alcance:** `109A-8` (CSS muerto), `109A-9` (`console-production`), `109A-10` (falsos positivos
del medidor), la etapa `sccache` del gate y cualquier cambio de comportamiento, de layout o de
tokens. No se introducen excepciones `sentinel-disable`.

## 3. Criterio técnico (por qué esta forma y no otra)

La regla `cssInlineScript` de VarSense (`src/core/analyzeDocument.ts:38`) marca tres formas:
`.style.<prop> =`, `.style.setProperty(` y `.setAttribute('style'`. **Exceptúa**
`style.setProperty('--<variable>', …)`, que es justo el patrón que pide el proyecto (clase CSS +
variable del sistema). De ahí el reparto:

1. **Estado discreto** (mostrar/ocultar, expandido, voltear, recortado) → **clase CSS**.
   El JS solo alterna clases; la presentación vive en la hoja.
2. **Valor medido en tiempo de ejecución** (posición del menú contextual, alto del textarea,
   reserva del scroll, proporción del canvas) → **variable CSS** publicada con `setProperty('--x')`.
   El número sigue calculándose en TS (depende de `getBoundingClientRect`/`scrollHeight`, no del
   diseño), pero el navegador lo aplica desde la hoja. Es la vía que la propia regla sanciona.

Consecuencia: el roadmap daba por «excepción justificada con `sentinel-disable`» el ocultar-antes-de-
medir de `menu.ts`/`menuComandos.ts` y el textarea fuera de pantalla de `portapapeles.ts`. Los tres
se resuelven con clase CSS y **no necesitan excepción**; si al cerrar no queda ninguna, el plan lo
registra como corrección del corto alcance previsto, no como ampliación.

## 4. Fases

- **F1 — Menús y overlays con valor medido** (`menu.ts`, `entradaContexto.ts`, `menuComandos.ts`,
  `selectorModelo.ts`): `visibility:hidden` base + `.visible` al terminar de medir; `top`/`left` como
  `var(--*-top/left, auto)` con respaldo `auto` (idéntico al layout de hoy antes de posicionar);
  el volteo del submenú pasa de dos escrituras de estilo a la clase `.abajo`, y
  `g.style.position='relative'` desaparece porque `.menu-ctx .menu-grupo` ya lo declara.
  *Verificación:* abrir menú de modelo (barra y modal), submenú con volteo, menú de comandos `/`,
  detalle del contexto — en navegador, con la mecánica de un solo `page.evaluate` por flujo
  (el menú muere en `window` blur entre llamadas).
- **F2 — Paneles y visibilidad** (`panelNavegadorControles.ts`, `panelNavegador.ts`, `modalProyecto.ts`,
  `anotaciones.ts`): `display:none` por defecto en la hoja y `.visible` para mostrar; el canvas de
  anotaciones estrena su regla (`.nav-anotaciones` no existía en CSS: dependía al 100% del inline) y
  la proporción pasa a `var(--anotaciones-aspecto)`.
  *Verificación:* panel del navegador oculto al arrancar y visible al activarlo, vista de captura,
  nota de ruta del modal en modo web, canvas de anotaciones con su proporción real.
- **F3 — Alto y reservas** (`entradaBarras.ts`, `panelMeta.ts`, `panelChat.ts`): `--entrada-alto`,
  `--pm-alto` y `--entrada-reserva` (esta ya estaba declarada en `layout.css` y el inline la
  pisaba). El tope de líneas sigue calculándose en TS, junto a la medición de `line-height`.
  *Verificación:* el compositor crece hasta 5 líneas y luego hace scroll; el panel meta crece a 3
  líneas al enfocar y hace scroll; los mensajes no quedan tapados por la entrada al crecer.
- **F4 — Boundary de la webview y portapapeles** (`plataforma/webview.ts`, `util/portapapeles.ts`):
  el resaltado remoto pasa a inyectar una regla en un `<style id="gh-resaltar-estilo">` y alternar la
  clase `gh-resaltar` (mismo patrón que ya usa `scriptSeleccionActivar`, y sin dejar el `outline`
  inline mutado en la página visitada); el textarea del fallback de copia usa `.copia-temporal`.
  *Verificación:* type-check + build; el resaltado solo es ejercitable en Tauri con página viva
  (limitación a registrar si no se puede reproducir en modo web).

## 5. Definition of Done

1. `scan` de VarSense con **0** `cssInlineScript` (y sin hallazgos nuevos de otras reglas).
2. `npm run build` del UI (que incluye `tsc --noEmit`) en verde.
3. Verificación visual en navegador de los flujos de F1, F2 y F3 (los tres bloques con efecto).
4. Gate canónico `check 109A-7` sin errores nuevos respecto al corte de 109A-6
   (`coverage` PASS, `sentinel` PASS 0 errores / 8 warnings / 1 info; `sccache` sigue FAIL por la
   tarea independiente del gate).
5. Ninguna excepción `sentinel-disable` añadida.
6. Roadmap actualizado (109A-7 fuera), evidencia en `Agente/completados/`, plan a `completados/`.

## 6. Riesgos

- **Medir sin pintar.** Las clases base `visibility:hidden`/`display:none` deben quitarse *después*
  de posicionar; si se quitan antes, reaparece el parpadeo del menú en la esquina.
- **Regresión de layout por orden de cascada.** `.proyecto-modal-nota` tiene que declararse después
  de `.proyecto-modal-label` (`display:flex`) para ganarle.
- **`.panel-navegador`** pasa a estar oculto por defecto en la hoja: cualquier ruta que lo mostrara
  sin pasar por `mostrar()` quedaría invisible. Verificado que el único punto que lo muestra es
  `mostrar()`.
- **Medición temprana** (lección de `ajustarEntrada`): el primer cálculo de alto sigue dependiendo de
  que el elemento esté montado; no se cambia ese contrato (`entrada.medir()` desde `main.ts`).

## 7. Cierre (11-09)

Las cuatro fases se ejecutaron en un solo bloque y los seis puntos del DoD quedan verificados:

1. `scan` de VarSense: **0** `cssInlineScript` y `severityCounts` `{error:0, warning:0,
   information:0, hint:0}`; `orphan-classes` sin nombres nuevos huérfanos.
2. `tsc --noEmit` y `vite build` en verde (165.05 kB de JS, 51.05 kB de CSS).
3. Flujos de F1, F2 y F3 verificados en navegador con estilos computados; además la ruta de reserva
   del portapapeles (F4) forzando el fallo de `navigator.clipboard`.
4. `sentinel check 109A-7`: `coverage` PASS · `sentinel` PASS **0 errores, 8 warnings, 1 info**
   (idéntico al corte de 109A-6) · `sccache` FAIL por la tarea independiente del gate.
5. **Cero** `sentinel-disable`: no hizo falta ninguna excepción.
6. Roadmap sin 109A-7, evidencia en `Agente/completados/tareas-2026-09-11.md`, plan archivado aquí.

**Corrección del corto alcance previsto:** el roadmap daba por candidatos a `sentinel-disable` el
ocultar-antes-de-medir de `menu.ts`/`menuComandos.ts` y el textarea de `portapapeles.ts`; los tres se
resolvieron con `visibility:hidden` + clase `.visible` y con `.copia-temporal`, así que no hubo
excepción alguna.

**Límite declarado:** `anotaciones.ts` (módulo huérfano) y `codigoResaltar` (`plataforma/webview.ts`,
que solo se ejerce por CDP con Tauri) quedan tree-shaken y no llegan al bundle, así que no se
verificaron en runtime; del segundo se comprobó el script generado contra un DOM mínimo. Detalle en
el registro de completadas.
