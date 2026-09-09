/* Barra superior global estilo Synara (extraída de main.ts [089A-16 F1b]).
 * Los toggles llegan como cierres perezosos (sidebar y panel derecho se
 * crean después); solo se invocan en runtime, fuera de la TDZ. */

import { montarBarraSuperior, type BarraSuperior } from '../componentes/barraSuperior';

export interface VistaBarraDeps {
  alternarSidebar: () => void;
  alternarPanelDerecho: () => void;
}

export function montarVistaBarra(deps: VistaBarraDeps): BarraSuperior {
  // [089A-3] Barra superior global estilo Synara (toggles + arrastre +
  // botonera caption). Se monta como primera hija de #app (ver montaje).
  const barra = montarBarraSuperior({
    onAlternarSidebar() {
      deps.alternarSidebar();
    },
    // [089A-3] Atrás/adelante replican la navegación por historial de Synara;
    // lógica pendiente (roadmap): arrancan deshabilitados.
    onAtras() {
      /* pendiente: historial de la app */
    },
    onAdelante() {
      /* pendiente: historial de la app */
    },
    onAlternarPanelDerecho() {
      deps.alternarPanelDerecho();
    },
  });
  barra.setPuedeNavegar(false, false);
  return barra;
}
