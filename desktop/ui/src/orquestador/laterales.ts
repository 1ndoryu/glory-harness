/* Gestión de chats laterales + menú de acciones de cabecera (extraído de
 * main.ts [089A-16 F1b]). Cada lateral vive en su propia tab del panel
 * derecho (multi-chat estilo Paseo). Las dependencias se inyectan por
 * parámetro; el contador de laterales es estado propio del módulo. */

import type { Conversacion } from '../dominio/tipos';
import type { PanelChat } from '../componentes/panelChat';
import type { PanelDerecho } from '../componentes/panelDerecho';
import type { Sidebar } from '../componentes/sidebar';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from '../componentes/menu';
import { copiarAlPortapapeles } from '../util/portapapeles';

export interface LateralesDeps {
  paneles: PanelChat[];
  getConversaciones: () => Conversacion[];
  crearPanel: (
    tipo: 'principal' | 'lateral',
    idPrefijo: string,
    opts?: { onCerrar?: () => void },
  ) => PanelChat;
  activarPanel: (panel: PanelChat | null) => void;
  panelActivo: () => PanelChat | null;
  asegurarPanelDerecho: () => void;
  cerrarPanelDerechoSiVacio: () => void;
  panelDerecho: PanelDerecho;
  sidebar: Sidebar;
  avisar: (texto: string, meta: string, detalle: string) => void;
  renombrarEnLista: (id: string, titulo: string) => Promise<void>;
}

/** ¿Se puede ofrecer "Abrir en lateral"? (multi-chat: hasta 8 laterales en
 * tabs; el ancho ya no limita porque comparten el panel derecho). */
const MAX_LATERALES = 8;
export function puedeAbrirLateralEn(paneles: PanelChat[]): boolean {
  return paneles.filter((p) => p.tipo === 'lateral').length < MAX_LATERALES;
}

let contadorLaterales = 0;

/** Abre/activa un lateral en su propia tab `chat:<id>` (multi-chat). Si la
 * conversación ya está abierta, solo se activa su tab. */
export async function abrirEnLateral(deps: LateralesDeps, id: string): Promise<void> {
  const tabId = `chat:${id}`;
  const yaAbierto = deps.paneles.find(
    (p) => p.tipo === 'lateral' && p.raiz.dataset.tabId === tabId,
  );
  if (yaAbierto) {
    deps.asegurarPanelDerecho();
    deps.panelDerecho.activarTab(tabId);
    deps.activarPanel(yaAbierto);
    yaAbierto.enfocarEntrada();
    return;
  }
  if (!puedeAbrirLateralEn(deps.paneles)) {
    deps.avisar('demasiados laterales abiertos', '', 'cierra alguna tab del panel derecho');
    return;
  }
  contadorLaterales += 1;
  const titulo = deps.getConversaciones().find((c) => c.id === id)?.titulo ?? 'Chat';
  const lateral = deps.crearPanel('lateral', `lateral-${contadorLaterales}`, {
    onCerrar() {
      cerrarLateral(deps, tabId);
    },
  });
  lateral.raiz.dataset.tabId = tabId;
  // El lateral vive como tab del panel derecho (convive con Files, Git,
  // navegador y otros chats). Sin grip propio: el panel derecho trae su grip único.
  deps.asegurarPanelDerecho();
  deps.panelDerecho.abrirTab(tabId, titulo, lateral.raiz, () => cerrarLateral(deps, tabId));
  // El textarea del lateral nace con height 0px porque el `medir()` de
  // montarEntrada corre en el constructor, antes de estar en el DOM
  // (scrollHeight 0). Al montar el panel ya se puede medir: recalcula la
  // altura del input (si no, el área de escritura del lateral queda invisible).
  lateral.medir();
  // Carga la conversación antes de devolver el control: la restauración del
  // layout depende de que el lateral ya tenga título, mensajes y conversaId.
  // Los clics de UI pueden ignorar la promesa, pero el fallo sigue siendo
  // observable y el panel incompleto no queda registrado como abierto.
  try {
    await lateral.cargarConversacion(id);
    deps.activarPanel(lateral);
  } catch (e: unknown) {
    deps.avisar('no se pudo abrir el chat lateral', '', String(e));
    cerrarLateral(deps, tabId);
  }
}

