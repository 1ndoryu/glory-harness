// Chrome de paneles (219A-1): constructor ÚNICO de botones icono del chrome
// (barra superior, cabeceras de panel/chat, tabs). Antes cada panel construía
// el suyo a mano (`.barra-boton`, `.cab-boton`, `.tab-mas`,
// `.consola-accion`, `.files-accion`, `.nav-btn`) con tamaños y hovers
// distintos que ningún tool del gate detectaba. El canon es el de la barra:
// 28×28 sin borde, icono Lucide 16px (ver `estilos/botonIcono.css`).
// Los botones de CONTENIDO (filas, `.btn` de texto, `.tab-x` dentro de tabs,
// controles del composer, botonera de ventana nativa) NO usan este
// constructor: tienen affordances propias (ver spec en
// `Agente/documentacion/ui-chrome-botones-titulos-2026-09-21.md`).

import '../estilos/botonIcono.css';
import type { IconoNombre } from '../dominio/tipos';
import { icono } from './iconos';
import { el } from '../util/dom';

export interface BotonIconoOpciones {
  /** Icono Lucide (`iconos.ts`). Siempre a 16px por el CSS compartido. */
  icono: IconoNombre;
  /** Etiqueta: va a `title` y `aria-label` (nunca hay botón sin nombre). */
  etiqueta: string;
  /** Click. Opcional: el Navegador cablea sus botones desde fuera. */
  alPulsar?: (e: MouseEvent) => void;
  /** Modo seleccionado permanente (toggles). Sin uso actual; el estado
   * `.activo` heredado del Navegador estaba sin cablear y se conserva como
   * variante documentada, no como comportamiento. */
  activo?: boolean;
  /** Hover invertido (⋯/× de la cabecera del chat, affordance 039A-3). */
  invertidoHover?: boolean;
  /** Nace deshabilitado (atrás/adelante, abrir-con, matar-consola). */
  deshabilitado?: boolean;
  /** Id interno (cabecera del chat, una instancia por panel). */
  id?: string;
  /** Hook posicional extra (p. ej. `files-visor-lista`): solo espaciado o
   * layout, nunca estilo de botón. */
  claseExtra?: string;
}

/** Botón icono canónico: `type=button`, nombre accesible y clase única. */
export function crearBotonIcono(opts: BotonIconoOpciones): HTMLButtonElement {
  const btn = el(
    'button',
    'boton-icono' + (opts.claseExtra ? ' ' + opts.claseExtra : ''),
  );
  btn.type = 'button';
  btn.title = opts.etiqueta;
  btn.setAttribute('aria-label', opts.etiqueta);
  if (opts.id) btn.id = opts.id;
  if (opts.activo) btn.classList.add('activo');
  if (opts.invertidoHover) btn.classList.add('invertido-hover');
  if (opts.deshabilitado) btn.disabled = true;
  btn.appendChild(icono(opts.icono));
  if (opts.alPulsar) btn.addEventListener('click', opts.alPulsar);
  return btn;
}

export interface CabeceraPanelOpciones {
  /** Clase raíz propia del panel (`consola-cabecera`, …): layout y borde los
   * pone el panel; el título siempre va en `--sm` vía `.chrome-titulo`. */
  claseRaiz: string;
  titulo: string;
  /** Nodos entre el título y las acciones (p. ej. el contador de Consola). */
  medio?: HTMLElement[];
  /** Botones icono (construidos con `crearBotonIcono`). */
  acciones: HTMLElement[];
}

/** Cabecera de panel: título en `--sm` + acciones. La raíz conserva la clase
 * del panel para no cambiar su layout. */
export function crearCabeceraPanel(opts: CabeceraPanelOpciones): {
  raiz: HTMLElement;
  titulo: HTMLElement;
} {
  const raiz = el('div', opts.claseRaiz);
  const t = el('span', 'chrome-titulo');
  t.textContent = opts.titulo;
  raiz.appendChild(t);
  for (const m of opts.medio ?? []) raiz.appendChild(m);
  for (const a of opts.acciones) raiz.appendChild(a);
  return { raiz, titulo: t };
}
