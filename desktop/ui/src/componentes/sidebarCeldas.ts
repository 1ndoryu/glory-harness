/* Celdas de conversación de la sidebar: fila `.conv`, menú contextual
 * (renombrar/archivar/copiar ID/lateral/eliminar) y renombrado inline. */
import type { Conversacion } from '../dominio/tipos';
import { icono } from './iconos';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from './menu';
import { el } from '../util/dom';
import { copiarAlPortapapeles } from '../util/portapapeles';

export interface CeldaConvDeps {
  onSeleccionar: (id: string) => void;
  onRenombrar: (id: string, titulo: string) => void;
  onArchivar: (id: string, archivada: boolean) => void;
  onEliminar: (id: string) => void;
  puedeAbrirLateral?: () => boolean;
  onAbrirEnLateral?: (id: string) => void;
  seleccionar: (id: string) => void;
  alternarArchivado: (id: string) => void;
  eliminar: (id: string) => void;
}

/** Convierte el título en un input inline para renombrar. */
export function empezarRenombrarConv(
  deps: CeldaConvDeps,
  conv: Conversacion,
  celda: HTMLDivElement,
): void {
  cerrarMenuActual();
  const t = celda.querySelector<HTMLElement>('.t');
  if (!t) return;
  const input = el('input') as HTMLInputElement;
  input.type = 'text';
  input.className = 't renombrar';
  input.value = conv.titulo;
  celda.replaceChild(input, t);

  const fin = (guardar: boolean) => {
    const nuevo = input.value.trim();
    if (guardar && nuevo && nuevo !== conv.titulo) {
      conv.titulo = nuevo;
      deps.onRenombrar(conv.id, nuevo);
    }
    const t2 = el('div', 't');
    t2.textContent = conv.titulo;
    if (input.parentNode === celda) celda.replaceChild(t2, input);
  };
  input.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') fin(true);
    else if (e.key === 'Escape') fin(false);
  });
  input.addEventListener('blur', () => fin(true));
  input.focus();
  input.select();
}

/** Crea una celda .conv con su menú contextual (misma mecánica que el modelo). */
export function crearCeldaConv(
  deps: CeldaConvDeps,
  conv: Conversacion,
  dentroDeProyecto = false,
): HTMLDivElement {
  const d = el(
    'div',
    'conv' +
      (conv.archivada ? ' archivada' : '') +
      (dentroDeProyecto ? ' conv-proyecto' : ''),
  );
  if (!conv.archivada && conv.seleccionada) d.classList.add('sel');
  d.dataset.id = conv.id;
  const t = el('div', 't');
  t.textContent = conv.titulo;
  d.appendChild(t);

  // botón "⋯" (visible solo al hover) para descubrir el menú sin clic derecho
  const btnMas = el('button', 'conv-mas') as HTMLButtonElement;
  btnMas.type = 'button';
  btnMas.title = 'acciones de conversación';
  btnMas.setAttribute('aria-label', 'acciones de conversación');
  btnMas.appendChild(icono('mas-horizontal', true));
  d.appendChild(btnMas);

  function abrirMenu(ancla: HTMLElement, e?: MouseEvent): void {
    // si se está renombrando esta fila, el menú no aplica
    const rect = ancla.getBoundingClientRect();
    abrirMenuContextual({
      rect: e
        ? new DOMRect(e.clientX, e.clientY, 0, 0)
        : new DOMRect(rect.left, rect.bottom, rect.width, 0),
      construir(m) {
        m.appendChild(
          crearItemMenu({
            texto: 'Cambiar nombre',
            onClick() {
              empezarRenombrarConv(deps, conv, d);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: conv.archivada ? 'Desarchivar' : 'Archivar',
            onClick() {
              deps.alternarArchivado(conv.id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Copiar ID',
            onClick() {
              cerrarMenuActual();
              void copiarAlPortapapeles(conv.id);
            },
          }),
        );
        // [039A-3 P5] "Abrir en panel lateral" (D4): solo se ofrece si el
        // orquestador lo permite (<2 chats y ancho suficiente).
        if (deps.puedeAbrirLateral?.() && deps.onAbrirEnLateral) {
          m.appendChild(crearSeparadorMenu());
          m.appendChild(
            crearItemMenu({
              texto: 'Abrir en panel lateral',
              onClick() {
                cerrarMenuActual();
                deps.onAbrirEnLateral?.(conv.id);
              },
            }),
          );
        }
        m.appendChild(crearSeparadorMenu());
        m.appendChild(
          crearItemMenu({
            texto: 'Eliminar',
            onClick() {
              deps.eliminar(conv.id);
            },
          }),
        );
      },
    });
  }

  // clic izquierdo = activar conversación (pero no en el botón ⋯)
  d.addEventListener('click', (e) => {
    if (e.target === btnMas) return;
    deps.onSeleccionar(conv.id);
    deps.seleccionar(conv.id);
  });
  // clic derecho sobre la fila = menú en el cursor
  d.addEventListener('contextmenu', (e) => {
    e.preventDefault();
    abrirMenu(d, e);
  });
  // botón ⋯ = menú bajo el botón
  btnMas.addEventListener('click', (e) => {
    e.stopPropagation();
    abrirMenu(btnMas);
  });

  return d;
}
