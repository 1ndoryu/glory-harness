/* [109A-4] Comandos `/` del área en el modo web.
 *
 * El catálogo y la expansión de plantillas (`@archivo`, `$ARGUMENTOS`) los
 * resuelve el backend de ESCRITORIO contra el área activa: el servidor web
 * todavía no expone esas rutas. Se rechaza en vez de devolver una lista vacía
 * para no afirmar "este proyecto no define comandos" cuando lo que falta es
 * la capacidad.
 */

import type { Transporte } from '../tauri/real';
import { errorCapacidadAusente } from '../util/capacidad';

const MOTIVO = 'los comandos del área requieren la aplicación de escritorio';

/** Fragmento del transporte HTTP/SSE: sin comandos de área en modo web. */
export function transporteComandosNoDisponibles(): Pick<
  Transporte,
  'comandosListar' | 'comandoExpandir'
> {
  return {
    comandosListar: async () => {
      throw errorCapacidadAusente(MOTIVO);
    },
    comandoExpandir: async () => {
      throw errorCapacidadAusente(MOTIVO);
    },
  };
}
