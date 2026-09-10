/* [109A-4 F3] Compactación por demanda en el modo web.
 *
 * El punto de compactación (resumen + marca temporal) vive en la SQLite del
 * escritorio y lo aplica el backend al preparar el turno; el servidor web
 * todavía no expone esa ruta. Se rechaza con motivo explícito en vez de
 * devolver un ahorro falso o un no-op silencioso, que el usuario leería como
 * "ya estaba compactado".
 */

import type { Transporte } from '../tauri/real';
import { errorCapacidadAusente } from '../util/capacidad';

const MOTIVO = 'la compactación por demanda requiere la aplicación de escritorio';

/** Fragmento del transporte HTTP/SSE: sin compactación por demanda. */
export function transporteCompactarNoDisponible(): Pick<
  Transporte,
  'compactarConversacion'
> {
  return {
    compactarConversacion: async () => {
      throw errorCapacidadAusente(MOTIVO);
    },
  };
}
