// ============================================================
// Panel derecho con tabs (089A-2, referencia Paseo): chats laterales,
// Navegador y Visor conviven como pestañas en vez de excluirse.
// Los ids son dinámicos: 'navegador', 'visor' y 'chat:<id>' (una tab por
// conversación lateral). Cada tab con cierre propio lleva su × (además
// del × global que cierra todo el panel). El contenido de cada tab es el
// nodo vivo del componente (se oculta con `hidden`, no se destruye).
// Sin tabs muestra el inicio: pantalla de opciones centrada (icono +
// etiqueta, full-width) al estilo `RightDockLauncher` de Synara
// (089A-4): Navegador, Visor y Chat lateral (con gating: el chat solo
// si hay conversación activa; Terminal/Files/Source control pendientes,
// ver roadmap). Elegir una opción abre su tab.
// ============================================================

import '../estilos/tabs.css';
import '../estilos/launcher.css';
import { icono } from './iconos';
import { el } from '../util/dom';
import type { IconoNombre } from '../dominio/tipos';

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
  /** Habilita la opción "Chat lateral" del inicio (hay conversación activa). */
  fijarInicioChatDisponible(hay: boolean): void;
}

/** Opción del inicio (pantalla sin tabs): abre su tab correspondiente. */
export type OpcionInicio = 'navegador' | 'visor' | 'chat';

export function montarPanelDerecho(opts: {
  onCerrarTodo(): void;
  onCambioTab(id: TabDerechaId | null): void;
  /** El usuario eligió una opción del inicio (panel sin tabs). */
  onElegirInicio(opcion: OpcionInicio): void;
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

  // ---- Inicio (089A-4): pantalla de opciones sin tabs, estilo
  // `RightDockLauncher` de Synara (icono + etiqueta, full-width).
  const inicio = el('div', 'panel-inicio');
  inicio.setAttribute('aria-label', 'elegir contenido del panel');
  const listaInicio = el('div', 'inicio-lista');
  inicio.appendChild(listaInicio);
  raiz.appendChild(inicio);

  function opcionInicio(
    opcion: OpcionInicio,
    iconoNombre: IconoNombre,
    etiqueta: string,
  ): HTMLButtonElement {
    const btn = el('button', 'inicio-opcion') as HTMLButtonElement;
    btn.type = 'button';
    btn.title = etiqueta;
    btn.setAttribute('aria-label', etiqueta);
    btn.appendChild(icono(iconoNombre));
    const texto = el('span', 'inicio-etiqueta');
    texto.textContent = etiqueta;
    btn.appendChild(texto);
    btn.addEventListener('click', () => opts.onElegirInicio(opcion));
    listaInicio.appendChild(btn);
    return btn;
  }

  opcionInicio('navegador', 'navegador', 'Navegador');
  opcionInicio('visor', 'archivo', 'Visor');
  // Gating como Synara: el chat lateral solo si hay conversación activa
  // (el orquestador lo habilita con `fijarInicioChatDisponible`).
  const btnChatInicio = opcionInicio('chat', 'mensaje', 'Chat lateral');
  btnChatInicio.disabled = true;

  const tabs = new Map<TabDerechaId, { boton: HTMLButtonElement; nodo: HTMLElement; onCerrar?: () => void }>();
  let activaId: TabDerechaId | null = null;

  function pintar(): void {
    for (const [id, t] of tabs) {
      const esActiva = id === activaId;
      t.boton.classList.toggle('sel', esActiva);
      t.boton.setAttribute('aria-selected', String(esActiva));
      t.nodo.hidden = !esActiva;
    }
    // Sin tabs: el inicio ocupa el panel (Synara muestra su launcher).
    // La barra de tabs se conserva (su × global cierra el panel).
    const vacio = tabs.size === 0;
    inicio.hidden = !vacio;
    contenido.hidden = vacio;
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
    fijarInicioChatDisponible(hay: boolean) {
      btnChatInicio.disabled = !hay;
    },
  };
}
