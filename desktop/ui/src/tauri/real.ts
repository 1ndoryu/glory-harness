// ============================================================
// Adaptador real (plan 039A-1, Fases 4-5): la UI habla al núcleo
// vía comandos in-process (Tauri) o HTTP/SSE (web) según el transporte.
// Misma superficie que la simulación (montar/detener) para no tocar
// el layout: solo cambia la fuente de los eventos.
//
// Split 089A-16 F2: tipos en `realTipos.ts`, transporte Tauri en
// `transporteTauri.ts`, descripción de tools en
// `descripcionHerramientas.ts`, render de eventos en `aplicarEventos.ts`
// y núcleo del turno en `turnoReal.ts`. Aquí solo la fachada de sesión
// (conversaciones, workspace, proyectos, filesystem) + re-exports para
// no tocar los ~16 importadores existentes.
// ============================================================

import type { EstadoGit } from '../componentes/panelGit';
import type {
  ListadoWorkspace,
  ResultadoBusqueda,
  Workspace,
} from '../dominio/tipos';
import type {
  CargaConversacion,
  HooksAdaptador,
  InfoConversacion,
  InfoSesion,
  ProveedorInfo,
  ResultadoRestauracionTramo,
  Transporte,
} from './realTipos';
import { transporteTauri } from './transporteTauri';
import { crearTurnoReal } from './turnoReal';

export * from './realTipos';
export { transporteTauri } from './transporteTauri';
export { compacto, descripcionDeTool, iconoDeTool, rutaDeArgumentos } from './descripcionHerramientas';
export { crearTurnoReal, type TurnoReal } from './turnoReal';

export function crearAdaptadorReal(hooks: HooksAdaptador = {}, transporte: Transporte = transporteTauri()) {
  const turno = crearTurnoReal(hooks, transporte);

  return {
    montar: turno.montar,
    detener: turno.detener,
    usoUltimoTurno: turno.usoUltimoTurno,
    resultadoUltimoTurno: turno.resultadoUltimoTurno,
    /** Abre la sesión si aún no existe (para listar/cargar al arrancar). */
    asegurarSesion: turno.asegurarSesion,
    sesion: {
      /** [039A-3 P5] Crea una conversación en el panel dado (default
       * 'principal'). Devuelve la conversación nueva. */
      async nueva(titulo?: string, panelId?: string): Promise<InfoConversacion> {
        return transporte.convNueva(titulo ?? null, panelId ?? null);
      },
      async listar(): Promise<InfoConversacion[]> {
        return transporte.convListar();
      },
      /** [039A-3 P5] Carga una conversación en el panel dado. */
      async cargar(id: string, panelId?: string): Promise<CargaConversacion> {
        return transporte.convCargar(id, panelId ?? null);
      },
      async renombrar(id: string, titulo: string): Promise<boolean> {
        return transporte.convRenombrar(id, titulo);
      },
      async archivar(id: string, archivada: boolean): Promise<boolean> {
        return transporte.convArchivar(id, archivada);
      },
      /** [039A-3 P5] Si era la actual del panel, el backend ancla la más
       * reciente restante y la devuelve; si no queda ninguna, `null`
       * (borrador create-on-write, sin fila fantasma). [069A-7] */
      async eliminar(id: string, panelId?: string): Promise<InfoConversacion | null> {
        return transporte.convEliminar(id, panelId ?? null);
      },
      /** [039A-3 P2] Borra el hilo posterior a un mensaje de usuario y
       * devuelve la conversación recién recortada. `editar=false` conserva el
       * mensaje objetivo ("volver a este punto"); `editar=true` lo borra
       * también (se reescribe al reenviar desde el modo edición).
       * [039A-3 P5] Opera sobre el panel dado. */
      async rewind(
        hastaMensajeId: string,
        editar: boolean,
        panelId?: string,
      ): Promise<CargaConversacion> {
        return transporte.convRewind(hastaMensajeId, editar, panelId ?? null);
      },
      /** [039A-3 P3] Restaura los archivos del último tramo rebobinado (acción
       * EXPLÍCITA tras "volver a punto"). Falla si no hay tramo pendiente.
       * [039A-3 P5] Opera sobre el tramo del panel dado. */
      async restaurarTramo(panelId?: string): Promise<ResultadoRestauracionTramo> {
        return transporte.tramoRestaurar(panelId ?? null);
      },
      async proveedores(): Promise<ProveedorInfo[]> {
        return transporte.leerProveedores();
      },
      async configLeer(clave: string): Promise<string | null> {
        return transporte.leerConfig(clave);
      },
      async configGuardar(clave: string, valor: string): Promise<void> {
        await transporte.guardarConfig(clave, valor);
      },
      /**
       * Diálogo nativo de carpeta. Reabre la sesión (conversación nueva).
       * Si el usuario cancela, devuelve la sesión actual sin cambios.
       */
      async elegirWorkspace(): Promise<InfoSesion> {
        const info = await transporte.elegirWorkspace();
        turno.avisarSesionAbierta(info);
        return info;
      },
      /** [069A-2 F4] Fija el workspace por ruta (modo web). En Tauri se
       * rechaza: el diálogo nativo es el dueño. */
      async fijarWorkspace(ruta: string): Promise<InfoSesion> {
        const info = await transporte.fijarWorkspace(ruta);
        turno.avisarSesionAbierta(info);
        return info;
      },
      async actualizarMeta(meta: string | null): Promise<string | null> {
        return transporte.fijarMeta(meta);
      },
      // [069A-Proyectos] Proyectos (áreas de trabajo).
      workspaces: {
        async listar(): Promise<{ workspaces: Workspace[]; activa: Workspace | null }> {
          return transporte.workspacesListar();
        },
        async guardarProyecto(nombre: string, ruta: string): Promise<InfoSesion> {
          const info = await transporte.proyectoGuardar(nombre, ruta);
          turno.avisarSesionAbierta(info);
          return info;
        },
        async activarPorRuta(ruta: string): Promise<InfoSesion> {
          const info = await transporte.workspaceActivarsPorRuta(ruta);
          turno.avisarSesionAbierta(info);
          return info;
        },
        async renombrar(id: string, nombre: string): Promise<boolean> {
          return transporte.workspaceRenombrar(id, nombre);
        },
        async eliminar(id: string): Promise<boolean> {
          return transporte.workspaceEliminar(id);
        },
      },
      filesystem: {
        async info(): Promise<{ ruta: string; nombre: string }> {
          return transporte.workspaceInfo();
        },
        async listar(ruta: string, profundidad?: number): Promise<ListadoWorkspace> {
          return transporte.workspaceListarEntrada(ruta, profundidad);
        },
        async leer(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }> {
          return transporte.workspaceLeerArchivo(ruta);
        },
        async abrirCon(ruta: string): Promise<void> {
          return transporte.workspaceAbrirCon(ruta);
        },
        async buscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda> {
          return transporte.workspaceBuscar(consulta, ruta);
        },
        async gitEstado(): Promise<EstadoGit> {
          return transporte.workspaceGitEstado();
        },
      },
    },
  };
}

export type AdaptadorReal = ReturnType<typeof crearAdaptadorReal>;
