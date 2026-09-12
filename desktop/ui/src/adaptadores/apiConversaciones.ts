/* [119A-2 F4] Batch de hilos por proyecto en el modo web: archivar todas
 * (`POST .../workspaces/:wid/archivar` → `archivadas`) y eliminar todas
 * (`DELETE .../workspaces/:wid/conversaciones` → `eliminadas` + `actual`
 * para reconciliar el panel, mismo contrato que el DELETE single).
 * Se extrajo a archivo propio para no engordar `api.ts` (límite 300). */

import type { Transporte, InfoConversacion } from '../tauri/real';
import type { ClienteApi } from './apiCliente';

/** Fragmento del transporte HTTP/SSE: batch de hilos por proyecto. */
export function crearTransporteConversacionesProyecto(
  cliente: ClienteApi,
): Pick<Transporte, 'convArchivarProyecto' | 'convEliminarProyecto'> {
  const http = cliente.http;
  const rutaProyecto = (id: string) =>
    `/api/v1/session/${cliente.getSid()}/workspaces/${encodeURIComponent(id)}`;

  return {
    convArchivarProyecto: async (id, archivada) => {
      const r = await http<{ ok: boolean; archivadas: number }>(
        'POST',
        `${rutaProyecto(id)}/archivar`,
        { archivada },
      );
      return r.archivadas;
    },
    convEliminarProyecto: async (id) => {
      const r = await http<{ ok: boolean; eliminadas: number; actual: InfoConversacion | null }>(
        'DELETE',
        `${rutaProyecto(id)}/conversaciones`,
      );
      return r.actual;
    },
  };
}
