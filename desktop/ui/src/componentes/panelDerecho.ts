// ============================================================
// Panel derecho con tabs (089A-2, referencia Paseo): Chat lateral,
// Navegador y Visor conviven como pestañas en vez de excluirse.
// El contenido de cada tab es el nodo vivo del componente (se oculta
// con `hidden`, no se destruye). El × de la barra cierra todo el
// panel (el orquestador desmonta cada contenido).
// ============================================================

import '../estilos/tabs.css';
import { icono } from './iconos';
import { el } from '../util/dom';

export type TabDerechaId = 'chat' | 'navegador' | 'visor';

const TITULOS: Record<TabDerechaId, string> = {
  chat: 'Chat',
  navegador: 'Navegador',
  visor: 'Visor',
};

export interface PanelDerecho {
  raiz: HTMLElement;
  /** Monta el contenido en su tab (o la reactiva) y la activa. */
  abrirTab(id: TabDerechaId, contenido: HTMLElement): void;
  /** Quita la tab y desmonta su contenido (el dueño conserva el nodo). */
  cerrarTab(id: TabDerechaId): void;
  tiene(id: TabDerechaId): boolean;
  activa(): TabDerechaId | null;
  hayTabs(): boolean;
}

export function montarPanelDerecho(opts: {
  onCerrarTodo(): void;
  onCambioTab(id: TabDerechaId | null): void;
}): PanelDerecho {
  const raiz = el('div', 'panel-derecho');
  raiz.setAttribute('aria-label', 'panel derecho');

  const barra = el('div', 'tabs-barra');
  barra.setAttribute('role', 'tablist');
  const botones = el('div', 'tabs-botones');
  const btnCerrar = el('button', 'tab-cerrar') as HTMLButtonElement;
  btnCerrar.type = 'button';
  btnCerrar.title = 'cerrar panel derecho';
  btnCerrar.setAttribute('aria-label', 'cerrar panel derecho');
  btnCerrar.appendChild(icono('x'));
  btnCerrar.addEventListener('click', () => opts.onCerrarTodo());
  barra.appendChild(botones);
  barra.appendChild(btnCerrar);

  const contenido = el('div', 'tabs-contenido');
  raiz.appendChild(barra);
  raiz.appendChild(contenido);

  const tabs = new Map<TabDerechaId, { boton: HTMLButtonElement; nodo: HTMLElement }>();
  let activaId: TabDerechaId | null = null;

  function pintar(): void {
    for (const [id, t] of tabs) {
      const esActiva = id === activaId;
      t.boton.classList.toggle('sel', esActiva);
      t.boton.setAttribute('aria-selected', String(esActiva));
      t.nodo.hidden = !esActiva;
    }
  }

  function activar(id: TabDerechaId | null): void {
    activaId = id;
    pintar();
    opts.onCambioTab(id);
  }

  return {
    raiz,
    abrirTab(id, nodo) {
      let t = tabs.get(id);
      if (!t) {
        const boton = el('button', 'tab') as HTMLButtonElement;
        boton.type = 'button';
        boton.setAttribute('role', 'tab');
        boton.textContent = TITULOS[id];
        boton.title = TITULOS[id];
        boton.addEventListener('click', () => activar(id));
        botones.appendChild(boton);
        contenido.appendChild(nodo);
        t = { boton, nodo };
        tabs.set(id, t);
      } else if (t.nodo !== nodo && nodo.parentNode !== contenido) {
        // Mismo tab con nodo nuevo (p. ej. lateral reabierto): sustituye.
        t.nodo.remove();
        contenido.appendChild(nodo);
        t.nodo = nodo;
      }
      activar(id);
    },
    cerrarTab(id) {
      const t = tabs.get(id);
      if (!t) return;
      t.boton.remove();
      t.nodo.remove();
      tabs.delete(id);
      if (activaId === id) {
        const siguiente = tabs.keys().next();
        activar(siguiente.done ? null : siguiente.value);
      }
    },
    tiene(id) {
      return tabs.has(id);
    },
    activa() {
      return activaId;
    },
    hayTabs() {
      return tabs.size > 0;
    },
  };
}
