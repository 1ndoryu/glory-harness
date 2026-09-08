// ============================================================
// Panel derecho con tabs (089A-2, referencia Paseo): chats laterales,
// Navegador y Visor conviven como pestañas en vez de excluirse.
// Los ids son dinámicos: 'navegador', 'visor' y 'chat:<id>' (una tab por
// conversación lateral). Cada tab con cierre propio lleva su × (además
// del × global que cierra todo el panel). El contenido de cada tab es el
// nodo vivo del componente (se oculta con `hidden`, no se destruye).
// ============================================================

import '../estilos/tabs.css';
import { icono } from './iconos';
import { el } from '../util/dom';

/** Id de tab: 'navegador' | 'visor' | 'chat:<conversaId>'. */
export type TabDerechaId = string;

export interface PanelDerecho {
  raiz: HTMLElement;
  /** Monta el contenido en su tab (o reactiva la existente) y la activa. */
  abrirTab(id: TabDerechaId, titulo: string, contenido: HTMLElement, onCerrar?: () => void): void;
  /** Activa una tab existente (sin crearla). */
  activarTab(id: TabDerechaId): void;
  /** Quita la tab y desmonta su contenido (sin invocar `onCerrar`: el dueño
   * ya limpió; el dueño conserva el nodo si le sirve). */
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

  const tabs = new Map<TabDerechaId, { boton: HTMLButtonElement; nodo: HTMLElement; onCerrar?: () => void }>();
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

  function quitarTab(id: TabDerechaId): void {
    const t = tabs.get(id);
    if (!t) return;
    t.boton.remove();
    t.nodo.remove();
    tabs.delete(id);
    if (activaId === id) {
      const siguiente = tabs.keys().next();
      activar(siguiente.done ? null : siguiente.value);
    }
  }

  return {
    raiz,
    abrirTab(id, titulo, nodo, onCerrar) {
      let t = tabs.get(id);
      if (!t) {
        const boton = el('button', 'tab') as HTMLButtonElement;
        boton.type = 'button';
        boton.setAttribute('role', 'tab');
        boton.title = titulo;
        const etiqueta = el('span', 'tab-etiqueta');
        etiqueta.textContent = titulo;
        boton.appendChild(etiqueta);
        // × propio de la tab (Paseo: cada tab se cierra sola). Si el dueño
        // dio `onCerrar`, él desmonta; si no, basta quitar la tab.
        const x = el('span', 'tab-x') as HTMLElement;
        x.setAttribute('role', 'button');
        x.setAttribute('aria-label', `cerrar ${titulo}`);
        x.title = `cerrar ${titulo}`;
        x.appendChild(icono('x'));
        x.addEventListener('click', (e) => {
          e.stopPropagation();
          const actual = tabs.get(id);
          if (actual?.onCerrar) actual.onCerrar();
          else quitarTab(id);
        });
        boton.appendChild(x);
        boton.addEventListener('click', () => activar(id));
        botones.appendChild(boton);
        contenido.appendChild(nodo);
        t = { boton, nodo, onCerrar };
        tabs.set(id, t);
      } else {
        if (t.nodo !== nodo && nodo.parentNode !== contenido) {
          // Mismo tab con nodo nuevo: sustituye.
          t.nodo.remove();
          contenido.appendChild(nodo);
          t.nodo = nodo;
        }
        t.onCerrar = onCerrar ?? t.onCerrar;
      }
      activar(id);
    },
    activarTab(id) {
      if (tabs.has(id)) activar(id);
    },
    cerrarTab(id) {
      quitarTab(id);
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
