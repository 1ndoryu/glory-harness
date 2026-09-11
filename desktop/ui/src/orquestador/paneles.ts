/* Gestión de los paneles de chat (principal + laterales): cuál está activo,
 * cómo se activa y a quién va el aviso global.
 *
 * Extraído de `main.ts` en 109A-6 sin cambiar comportamiento. La sidebar y el
 * panel derecho se montan después de registrar los paneles, así que llegan
 * como cierres perezosos: solo se invocan desde eventos de UI, nunca durante
 * el montaje. */

import type { PanelChat } from '../componentes/panelChat';

export interface PanelesDeps {
  paneles: PanelChat[];
  getSidebar: () => { seleccionar(id: string): void };
  /** [089A-4] El panel derecho decide si ofrecer "Chat lateral". */
  getPanelDerecho: () => { fijarInicioChatDisponible(disponible: boolean): void };
}

export interface GestorPaneles {
  /** Último panel enfocado; si ninguno, el principal. */
  panelActivo(): PanelChat | null;
  activarPanel(panel: PanelChat | null): void;
  avisoGlobal(texto: string, meta: string, detalle: string): void;
}

export function crearGestorPaneles(deps: PanelesDeps): GestorPaneles {
  function panelActivo(): PanelChat | null {
    return (
      deps.paneles.find((p) => p.raiz.classList.contains('enfocado')) ??
      deps.paneles.find((p) => p.tipo === 'principal') ??
      null
    );
  }

  function activarPanel(panel: PanelChat | null): void {
    if (!panel) return;
    deps.paneles.forEach((p) => {
      if (p !== panel) p.desenfocar();
    });
    panel.activar();
    const id = panel.conversaId;
    const sidebar = deps.getSidebar();
    // [069A-7] `null` = borrador (sin conversación): deselecciona la sidebar.
    if (id) sidebar.seleccionar(id);
    else sidebar.seleccionar('');
    deps.getPanelDerecho().fijarInicioChatDisponible(id != null);
  }

  function avisoGlobal(texto: string, meta: string, detalle: string): void {
    panelActivo()?.avisoLocal(texto, meta, detalle);
  }

  return { panelActivo, activarPanel, avisoGlobal };
}
