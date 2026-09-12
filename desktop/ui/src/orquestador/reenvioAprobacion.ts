/* Reenvío del mensaje tras resolver aprobaciones (extraído de main.ts
 * [fix 12-09] para el techo de 300 líneas).
 *
 * La decisión suele llegar DESPUÉS del `turno-fin` (el cierre ya no pudo
 * reenviar: había pendientes). Si no hay turno en curso ni pendientes, el
 * último mensaje vuelve a correr como turno nuevo por el curso normal
 * (flag global, pie y meta). Doble guarda de `hayTurnoGlobal` (antes y
 * después del `await`) contra un envío manual que arranque en medio de la
 * comprobación. En web/HTTP el hook nunca se invoca (el transporte mock
 * no reenvía; `usaReal` lo blinda de todos modos). */

import type { HooksAdaptador } from '../tauri/real';

export interface ReenvioAprobacionDeps {
  usaReal: boolean;
  hayTurnoGlobal: () => boolean;
  aprobacionesPendientes: () => Promise<unknown[]>;
  reanudarUltimoEnvio: () => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
}

/** Conecta `onAprobacionResuelta` con el reenvío del último mensaje. */
export function conectarReenvioTrasAprobar(
  hooks: HooksAdaptador,
  d: ReenvioAprobacionDeps,
): void {
  hooks.onAprobacionResuelta = () => {
    if (!d.usaReal) return;
    if (d.hayTurnoGlobal()) return;
    void (async () => {
      try {
        if ((await d.aprobacionesPendientes()).length !== 0) return;
      } catch {
        return;
      }
      if (d.hayTurnoGlobal()) return;
      d.avisar('aprobaciones resueltas: se reenvía el mensaje', '', '');
      d.reanudarUltimoEnvio();
    })();
  };
}
