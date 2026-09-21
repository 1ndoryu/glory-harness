# Chrome de paneles: botones icono y títulos (219A-1, 2026-09-21)

## Canon
- Botón icono de chrome: **28×28, sin borde, icono Lucide 16px**, hover
  `superficie-hover`, focus `outline 2px foco`, disabled `opacity .35` sin
  hover, active `superficie-active`. Es el patrón que ya tenía la barra
  superior (elegido por el usuario como canónico).
- Título de cabecera de panel: **`--sm`** (como el título de la conversación).
- Sin radius, sin sombras (monocromo estricto, `variables.css`).

## Construir (única vía para chrome)
- `componentes/chromePanel.ts`: `crearBotonIcono({icono, etiqueta, alPulsar?,
  activo?, invertidoHover?, deshabilitado?, id?, claseExtra?})` — siempre
  `type=button` + `title`/`aria-label`. Estilo en `estilos/botonIcono.css`.
- `crearCabeceraPanel({claseRaiz, titulo, medio?, acciones?})` — la raíz
  conserva la clase del panel (su layout); el título sale en `.chrome-titulo`
  (`--sm`).
- Variantes: `.activo` (toggle seleccionado; hoy sin cablear — el `.activo`
  del Navegador viejo estaba muerto) e `.invertido-hover` (⋯/× del chat,
  affordance 039A-3 conservada).
- `claseExtra` es solo hook posicional/espaciado (p. ej. `files-visor-lista`),
  nunca estilo de botón.

## No usa el constructor (a propósito)
- `.btn` (texto, `mensajes.css`), filas (`consola-fila`, `files-nombre`,
  `inicio-opcion`, `tab`), `.tab-x` (span 12px dentro de la tab), controles
  del composer (`.control`, `.pie-*`, `.msg-*`), botones de programación
  (`.conv-mas`, `.prog-*`), botonera de ventana (`ventana.ts`, patrón caption
  nativo 089A-3). Segunda pasada si se divergen.

## Cambios intencionales al migrar (no regresiones)
- Consola 24→28px, Files pierde el borde visible, Nav pierde el borde-hover y
  sus iconos pasan de 11px (`pequeno`) a 16px, tabs + pierde el dim idle
  (`opacity:.65`), cabecera ⋯/× 24→28px (icono ⋯ 11→16px), título Consola
  `--md`→`--sm`.
- `cerrarCaptura` del Navegador era botón de texto con clase de icono:
  ahora `.btn` canónico. `abrirCon`: normalizado `aria-label` con `…`.
- `alternarLista` (Files) gana `title`/`aria-label` (antes solo
  `aria-expanded`).

## Por qué no hay regla de gate que lo impida
VarSense SÍ corre en el gate (`sentinel analyze`), pero sus reglas son
léxicas: las 6 clases artesanales cumplían todas (tokens, sin hardcode, sin
huérfanas). Ningún tool compara coherencia entre ficheros. La defensa es este
constructor único + este spec. No se añade regla de "prohibir clases":
daría falsos positivos con hooks posicionales legítimos.

## Verificación 21-09 (cierre)
`tsc` EXIT 0, `vite build` OK (3.65 s), VarSense 15E/14W idéntico al baseline
(0 nuevos; los de la zona tocada son preexistentes: `--files-nivel` con
fallback + `setProperty`, `--sidebar-ancho` definido en `#cuerpo`, `todoProsa`
en `botonera`/`método` —"todo" dentro de "método"—, `rgba` en tema-synara,
`cssInlineScript` en `explorador.style.flex` del grip), grep de clases viejas
= 0 (salvo comentarios). **Visual en vivo verificada** (vite dev `:8760`):
18/18 `.boton-icono` con 28×28 + `border none` + `.ic` 16px, 0 malformados;
título Consola 11.05px (= `--sm`); 4 deshabilitados (atrás/adelante/matar/
abrirCon) a opacity 0.35; `cerrarCaptura` con clase `.btn`; 0 clases viejas
en el DOM. Navegador verificado abriendo su tab.
