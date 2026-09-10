// Fachada del adaptador real (split 089A-16 F2-resto): `crearAdaptadorReal`
// vive aquí; `real.ts` queda como barrel. Sin ciclos: solo importa tipos de
// dominio, tipos/transporte/turno propios y el tipo de estado git.

import type { EstadoGit } from '../componentes/panelGit';
import type {
  ListadoWorkspace,
  ResultadoBusqueda,
  Workspace,
} from '../dominio/tipos';
import type {
  CargaConversacion,
  ComandoArea,
  HooksAdaptador,
  InfoConversacion,
  InfoSesion,
  ListadoMemoria,
  ProveedorInfo,
  ResultadoCarpetaMemoria,
  ResultadoRestauracionTramo,
  ResumenCompactacion,
  Transporte,
} from './realTipos';
import { transporteTauri } from './transporteTauri';
import { crearTurnoReal } from './turnoReal';

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
      /** [109A-4 F3] Compacta el contexto de la conversación del panel
       * (`/compactar`). El backend decide con el historial real: devuelve el
       * ahorro logrado o, si no había material, el `motivo` del no-op.
       * [039A-3 P5] Opera sobre el panel dado (`principal`/`lateral`). */
      async compactar(panelId?: string, instruccion?: string | null): Promise<ResumenCompactacion> {
        return transporte.compactarConversacion(panelId ?? null, instruccion ?? null);
      },
      /** [109A-4 F4] ¿Este transporte sabe correr un turno solo-lectura
       * (`/meta`)? Tauri sí (política del núcleo); el modo web no expone
       * política por turno y el comando lo avisa en vez de simularlo. */
      soportaSoloLectura(): boolean {
        return transporte.soportaSoloLectura();
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
      // [109A-3] Memorias del proyecto activo (panel "Memorias"). El ámbito
      // lo decide el backend con el área activa de la sesión: aquí no viaja
      // ningún identificador de proyecto.
      memorias: {
        async listar(): Promise<ListadoMemoria> {
          return transporte.memoriaListar();
        },
        async borrar(clave: string): Promise<ListadoMemoria> {
          return transporte.memoriaBorrar(clave);
        },
        async curar(): Promise<string> {
          return transporte.memoriaCurar();
        },
        async exportar(): Promise<ResultadoCarpetaMemoria> {
          return transporte.memoriaExportar();
        },
        async importar(): Promise<ResultadoCarpetaMemoria> {
          return transporte.memoriaImportar();
        },
      },
      // [109A-4] Comandos `/` del área activa: el backend resuelve la carpeta
      // (`.glory/comandos`); aquí no viaja ninguna ruta del front.
      comandos: {
        async listar(): Promise<ComandoArea[]> {
          return transporte.comandosListar();
        },
        async expandir(nombre: string, argumentos: string): Promise<string> {
          return transporte.comandoExpandir(nombre, argumentos);
        },
      },
    },
  };
}

export type AdaptadorReal = ReturnType<typeof crearAdaptadorReal>;
