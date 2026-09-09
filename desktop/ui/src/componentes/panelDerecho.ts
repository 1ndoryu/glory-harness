// Panel derecho con tabs (089A-2, referencia Synara): chats laterales,
// Files, Git local y Navegador conviven como pestañas. Files incluye su
// propio visor dividido, así que no existe una tab de visor independiente.
// Los ids son dinámicos: 'navegador' y 'chat:<id>'. Cada tab con cierre
// propio cierra solo su pestaña. Sin tabs muestra el inicio: pantalla de
// opciones centrada al estilo `RightDockLauncher` de Synara.

import '../estilos/tabs.css';
import '../estilos/launcher.css';
import { cerrarMenuActual, abrirMenuContextual, crearItemMenu } from './menu';
import { icono } from './iconos';
import { el } from '../util/dom';
import type { IconoNombre } from '../dominio/tipos';

/** Id de tab: 'files' | 'git' | 'navegador' | 'chat:<conversaId>'. */
export type TabDerechaId = string;

export interface PanelDerecho {
  raiz: HTMLElement;
  /** Barra única de tabs, montada por el orquestador en la barra superior. */
  tabsBarra: HTMLElement;
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
export type OpcionInicio = 'files' | 'git' | 'navegador' | 'chat';

export function montarPanelDerecho(opts: {
  onCambioTab(id: TabDerechaId | null): void;
  /** El usuario eligió una opción del inicio (panel sin tabs). */
  onElegirInicio(opcion: OpcionInicio): void;
}): PanelDerecho {
  const raiz = el('div', 'panel-derecho');
  raiz.setAttribute('aria-label', 'panel derecho');

  const barra = el('div', 'tabs-barra');
  barra.setAttribute('role', 'tablist');
  const botones = el('div', 'tabs-botones');
  barra.appendChild(botones);
  const botonMas = el('button', 'tab-mas') as HTMLButtonElement;
  botonMas.type = 'button';
  botonMas.title = 'Agregar tab';
  botonMas.setAttribute('aria-label', 'Agregar tab');
  botonMas.setAttribute('aria-haspopup', 'menu');
  botonMas.appendChild(icono('mas'));
  barra.appendChild(botonMas);

  const contenido = el('div', 'tabs-contenido');
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

  opcionInicio('files', 'carpeta', 'Files');
  opcionInicio('git', 'flujo', 'Git local');
  opcionInicio('navegador', 'navegador', 'Navegador');
  // Gating como Synara: el chat lateral solo si hay conversación activa
  // (el orquestador lo habilita con `fijarInicioChatDisponible`).
  const btnChatInicio = opcionInicio('chat', 'mensaje', 'Chat lateral');
  btnChatInicio.disabled = true;
  let chatLateralDisponible = false;

  function abrirMenuAgregar(): void {
    abrirMenuContextual({
      rect: botonMas.getBoundingClientRect(),
      construir(menu) {
        for (const [opcion, etiqueta] of [
          ['files', 'Files'],
          ['git', 'Git local'],
          ['navegador', 'Navegador'],
          ...(chatLateralDisponible ? [['chat', 'Chat lateral']] : []),
        ] as Array<[OpcionInicio, string]>) {
          menu.appendChild(
            crearItemMenu({
              texto: etiqueta,
              onClick() {
                cerrarMenuActual();
                opts.onElegirInicio(opcion);
              },
            }),
          );
        }
      },
    });
  }
  botonMas.addEventListener('click', abrirMenuAgregar);

  const tabs = new Map<TabDerechaId, { boton: HTMLButtonElement; nodo: HTMLElement; onCerrar?: () => void }>();
  let activaId: TabDerechaId | null = null;

  function pintar(): void {
    for (const [id, t] of tabs) {
      const esActiva = id === activaId;
      t.boton.classList.toggle('sel', esActiva);
      t.boton.setAttribute('aria-selected', String(esActiva));
      t.nodo.hidden = !esActiva;
    }
    // Sin tabs: el launcher ocupa todo el panel y no hay barra vacía.
    const vacio = tabs.size === 0;
    barra.hidden = vacio;
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

  pintar();

  return {
    raiz,
    tabsBarra: barra,
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
      chatLateralDisponible = hay;
      btnChatInicio.disabled = !hay;
    },
  };
}