/** Abre un lateral nuevo sin cargar la conversación enfocada. */
export function abrirChatLateralVacio(deps: LateralesDeps): void {
  if (!puedeAbrirLateralEn(deps.paneles)) {
    deps.avisar('demasiados laterales abiertos', '', 'cierra alguna tab del panel derecho');
    return;
  }
  contadorLaterales += 1;
  const tabId = `chat:nuevo-${contadorLaterales}`;
  const lateral = deps.crearPanel('lateral', `lateral-${contadorLaterales}`, {
    onCerrar() {
      cerrarLateral(deps, tabId);
    },
  });
  lateral.raiz.dataset.tabId = tabId;
  lateral.ponerBorrador();
  deps.asegurarPanelDerecho();
  deps.panelDerecho.abrirTab(tabId, 'Nueva conversación', lateral.raiz, () =>
    cerrarLateral(deps, tabId),
  );
  lateral.medir();
  deps.activarPanel(lateral);
  lateral.enfocarEntrada();
}

/** Cierra un lateral (`tabId`) o todos (el principal permanece). */
export function cerrarLateral(deps: LateralesDeps, tabId?: string): void {
  const victimas = deps.paneles.filter(
    (p) => p.tipo === 'lateral' && (!tabId || p.raiz.dataset.tabId === tabId),
  );
  if (victimas.length === 0) return;
  const habiaActivo = victimas.some((p) => p === deps.panelActivo());
  victimas.forEach((p) => {
    const idx = deps.paneles.indexOf(p);
    if (idx >= 0) deps.paneles.splice(idx, 1);
    const tid = p.raiz.dataset.tabId;
    if (tid) deps.panelDerecho.cerrarTab(tid);
  });
  // Desmonta sus tabs; si no quedan tabs se oculta el panel derecho
  // (con su grip). El nodo lo conserva el panel hasta reabrirlo.
  deps.cerrarPanelDerechoSiVacio();
  if (habiaActivo) {
    const principal = deps.paneles.find((p) => p.tipo === 'principal');
    if (principal) {
      deps.activarPanel(principal);
      principal.enfocarEntrada();
    }
  }
}

/** Menú ⋯ de la cabecera de un panel (sobre la conversación de ESE panel). */
export function abrirAccionesPanel(deps: LateralesDeps, panel: PanelChat, rect: DOMRect): void {
  const id = panel.conversaId;
  const conv = deps.getConversaciones().find((c) => c.id === id);
  if (!conv) {
    deps.avisar('no hay conversación activa', '', 'abre una conversación desde la lista');
    return;
  }
  abrirMenuContextual({
    rect,
    construir(m) {
      m.appendChild(
        crearItemMenu({
          texto: 'Cambiar nombre',
          onClick() {
            cerrarMenuActual();
            // Renombrar inline en la cabecera de ESTE panel.
            panel.empezarRenombrarCabecera(conv.titulo, (nuevo) => {
              void deps.renombrarEnLista(conv.id, nuevo);
            });
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: conv.archivada ? 'Desarchivar' : 'Archivar',
          onClick() {
            deps.sidebar.archivarConversacion(conv.id);
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
      // "Abrir en panel lateral" desde el ⋯ del principal.
      if (puedeAbrirLateralEn(deps.paneles) && panel.tipo === 'principal') {
        m.appendChild(crearSeparadorMenu());
        m.appendChild(
          crearItemMenu({
            texto: 'Abrir en panel lateral',
            onClick() {
              cerrarMenuActual();
              abrirEnLateral(deps, conv.id);
            },
          }),
        );
      }
      m.appendChild(crearSeparadorMenu());
      m.appendChild(
        crearItemMenu({
          texto: 'Eliminar',
          onClick() {
            deps.sidebar.eliminarConversacion(conv.id);
          },
        }),
      );
    },
  });
}
