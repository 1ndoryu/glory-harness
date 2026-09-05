// ============================================================
// Cabecera del chat (título de la conversación activa + acciones).
// [039A-3 P4] Añade el botón ⋯ (menú de acciones de la conversación,
// reutilizando menu.ts) y el botón de colapsar/expandir la sidebar
// junto al título. El título puede entrar en edición inline (renombrar
// desde el ⋯) igual que una fila de la sidebar.
// ============================================================

import { icono } from './iconos';
import { el } from '../util/dom';

export interface CabeceraChat {
  raiz: HTMLElement;
  /** Cambia el título mostrado. */
  ponerTitulo(texto: string): void;
  /** [039A-3 P4] Refleja el estado abierto/colapsado de la sidebar en el
   * icono del botón toggle (panel-izq-cerrar ↔ panel-izq-abrir). */
  setSidebarAbierta(abierta: boolean): void;
  /** [039A-3 P4] Pone el título en edición inline (renombrar). Se usa desde
   * el ⋯ de la cabecera; al guardar llama `onGuardar(nuevo)`. */
  empezarRenombrar(
    titulo: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void;
}

export interface CabeceraChatOpciones {
  titulo: string;
  /** Se invoca al pulsar el botón ⋯: abre el menú de acciones de la
   * conversación activa (el ⋯ solo dispara; main.ts construye el menú para
   * tener acceso a la conversación actual). */
  onAcciones: (rect: DOMRect) => void;
  /** Se invoca al pulsar el botón de colapsar/expandir la sidebar. */
  onToggleSidebar: () => void;
}

/** Mecánica de renombrar inline compartida (fila de sidebar y cabecera). */
export function montarCabeceraChat(opts: CabeceraChatOpciones): CabeceraChat {
  const cab = el('div');
  cab.id = 'cabecera-chat';
  let titulo = opts.titulo;

  // ---- acciones: colapsar sidebar + ⋯ (acciones de la conversación) ----
  const acciones = el('div', 'acciones-cabecera');
  acciones.setAttribute('aria-label', 'acciones de la conversación');

  const btnToggle = el('button', 'cab-boton') as HTMLButtonElement;
  btnToggle.id = 'btn-abrir-sidebar';
  btnToggle.type = 'button';
  btnToggle.title = 'ocultar lista de conversaciones';
  btnToggle.setAttribute('aria-label', 'ocultar lista de conversaciones');
  let sidebarAbierta = true;
  function pintarIconoToggle(): void {
    btnToggle.replaceChildren(icono(sidebarAbierta ? 'panel-izq-cerrar' : 'panel-izq-abrir'));
  }
  pintarIconoToggle();
  btnToggle.addEventListener('click', () => opts.onToggleSidebar());
  acciones.appendChild(btnToggle);

  const btnMas = el('button', 'cab-boton') as HTMLButtonElement;
  btnMas.id = 'btn-acciones-chat';
  btnMas.type = 'button';
  btnMas.title = 'acciones de la conversación';
  btnMas.setAttribute('aria-label', 'acciones de la conversación');
  btnMas.textContent = '⋯';
  btnMas.addEventListener('click', (e) => {
    e.stopPropagation();
    opts.onAcciones(btnMas.getBoundingClientRect());
  });
  acciones.appendChild(btnMas);

  // ---- título (editado inline al renombrar) ----
  const t = el('span', 'titulo');
  t.textContent = titulo;
  t.title = titulo;

  let editando = false;
  function ponerTitulo(texto: string): void {
    titulo = texto;
    if (!editando) {
      t.textContent = texto;
      t.title = texto;
    }
  }

  function empezarRenombrar(
    valor: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void {
    if (editando) return;
    editando = true;
    const input = el('input') as HTMLInputElement;
    input.type = 'text';
    input.className = 'titulo-renombrar';
    input.value = valor;
    input.setAttribute('aria-label', 'renombrar conversación');
    cab.replaceChild(input, t);

    const fin = (guardar: boolean) => {
      if (!editando) return;
      editando = false;
      const nuevo = input.value.trim();
      if (guardar && nuevo && nuevo !== titulo) {
        titulo = nuevo;
        onGuardar(nuevo);
      } else if (!guardar) {
        onCancelar?.();
      }
      t.textContent = titulo;
      t.title = titulo;
      if (input.parentNode === cab) cab.replaceChild(t, input);
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

  cab.appendChild(acciones);
  cab.appendChild(t);

  return {
    raiz: cab,
    ponerTitulo(texto: string) {
      ponerTitulo(texto);
    },
    setSidebarAbierta(abierta: boolean) {
      sidebarAbierta = abierta;
      pintarIconoToggle();
      const label = abierta ? 'ocultar lista de conversaciones' : 'mostrar lista de conversaciones';
      btnToggle.title = label;
      btnToggle.setAttribute('aria-label', label);
    },
    empezarRenombrar(valor, onGuardar, onCancelar) {
      empezarRenombrar(valor, onGuardar, onCancelar);
    },
  };
}
