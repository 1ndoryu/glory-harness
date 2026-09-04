// ============================================================
// Sidebar: botones de acción + lista de conversaciones + pie
// (configuración). Port 1:1 del mockup. La selección de
// conversación es visual; las acciones del menú contextual
// (cambiar nombre / archivar / copiar ID / eliminar) mutan el
// estado local y notifican al llamador (sin backend aún). El
// menú contextual REUTILIZA la mecánica compartida de menu.ts
// (la misma del selector de modelo y del modo de ejecución).
// ============================================================

import type { Conversacion } from '../dominio/tipos';
import { icono } from './iconos';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from './menu';
import { el } from '../util/dom';

export interface Sidebar {
  raiz: HTMLElement;
  /** Marca una conversación como seleccionada (visual). */
  seleccionar(id: string): void;
  /**
   * Sustituye la lista completa (p. ej. tras listar el backend): repinta y
   * conserva la selección si el id sigue existiendo.
   */
  sustituir(conversaciones: Conversacion[]): void;
}

/** Acciones de los botones superiores del nav. */
export type AccionNav = 'nueva' | 'agente' | 'flujo' | 'complementos';

export interface SidebarOpciones {
  conversaciones: Conversacion[];
  /** Al pulsar una conversación (activa esa conversación). */
  onSeleccionar: (id: string) => void;
  /** Se invoca tras cambiar el nombre de una conversación. */
  onRenombrar: (id: string, titulo: string) => void;
  /** Se invoca al archivar (id, y si se desarchiva). */
  onArchivar: (id: string, archivada: boolean) => void;
  /** Se invoca al eliminar una conversación. */
  onEliminar: (id: string) => void;
  /** Al pulsar un botón superior del nav (nueva/agentes/flujo/complementos). */
  onAccionNav?: (accion: AccionNav) => void;
  abrirConfig: () => void;
}

export function montarSidebar(opts: SidebarOpciones): Sidebar {
  const aside = el('aside');
  aside.id = 'sidebar';
  const conversaciones: Conversacion[] = [...opts.conversaciones];

  // ---- botones de acción (nav) ----
  const nav = el('nav', 'nav-botones');
  nav.setAttribute('aria-label', 'crear y tipos de conversación');

  function botonNav(
    tooltip: string,
    etiqueta: string,
    iconoNombre: 'nueva' | 'agente' | 'flujo' | 'complementos',
    onClick?: () => void,
  ): HTMLButtonElement {
    const b = el('button', 'nav-boton') as HTMLButtonElement;
    b.type = 'button';
    b.title = tooltip;
    b.appendChild(icono(iconoNombre, true));
    const span = el('span');
    span.textContent = etiqueta;
    b.appendChild(span);
    if (onClick) b.addEventListener('click', onClick);
    return b;
  }

  nav.appendChild(
    botonNav('nueva conversación', 'Nueva conversación', 'nueva', () =>
      opts.onAccionNav?.('nueva'),
    ),
  );
  nav.appendChild(
    botonNav('agentes', 'Agentes', 'agente', () => opts.onAccionNav?.('agente')),
  );
  nav.appendChild(
    botonNav('flujo', 'Flujo', 'flujo', () => opts.onAccionNav?.('flujo')),
  );
  nav.appendChild(
    botonNav('complementos', 'Complementos', 'complementos', () =>
      opts.onAccionNav?.('complementos'),
    ),
  );

  // ---- lista de conversaciones (activas + sección archivadas) ----
  const lista = el('div');
  lista.id = 'lista-conversaciones';

  const celdas = new Map<string, HTMLDivElement>();
  let activaId: string | null = null;
  // `conversaciones` es una copia local mutable que la sidebar reordena
  // (activar/archivar/eliminar); el llamador recibe los cambios por callbacks.

  function seleccionar(id: string): void {
    activaId = id;
    celdas.forEach((celda, clave) => {
      celda.classList.toggle('sel', clave === id);
    });
  }

  /** Convierte el título en un input inline para renombrar. */
  function empezarRenombrar(conv: Conversacion, celda: HTMLDivElement): void {
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
        opts.onRenombrar(conv.id, nuevo);
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
  function crearCelda(conv: Conversacion): HTMLDivElement {
    const d = el('div', 'conv' + (conv.archivada ? ' archivada' : ''));
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
    btnMas.innerHTML = '⋯';
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
                empezarRenombrar(conv, d);
              },
            }),
          );
          m.appendChild(
            crearItemMenu({
              texto: conv.archivada ? 'Desarchivar' : 'Archivar',
              onClick() {
                conv.archivada = !conv.archivada;
                opts.onArchivar(conv.id, conv.archivada);
                pintarLista();
                cerrarMenuActual();
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
          m.appendChild(crearSeparadorMenu());
          m.appendChild(
            crearItemMenu({
              texto: 'Eliminar',
              onClick() {
                celdas.delete(conv.id);
                conversaciones.splice(conversaciones.indexOf(conv), 1);
                opts.onEliminar(conv.id);
                pintarLista();
                cerrarMenuActual();
              },
            }),
          );
        },
      });
    }

    // clic izquierdo = activar conversación (pero no en el botón ⋯)
    d.addEventListener('click', (e) => {
      if (e.target === btnMas) return;
      opts.onSeleccionar(conv.id);
      seleccionar(conv.id);
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

  function pintarLista(): void {
    lista.replaceChildren();
    celdas.clear(); // los nodos viejos salieron del DOM; no dejar referencias muertas
    const activas = conversaciones.filter((c) => !c.archivada);
    const archivadas = conversaciones.filter((c) => c.archivada);

    activas.forEach((c) => {
      const d = crearCelda(c);
      celdas.set(c.id, d);
      lista.appendChild(d);
    });
    if (archivadas.length > 0) {
      const sep = el('div', 'conv-sep');
      sep.textContent = 'Archivadas';
      lista.appendChild(sep);
      archivadas.forEach((c) => {
        const d = crearCelda(c);
        celdas.set(c.id, d);
        lista.appendChild(d);
      });
    }
    // reaplica la selección visual tras repintar
    if (activaId) seleccionar(activaId);
  }

  pintarLista();

  // ---- pie: configuración ----
  const pie = el('div', 'pie');
  const bConfig = el('button', 'pie-boton') as HTMLButtonElement;
  bConfig.id = 'btn-config';
  bConfig.type = 'button';
  bConfig.title = 'abrir configuración';
  bConfig.appendChild(icono('ajustes', true));
  const spanCfg = el('span');
  spanCfg.textContent = 'Configuración';
  bConfig.appendChild(spanCfg);
  bConfig.addEventListener('click', opts.abrirConfig);
  pie.appendChild(bConfig);

  aside.appendChild(nav);
  aside.appendChild(lista);
  aside.appendChild(pie);

  return {
    raiz: aside,
    seleccionar(id: string) {
      seleccionar(id);
    },
    sustituir(nuevas: Conversacion[]) {
      conversaciones.splice(0, conversaciones.length, ...nuevas.map((c) => ({ ...c })));
      if (activaId && !conversaciones.some((c) => c.id === activaId)) activaId = null;
      pintarLista();
    },
  };
}

/** Copia texto al portapapeles (fallback a execCommand si no hay API). */
async function copiarAlPortapapeles(texto: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(texto);
      return;
    }
  } catch {
    // fallthrough al fallback
  }
  // fallback para contextos no seguros / permisos denegados
  const ta = el('textarea') as HTMLTextAreaElement;
  ta.value = texto;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  document.execCommand?.('copy');
  ta.remove();
}
