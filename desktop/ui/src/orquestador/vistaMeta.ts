/* PanelMeta global + estado de turno (extraído de main.ts [089A-16 F1b]).
 * El flag de turno y el último panel con envío viven aquí (M1: un turno a
 * la vez); el orquestador los consume vía los helpers devueltos. Sin
 * importes del orquestador. */

import { montarPanelMeta, type PanelMeta } from '../componentes/panelMeta';
import type { PanelChat } from '../componentes/panelChat';

export interface VistaMetaDeps {
  usaReal: boolean;
  actualizarMeta: (valor: string | null) => Promise<unknown>;
  detenerReal: () => void;
  detenerMock: () => void;
  paneles: () => PanelChat[];
  avisar: (texto: string, meta: string, detalle: string) => void;
}

export interface VistaMeta {
  panelMeta: PanelMeta;
  hayTurnoGlobal: () => boolean;
  notificarTurnoInicio: () => void;
  notificarTurnoFin: (alTerminar: () => void) => void;
  registrarUltimoEnvio: (panel: PanelChat) => void;
  sincronizarPanelMeta: () => void;
}

export function montarVistaMeta(deps: VistaMetaDeps): VistaMeta {
  // Flag global: algún panel tiene turno en curso (M1: solo uno a la vez).
  let turnoGlobal = false;
  // Panel que envió el último mensaje (para el play/reanudar del panelMeta).
  let panelUltimoEnvio: PanelChat | null = null;

  function hayTurnoGlobal(): boolean {
    return turnoGlobal;
  }

  function notificarTurnoInicio(): void {
    turnoGlobal = true;
    deps.paneles().forEach((p) => p.setCorriendoGlobal(true));
  }

  function notificarTurnoFin(alTerminar: () => void): void {
    turnoGlobal = false;
    deps.paneles().forEach((p) => p.setCorriendoGlobal(false));
    alTerminar();
  }

  function registrarUltimoEnvio(panel: PanelChat): void {
    panelUltimoEnvio = panel;
  }

  // ---------- PanelMeta global (M1): lo monta el orquestador dentro de la
  // entrada del principal (el lateral no tiene panelMeta propio). ----------
  const panelMeta = montarPanelMeta({
    onMetaCambiada(meta) {
      if (!deps.usaReal) return;
      const valor = meta.trim() ? meta.trim() : null;
      void deps
        .actualizarMeta(valor)
        .catch((e: unknown) => deps.avisar(`no se pudo fijar la meta: ${String(e)}`, '', ''));
    },
    onPausar() {
      if (!turnoGlobal) return;
      // Pausar = cancelar el turno global en curso (M1).
      if (deps.usaReal) deps.detenerReal();
      else deps.detenerMock();
      panelMeta.setEstado('pausado');
      deps.paneles().forEach((p) => p.setCorriendoGlobal(false));
      turnoGlobal = false;
    },
    onReanudar() {
      if (turnoGlobal) return;
      if (!panelUltimoEnvio) {
        deps.avisar('nada que reanudar: envía un mensaje primero', '', '');
        return;
      }
      panelUltimoEnvio.reanudarUltimo();
    },
  });

  function sincronizarPanelMeta(): void {
    /* [109A-4 F4] El modo global `meta` se retiró, así que la fila ya no depende
     * de él: vive con la conversación y muestra estado/tiempo/tokens del turno
     * M1. La meta que hay ahí se aplica al turno solo-lectura —`/meta <texto>`
     * la refleja y la persiste— y el backend la ignora fuera de esos turnos
     * (segunda barrera, no la única). */
    const hayConversacion = deps.paneles().some((p) => p.conversaId !== null);
    panelMeta.mostrar(hayConversacion);
  }

  return {
    panelMeta,
    hayTurnoGlobal,
    notificarTurnoInicio,
    notificarTurnoFin,
    registrarUltimoEnvio,
    sincronizarPanelMeta,
  };
}
