/* Acciones de proyecto de la barra lateral (extraído de barraLateral.ts [119A-2 F1]).
 * Handlers del menú contextual de proyecto: renombrar y quitar. Todo el
 * estado compartido llega por `deps`; sin importes del orquestador. */

import type { BarraLateralDeps } from './barraLateralTipos';

export interface AccionesProyecto {
  onRenombrarProyecto(id: string, nombre: string): void;
  onEliminarProyecto(id: string): void;
}

/** [119A-2 F1] Renombra el proyecto (backend + resync si falla) y quita el
 * proyecto del área (las conversaciones huérfanas pasan a "Sin proyecto"
 * tras la resincronización). */
export function crearAccionesProyecto(deps: BarraLateralDeps): AccionesProyecto {
  return {
    onRenombrarProyecto(id, nombre) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          const ok = await deps.adaptador.sesion.workspaces.renombrar(id, nombre);
          if (!ok) {
            deps.avisar('el backend no renombró el proyecto (id ajeno o inexistente)', '', '');
            await deps.resincronizarSidebar();
          }
        } catch (e: unknown) {
          deps.avisar(`no se pudo renombrar el proyecto: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
    onEliminarProyecto(id) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          const ok = await deps.adaptador.sesion.workspaces.eliminar(id);
          if (!ok) deps.avisar('el backend no quitó el proyecto (id ajeno o inexistente)', '', '');
          await deps.resincronizarSidebar();
        } catch (e: unknown) {
          deps.avisar(`no se pudo quitar el proyecto: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
  };
}
