/* Menú flotante de comandos `/` (plan 109A-4 F2).
 *
 * A diferencia de `.menu-ctx` (menú contextual de acciones, que cierra al
 * perder foco), este menú NO roba el foco: el usuario sigue escribiendo en el
 * textarea y las teclas las enruta el compositor con `alTecla()`. Por eso se
 * ancla con `position: fixed` al rect del textarea y no registra listeners
 * globales de teclado.
 *
 * Solo pinta: el filtrado vive en `dominio/comandosSlash.ts` y el efecto de
 * cada comando en `panelChatComandos.ts`.
 */

import { el, vaciar } from '../util/dom';
import { altoVentana, anchoVentana } from '../plataforma/ventana';
import { icono } from './iconos';
import { uso, type ComandoSlash } from '../dominio/comandosSlash';
import '../estilos/menuComandos.css';

export interface MenuComandosDeps {
  /** Elemento al que se ancla (el textarea del compositor). */
  ancla: HTMLElement;
  /** Se invoca al elegir un comando (clic o Enter/↑↓+Enter). */
  onElegir(comando: ComandoSlash): void;
}

export interface MenuComandosPanel {
  raiz: HTMLElement;
  abierto(): boolean;
  /** Repinta con la lista filtrada. Lista vacía → se cierra (nunca queda vacío). */
  mostrar(comandos: ComandoSlash[]): void;
  cerrar(): void;
  /** `true` si la tecla la consumió el menú (el compositor NO debe seguir). */
  alTecla(e: KeyboardEvent): boolean;
  destruir(): void;
}

export function crearMenuComandos(deps: MenuComandosDeps): MenuComandosPanel {
  const raiz = el('div', 'menu-cmd');
  raiz.hidden = true;
  raiz.setAttribute('role', 'listbox');
  raiz.setAttribute('aria-label', 'comandos disponibles');

  let comandos: ComandoSlash[] = [];
  let activo = 0;
  let filas: HTMLElement[] = [];

  /** Coloca el menú sobre el compositor (crece hacia arriba si no cabe abajo). */
  function posicionar(): void {
    const rect = deps.ancla.getBoundingClientRect();
    const margen = 8;
    const vw = anchoVentana();
    const vh = altoVentana();
    const alto = raiz.offsetHeight;
    const ancho = raiz.offsetWidth;
    // El compositor vive al fondo, así que casi siempre hay más sitio arriba.
    const espacioAbajo = vh - rect.bottom - margen;
    const top = alto <= espacioAbajo ? rect.bottom + 4 : Math.max(margen, rect.top - alto - 4);
    // Anclaje medido contra el rect del textarea y el viewport: no es diseño,
    // así que viaja como variable y la hoja lo aplica (ver menuComandos.css).
    raiz.style.setProperty('--menu-cmd-top', `${top}px`);
    let left = rect.left;
    if (left + ancho > vw - margen) left = Math.max(margen, vw - ancho - margen);
    raiz.style.setProperty('--menu-cmd-left', `${left}px`);
  }

  function pintar(): void {
    vaciar(raiz);
    filas = comandos.map((c, i) => fila(c, i));
    for (const f of filas) raiz.appendChild(f);
    resaltar(activo);
  }

  function fila(c: ComandoSlash, indice: number): HTMLElement {
    const b = el('button', 'menu-cmd-item');
    b.type = 'button';
    b.setAttribute('role', 'option');
    b.id = `menu-cmd-${c.nombre}`;
    if (c.origen === 'proyecto') b.classList.add('de-proyecto');
    const nombre = el('span', 'menu-cmd-nombre');
    nombre.textContent = uso(c);
    b.appendChild(nombre);
    const resumen = el('span', 'menu-cmd-resumen');
    resumen.textContent = c.resumen;
    b.appendChild(resumen);
    if (c.origen === 'proyecto') {
      const marca = el('span', 'menu-cmd-marca');
      marca.appendChild(icono('carpeta', true));
      b.appendChild(marca);
    }
    // Sin `mousedown`/blur: el foco se queda en el textarea.
    b.addEventListener('mousedown', (e) => e.preventDefault());
    b.addEventListener('click', () => elegir(indice));
    b.addEventListener('mouseenter', () => resaltar(indice));
    return b;
  }

  /** Mueve el resaltado ({-1,+1} cíclico) y lo hace visible en el scroll. */
  function mover(delta: number): void {
    if (comandos.length === 0) return;
    resaltar((activo + delta + comandos.length) % comandos.length);
    filas[activo]?.scrollIntoView({ block: 'nearest' });
  }

  function resaltar(indice: number): void {
    activo = indice;
    filas.forEach((f, i) => {
      f.classList.toggle('activo', i === indice);
      f.setAttribute('aria-selected', String(i === indice));
    });
  }

  function elegir(indice: number): void {
    const c = comandos[indice];
    cerrar();
    if (c) deps.onElegir(c);
  }

  function mostrar(lista: ComandoSlash[]): void {
    comandos = lista;
    if (lista.length === 0) {
      cerrar();
      return;
    }
    activo = 0;
    raiz.hidden = false;
    raiz.id = 'menu-cmd-lista';
    pintar();
    posicionar();
    deps.ancla.setAttribute('aria-expanded', 'true');
    deps.ancla.setAttribute('aria-controls', raiz.id);
  }

  function cerrar(): void {
    if (raiz.hidden) return;
    raiz.hidden = true;
    comandos = [];
    filas = [];
    deps.ancla.setAttribute('aria-expanded', 'false');
  }

  function alTecla(e: KeyboardEvent): boolean {
    if (raiz.hidden) return false;
    switch (e.key) {
      case 'ArrowDown':
        e.preventDefault();
        mover(1);
        return true;
      case 'ArrowUp':
        e.preventDefault();
        mover(-1);
        return true;
      case 'Tab':
        e.preventDefault();
        mover(e.shiftKey ? -1 : 1);
        return true;
      case 'Enter':
        e.preventDefault();
        elegir(activo);
        return true;
      case 'Escape':
        e.preventDefault();
        cerrar();
        return true;
      default:
        return false;
    }
  }

  return {
    raiz,
    abierto: () => !raiz.hidden,
    mostrar,
    cerrar,
    alTecla,
    destruir() {
      cerrar();
      raiz.remove();
    },
  };
}
