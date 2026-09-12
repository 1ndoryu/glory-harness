/* Acciones de proyecto de la barra lateral (extraído de barraLateral.ts [119A-2 F1]).
 * Handlers del menú contextual de proyecto: renombrar, quitar, revelar en
 * el Explorador, fijar y batch de hilos (archivar/eliminar). Todo el estado
 * compartido llega por `deps`; sin importes del orquestador. */

import type { BarraLateralDeps } from './barraLateralTipos';

export interface AccionesProyecto {
  onRenombrarProyecto(id: string, nombre: string): void;
  onEliminarProyecto(id: string): void;
  onRevelarProyecto(id: string): void;
  onFijarProyecto(id: string, fijado: boolean): void;
  onArchivarHilosProyecto(id: string): void;
  onEliminarHilosProyecto(id: string): void;
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
    // [119A-2 F2] Abre la carpeta en el Explorador; sin resync (no muta
    // estado) y en web el transporte rechaza con aviso visible.
    onRevelarProyecto(id) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          await deps.adaptador.sesion.workspaces.revelar(id);
        } catch (e: unknown) {
          deps.avisar(`no se pudo abrir la carpeta: ${String(e)}`, '', '');
        }
      })();
    },
    // [119A-2 F3] Fija/suelta el proyecto; el orden cambia así que se
    // resincroniza (los fijados van primero).
    onFijarProyecto(id, fijado) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          const ok = await deps.adaptador.sesion.workspaces.fijar(id, fijado);
          if (!ok) deps.avisar('el backend no fijó el proyecto (id ajeno o inexistente)', '', '');
          await deps.resincronizarSidebar();
        } catch (e: unknown) {
          deps.avisar(`no se pudo fijar el proyecto: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
    // [119A-2 F4] Archiva todos los hilos del proyecto (reversible hilo a
    // hilo, sin confirmación como el archivado single); el resync reconcilia.
    onArchivarHilosProyecto(id) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          await deps.adaptador.sesion.archivarProyecto(id, true);
          await deps.resincronizarSidebar();
        } catch (e: unknown) {
          deps.avisar(`no se pudieron archivar los hilos: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
    // [119A-2 F4] Elimina todos los hilos del proyecto (el menú ya pidió
    // confirmación); re-ancla los paneles que mostraban un hilo borrado.
    onEliminarHilosProyecto(id) {
      if (!deps.usaReal) return;
      const borradas = new Set(
        deps.getConversaciones().filter((c) => c.workspaceId === id).map((c) => c.id),
      );
      void (async () => {
        try {
          const actual = await deps.adaptador.sesion.eliminarProyecto(
            id,
            deps.panelActivo()?.tipo,
          );
          await deps.resincronizarSidebar();
          deps.paneles.forEach((p) => {
            if (p.conversaId && borradas.has(p.conversaId)) {
              if (actual) void p.cargarConversacion(actual.id);
              else p.ponerBorrador();
            }
          });
        } catch (e: unknown) {
          deps.avisar(`no se pudieron borrar los hilos: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
  };
}
