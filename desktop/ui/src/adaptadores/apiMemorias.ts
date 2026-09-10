/* [109A-3] Memorias del proyecto activo en el modo web.
 *
 * El ámbito lo resuelve el backend de ESCRITORIO contra el área activa de la
 * sesión (tabla `workspaces`), y el servidor web todavía no expone rutas de
 * memoria. Por eso estas acciones RECHAZAN en lugar de devolver una lista
 * vacía: una lista vacía se leería como "este proyecto no tiene memorias" y
 * mentiría sobre el estado real.
 */

import type { Transporte } from '../tauri/real';

const MOTIVO = 'el panel de memorias requiere la aplicación de escritorio';

/** Fragmento del transporte HTTP/SSE: sin memorias en modo web. */
export function transporteMemoriasNoDisponibles(): Pick<
  Transporte,
  'memoriaListar' | 'memoriaBorrar' | 'memoriaCurar' | 'memoriaExportar' | 'memoriaImportar'
> {
  return {
    memoriaListar: async () => {
      throw new Error(MOTIVO);
    },
    memoriaBorrar: async () => {
      throw new Error(MOTIVO);
    },
    memoriaCurar: async () => {
      throw new Error(MOTIVO);
    },
    memoriaExportar: async () => {
      throw new Error(MOTIVO);
    },
    memoriaImportar: async () => {
      throw new Error(MOTIVO);
    },
  };
}
