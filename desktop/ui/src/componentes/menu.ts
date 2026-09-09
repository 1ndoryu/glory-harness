// Menú contextual compartido (.menu-ctx). Mecánica ÚNICA para
// todos los menús de la app (selector de modelo, modo de
// ejecución, acciones de conversación): ítems monocromo, anclado
// con vuelco al viewport y cierre por click fuera, Escape, resize
// o blur. NO reinventar esta mecánica por componente: usar
// crearItemMenu() para cada fila y abrirMenuContextual() para
// abrir. El menú se ancla a document.body (posición fixed) para
// no quedar recortado por contenedores con overflow.

import { icono } from './iconos';
import { cuerpo, el } from '../util/dom';
import {
  alDesenfocar,
  alRedimensionar,
  altoVentana,
  anchoVentana,
  finDesenfocar,
  finRedimension,
} from '../plataforma/ventana';

export interface ItemMenuOpciones {
  texto: string;
  /** check a la izquierda (elemento seleccionado). */
  marcado?: boolean;
  /** chevron a la derecha (sugiere submenú). */
  conFlecha?: boolean;
  /** Se invoca al pulsar el ítem (recibe el evento para feedback inline). */
  onClick?: (e: MouseEvent) => void;
}

/** Botón .menu-item con marca/etiqueta/flecha opcionales. */
export function crearItemMenu(opts: ItemMenuOpciones): HTMLButtonElement {
  const b = el('button', 'menu-item') as HTMLButtonElement;
  b.type = 'button';
  if (opts.marcado) {
    const mk = el('span', 'marca');
    mk.appendChild(icono('check', true));
    b.appendChild(mk);
  }
  const lbl = el('span', 'etiqueta');
  lbl.textContent = opts.texto;
  b.appendChild(lbl);
  if (opts.conFlecha) {
    const fl = el('span', 'flecha');
    fl.appendChild(icono('chevron-derecha', true));
    b.appendChild(fl);
  }
  if (opts.onClick) b.addEventListener('click', opts.onClick);
  return b;
}

/** Separador horizontal dentro del menú (p. ej. antes de una acción destructiva). */
export function crearSeparadorMenu(): HTMLElement {
  return el('div', 'menu-sep');
}

interface MenuVivo {
  cerrar(): void;
}
let vivo: MenuVivo | null = null;

/** Cierra el menú contextual abierto, si lo hay (unicidad global). */
export function cerrarMenuActual(): void {
  vivo?.cerrar();
}

/**
 * Abre un menú .menu-ctx anclado a `rect` (normalmente el rect del
 * disparador; para un clic derecho, un DOMRect de 0×0 en el cursor).
 * `construir` rellena el contenido (crearItemMenu, submenús...).
 *
 * - Cierra cualquier menú previo (solo hay uno abierto a la vez).
 * - Click fuera, Escape, resize y blur lo cierran y limpian listeners.
 * - El click DENTRO del menú NO lo cierra: cada ítem con acción llama a
 *   cerrarMenuActual() (o reabre) al ejecutarse; los submenús hover no cierran.
 */
export function abrirMenuContextual(opts: {
  rect: DOMRect;
  construir: (menu: HTMLElement) => void;
}): void {
  cerrarMenuActual();

  const m = el('div', 'menu-ctx');
  m.style.visibility = 'hidden'; // se mide y posiciona antes de pintar (sin parpadeo)
  opts.construir(m);
  cuerpo().appendChild(m);

  const margen = 8;
  const vw = anchoVentana();
  const vh = altoVentana();
  const altura = m.offsetHeight;
  const ancho = m.offsetWidth;

  // ---- posición con vuelco para no salirse de pantalla ----
  // Vertical: preferir bajo el ancla; si no cabe abajo y arriba sí, invertir.
  const espacioAbajo = vh - opts.rect.bottom - margen;
  let top =
    espacioAbajo < altura && opts.rect.top > altura + margen
      ? opts.rect.top - altura - 4
      : opts.rect.bottom + 4;
  if (top < margen) top = margen;
  if (top + altura > vh - margen) top = vh - altura - margen; // cota de seguridad
  m.style.top = top + 'px';

  let left = opts.rect.left;
  if (left + ancho > vw - margen) left = Math.max(margen, vw - ancho - margen);
  m.style.left = left + 'px';

  const quitar = () => {
    m.remove();
    document.removeEventListener('click', alClic, true);
    document.removeEventListener('keydown', alTecla, true);
    finRedimension(alRedimension);
    finDesenfocar(alPerderFoco);
    if (vivo === manejador) vivo = null;
  };
  const manejador: MenuVivo = { cerrar: quitar };
  vivo = manejador;

  function alClic(e: MouseEvent): void {
    if (m.contains(e.target as Node)) return; // clics dentro (ítems/submenús)
    quitar();
  }
  function alTecla(e: KeyboardEvent): void {
    if (e.key === 'Escape') quitar();
  }
  function alRedimension(): void {
    quitar();
  }
  function alPerderFoco(): void {
    quitar();
  }

  // capture para cerrar también clics que otros menús dejan pasar
  document.addEventListener('click', alClic, true);
  document.addEventListener('keydown', alTecla, true);
  alRedimensionar(alRedimension);
  alDesenfocar(alPerderFoco);

  m.style.visibility = '';
}
