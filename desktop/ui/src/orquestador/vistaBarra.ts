/* Barra superior global estilo Synara (extraída de main.ts [089A-16 F1b]).
 * Los toggles llegan como cierres perezosos (sidebar y panel derecho se
 * crean después); solo se invocan en runtime, fuera de la TDZ. */

import { montarBarraSuperior, type BarraSuperior } from '../componentes/barraSuperior';

export interface VistaBarraDeps {
  alternarSidebar: () => void;
  alternarPanelDerecho: () => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
  /** [089A-5] Atrás/adelante del historial de la app (cierres de runtime). */
  onAtras: () => void;
  onAdelante: () => void;
}

export function montarVistaBarra(deps: VistaBarraDeps): BarraSuperior {
  // [089A-3] Barra superior global estilo Synara (toggles + arrastre +
  // botonera caption). Se monta como primera hija de #app (ver montaje).
  const barra = montarBarraSuperior({
    onAlternarSidebar() {
      deps.alternarSidebar();
    },
    // [089A-5] Historial de la app con la misma lógica que Synara.
    onAtras() {
      deps.onAtras();
    },
    onAdelante() {
      deps.onAdelante();
    },
    onAlternarPanelDerecho() {
      deps.alternarPanelDerecho();
    },
    onErrorVentana(detalle) {
      deps.avisar('Ventana: arrastre no disponible', 'ventana', detalle);
    },
  });
  barra.setPuedeNavegar(false, false);
  return barra;
}
