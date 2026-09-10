/* Transporte Tauri in-process (comportamiento 039A-1 intacto): cada método es
 * un `invoke` directo al comando del backend con el mismo nombre. */
import { invoke } from '@tauri-apps/api/core';
import type { EstadoGit } from '../componentes/panelGit';
import type { ListadoWorkspace, ResultadoBusqueda, Workspace } from '../dominio/tipos';
import type {
  CargaConversacion,
  ComandoArea,
  InfoConversacion,
  InfoSesion,
  ListadoMemoria,
  ProveedorInfo,
  ResultadoCarpetaMemoria,
  ResultadoRestauracionTramo,
  Transporte,
} from './realTipos';

/** Transporte Tauri in-process (comportamiento 039A-1 intacto). */
export function transporteTauri(): Transporte {
  return {
    abrirSesion: (opts) =>
      invoke<InfoSesion>('abrir_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        dir: null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      }),
    reconfigurarSesion: (opts) =>
      invoke<InfoSesion>('reconfigurar_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      }),
    enviarTurno: (mensaje, panelId) => invoke<void>('enviar_turno', { mensaje, panel_id: panelId }),
    detenerTurno: (panelId) => {
      void invoke('cancelar_turno', { panel_id: panelId }).catch(() => {});
    },
    responderAprobacion: (id, respuesta) =>
      invoke<void>('responder_aprobacion', { id, respuesta }),
    pendientesAprobacion: () => invoke<unknown[]>('pendientes_aprobacion'),
    requiereReenvioTrasAprobar: () => true,
    escucharTurno: async (onEvento, onFin) => {
      await listenTurno(onEvento, onFin);
    },
    convNueva: (titulo, panelId) =>
      invoke<InfoConversacion>('conversacion_nueva', { titulo, panel_id: panelId }),
    convListar: () => invoke<InfoConversacion[]>('listar_conversaciones'),
    convCargar: (id, panelId) =>
      invoke<CargaConversacion>('cargar_conversacion', { id, panel_id: panelId }),
    convRenombrar: (id, titulo) => invoke<boolean>('renombrar_conversacion', { id, titulo }),
    convArchivar: (id, archivada) => invoke<boolean>('archivar_conversacion', { id, archivada }),
    convEliminar: (id, panelId) =>
      invoke<InfoConversacion>('eliminar_conversacion', { id, panel_id: panelId }),
    convRewind: (hastaMensajeId, editar, panelId) =>
      invoke<CargaConversacion>('rewind_conversacion', {
        hastaMensajeId,
        editar,
        panel_id: panelId,
      }),
    tramoRestaurar: (panelId) =>
      invoke<ResultadoRestauracionTramo>('restaurar_archivos_tramo', { panel_id: panelId }),
    leerProveedores: () => invoke<ProveedorInfo[]>('proveedores_disponibles'),
    leerConfig: (clave) => invoke<string | null>('config_leer', { clave }),
    guardarConfig: (clave, valor) => invoke<void>('config_guardar', { clave, valor }),
    elegirWorkspace: () => invoke<InfoSesion>('elegir_workspace'),
    fijarWorkspace: () =>
      Promise.reject(
        new Error('en Tauri el workspace se elige con el diálogo nativo (elegirWorkspace)'),
      ),
    fijarMeta: (meta) => invoke<string | null>('actualizar_meta', { meta }),
    // [069A-Proyectos] Tauri: invoke directo a comandos del backend.
    workspacesListar: () =>
      invoke<{ workspaces: Workspace[]; activa: Workspace | null }>('workspaces_listar'),
    proyectoGuardar: (nombre, ruta) =>
      invoke<InfoSesion>('workspace_crear_o_activar', { nombre, ruta }),
    workspaceActivarsPorRuta: (ruta) =>
      invoke<InfoSesion>('workspace_activar_por_ruta', { ruta }),
    workspaceRenombrar: (id, nombre) =>
      invoke<boolean>('workspace_renombrar', { id, nombre }),
    workspaceEliminar: (id) => invoke<boolean>('workspace_eliminar', { id }),
    workspaceInfo: () => invoke<{ ruta: string; nombre: string }>('workspace_info'),
    workspaceListarEntrada: (ruta, profundidad) =>
      invoke<ListadoWorkspace>('workspace_listar_entrada', { rutaRelativa: ruta, profundidad }),
    workspaceLeerArchivo: (ruta) =>
      invoke<{ ruta: string; lineas: number; contenido: string }>('workspace_leer_archivo', {
        rutaRelativa: ruta,
      }),
    workspaceAbrirCon: (ruta) =>
      invoke<void>('workspace_abrir_con', { rutaRelativa: ruta }),
    workspaceBuscar: (consulta, ruta) =>
      invoke<ResultadoBusqueda>('workspace_buscar', {
        consulta,
        rutaRelativa: ruta ?? null,
      }),
    workspaceGitEstado: () =>
      invoke<EstadoGit>('workspace_git_estado'),
    // [109A-3] Memorias: el backend resuelve el ámbito con el área activa y
    // no acepta rutas del front (export/import van a `.glory/memorias`).
    memoriaListar: () => invoke<ListadoMemoria>('memoria_listar_proyecto'),
    memoriaBorrar: (clave) => invoke<ListadoMemoria>('memoria_borrar', { clave }),
    memoriaCurar: () => invoke<string>('memoria_curar'),
    memoriaExportar: () => invoke<ResultadoCarpetaMemoria>('memoria_exportar'),
    memoriaImportar: () => invoke<ResultadoCarpetaMemoria>('memoria_importar'),
    // [109A-4] Comandos del área activa: el backend resuelve la carpeta
    // (`.glory/comandos`) como el CLI; el front nunca envía rutas.
    comandosListar: () => invoke<ComandoArea[]>('comandos_listar'),
    comandoExpandir: (nombre, argumentos) =>
      invoke<string>('comando_expandir', { nombre, argumentos }),
  };
}

async function listenTurno(
  onEvento: (ev: import('./realTipos').AgenteEvento) => void,
  onFin: (ok: boolean, error?: string) => void,
): Promise<void> {
  const { listen } = await import('@tauri-apps/api/event');
  await listen<import('./realTipos').AgenteEvento>('agente-evento', (e) => onEvento(e.payload));
  await listen<{ ok: boolean; error?: string }>('turno-fin', (e) =>
    onFin(e.payload.ok, e.payload.error),
  );
}
