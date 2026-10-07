// Fragmento del transporte HTTP/SSE: archivos del workspace + estado git.
// Extraído de `api.ts` (superaba el límite de 300 líneas efectivas): la
// superficie `Transporte` y el contrato HTTP no cambian; `api.ts` lo mezcla
// con spread como ya hace con `apiMeta`/`apiWorkspaces`.

import type { EstadoGit, RepoGit } from '../componentes/panelGit';
import type { ListadoWorkspace, ResultadoBusqueda } from '../dominio/tipos';
import type { Transporte } from '../tauri/real';
import type { ClienteApi } from './apiCliente';

/** Fragmento del transporte HTTP/SSE: archivos del workspace y estado git. */
export function crearTransporteArchivos(
  cliente: ClienteApi,
): Pick<
  Transporte,
  | 'workspaceInfo'
  | 'workspaceListarEntrada'
  | 'workspaceLeerArchivo'
  | 'workspaceAbrirCon'
  | 'workspaceBuscar'
  | 'workspaceGitEstado'
  | 'workspaceGitRepos'
> {
  const http = cliente.http;
  return {
    workspaceInfo: async () =>
      http<{ ruta: string; nombre: string }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/info`,
      ),
    workspaceListarEntrada: async (ruta, profundidad) => {
      // [089A-10] El backend limita profundidad a 0..=3; el panel Files usa
      // profundidad 1 para la raíz (carpetas expandibles) y 0 para el resto.
      const nivel = profundidad && profundidad > 0 ? `&profundidad=${profundidad}` : '';
      return http<ListadoWorkspace>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/listar?ruta=${encodeURIComponent(ruta)}${nivel}`,
      );
    },
    workspaceLeerArchivo: async (ruta) =>
      http<{ ruta: string; lineas: number; contenido: string }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/leer?ruta=${encodeURIComponent(ruta)}`,
      ),
    workspaceAbrirCon: async () => {
      throw new Error('abrir archivos con otra aplicación no está disponible en modo web');
    },
    workspaceBuscar: async (consulta, ruta) => {
      const extra = ruta ? `&ruta=${encodeURIComponent(ruta)}` : '';
      return http<ResultadoBusqueda>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/buscar?consulta=${encodeURIComponent(consulta)}${extra}`,
      );
    },
    workspaceGitEstado: async (ruta?): Promise<EstadoGit> => {
      // [139A-2] El modo web no expone repos por ruta: nunca se devuelve el
      // área haciéndola pasar por el repo (explícito en vez de dato erróneo).
      if (ruta) throw new Error('estado por repo no disponible en modo web');
      return http<EstadoGit>('GET', `/api/v1/session/${cliente.getSid()}/git/estado`);
    },
    // [139A-2] Sin espejo `git/repos` en el servidor web: el front usa el
    // modo simple (una sola consulta al área).
    workspaceGitRepos: async (): Promise<RepoGit[]> => {
      throw new Error('repos multi-git no disponibles en modo web');
    },
  };
}
