/* [119A-2 F3] Gestión de proyectos en el modo web: listar/crear/activar/
 * renombrar/quitar + revelar (rechazo explícito, sin explorador local) +
 * fijar/soltar (mismo PATCH de renombrar, con `fijado`). Se extrajo de
 * `api.ts` —que superó el límite de 300 líneas— sin cambiar la superficie
 * `Transporte` ni el contrato HTTP. */

import type { Transporte, InfoSesion } from '../tauri/real';
import type { Workspace } from '../dominio/tipos';
import type { ClienteApi } from './apiCliente';

/** Fragmento del transporte HTTP/SSE: proyectos (áreas de trabajo). */
export function crearTransporteWorkspaces(
  cliente: ClienteApi,
): Pick<
  Transporte,
  | 'workspacesListar'
  | 'proyectoGuardar'
  | 'workspaceActivarsPorRuta'
  | 'workspaceRenombrar'
  | 'workspaceEliminar'
  | 'workspaceRevelar'
  | 'workspaceFijar'
> {
  const http = cliente.http;
  const recordar = cliente.recordar;
  const rutaAreas = () => `/api/v1/session/${cliente.getSid()}/workspaces`;

  return {
    // [069A-Proyectos] HTTP workspaces
    workspacesListar: async () => {
      const r = await http<{ ok: boolean; workspaces: Workspace[]; activa: Workspace | null }>(
        'GET',
        rutaAreas(),
      );
      return { workspaces: r.workspaces, activa: r.activa };
    },
    proyectoGuardar: async (nombre, ruta) => {
      // Web: POST /workspaces crea la fila + POST /workspace la activa
      const creada = await http<{ ok: boolean; activa: Workspace; creada: Workspace; workspace: string }>(
        'POST',
        rutaAreas(),
        { nombre, ruta },
      );
      const base_info: InfoSesion = cliente.getUltimoInfo() ?? {
        modelo: '/',
        workspace: creada.workspace,
        proveedores: [],
        conversacion: null,
      };
      // El backend ya cambió el workspace de la sesión; componer info limpia.
      const info: InfoSesion = {
        ...base_info,
        workspace: creada.workspace,
      };
      return recordar(info);
    },
    workspaceActivarsPorRuta: async (ruta) => {
      // Reusa fijarWorkspace (POST /workspace) que cambia la ruta activa.
      return cliente.fijarWorkspaceImpl(ruta);
    },
    workspaceRenombrar: async (id, nombre) => {
      const r = await http<{ ok: boolean; renombrada: boolean }>(
        'PATCH',
        `${rutaAreas()}/${encodeURIComponent(id)}`,
        { nombre },
      );
      return r.renombrada;
    },
    workspaceEliminar: async (id) => {
      const r = await http<{ ok: boolean; eliminada: boolean }>(
        'DELETE',
        `${rutaAreas()}/${encodeURIComponent(id)}`,
      );
      return r.eliminada;
    },
    // [119A-2 F2] Sin explorador local en web: aviso en vez de simular.
    workspaceRevelar: async () => {
      throw new Error('abrir en el Explorador solo está disponible en la app de escritorio');
    },
    // [119A-2 F3] Web: el mismo PATCH de renombrar, con `fijado`.
    workspaceFijar: async (id, fijado) => {
      const r = await http<{ ok: boolean; fijada: boolean }>(
        'PATCH',
        `${rutaAreas()}/${encodeURIComponent(id)}`,
        { fijado },
      );
      return r.fijada;
    },
  };
}
