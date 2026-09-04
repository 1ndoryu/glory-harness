// ============================================================
// Cabecera del chat (título de la conversación activa).
// Port 1:1 del mockup.
// ============================================================

import { el } from '../util/dom';

export interface CabeceraChat {
  raiz: HTMLElement;
  /** Cambia el título mostrado. */
  ponerTitulo(texto: string): void;
}

export function montarCabeceraChat(titulo: string): CabeceraChat {
  const cab = el('div');
  cab.id = 'cabecera-chat';
  const t = el('span', 'titulo');
  t.textContent = titulo;
  cab.appendChild(t);
  return {
    raiz: cab,
    ponerTitulo(texto: string) {
      t.textContent = texto;
    },
  };
}
