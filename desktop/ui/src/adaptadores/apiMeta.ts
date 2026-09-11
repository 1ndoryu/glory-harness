/* [109A-5 F3] Ciclo de vida de la meta en el modo web: el PATCH sobre
 * `/meta` lleva la acción (y el `turno_id` que respalda un logro) y devuelve
 * el estado completo de la conversación; el GET es la lectura del estado
 * durable. Se extrajo de `api.ts` —que quedó por encima del límite de 300
 * líneas— sin cambiar la superficie `Transporte` ni el contrato HTTP. */

import type { Transporte } from '../tauri/real';
import type { EstadoMetaVisible } from '../dominio/tipos';
import type { ClienteApi } from './apiCliente';

/** Fragmento del transporte HTTP/SSE: meta de la conversación activa. */
export function crearTransporteMeta(
  cliente: ClienteApi,
): Pick<Transporte, 'fijarMeta' | 'metaAplicar' | 'metaLeer'> {
  const http = cliente.http;
  const ruta = () => `/api/v1/session/${cliente.getSid()}/meta`;

  return {
    fijarMeta: async (meta) => {
      const r = await http<{ ok: boolean; meta: string | null }>('PATCH', ruta(), { meta });
      return r.meta;
    },
    /* Un logro sin `turno_id` no puede anclarse al pie del turno, así que el
     * backend lo rechaza y el panel lo avisa antes de llamar. */
    metaAplicar: async (comando) => {
      const r = await http<{ ok: boolean; estado: EstadoMetaVisible | null }>(
        'PATCH',
        ruta(),
        {
          accion: comando.accion,
          meta: comando.meta ?? null,
          turno_id: comando.turno_id ?? null,
        },
      );
      return r.estado;
    },
    metaLeer: async () => {
      const r = await http<{ ok: boolean; estado: EstadoMetaVisible | null }>('GET', ruta());
      return r.estado;
    },
  };
}
