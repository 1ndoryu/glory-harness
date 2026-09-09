/* Estado de sesión/proyectos de la vista: conversaciones, proyectos,
 * proyecto activo y suscriptores de cambio de workspace. */
import type {
  Conversacion,
  ModeloSeleccionado,
  Workspace,
} from '../dominio/tipos';
import type { PanelChat } from '../componentes/panelChat';
import type { Sidebar } from '../componentes/sidebar';
import type { InfoConversacion, InfoSesion } from '../tauri/real';

export interface SesionVistaDeps {
  usaReal: boolean;
  conversacionesIniciales: Conversacion[];
  paneles: () => PanelChat[];
  getSidebar: () => Sidebar;
  getEstadoVista: () => { modelo: ModeloSeleccionado };
  setModeloEnModal: (m: ModeloSeleccionado) => void;
  listarConversaciones: () => Promise<InfoConversacion[]>;
  listarWorkspaces: () => Promise<{
    workspaces: Workspace[];
    activa: Workspace | null;
  }>;
}

export interface SesionVista {
  getConversaciones: () => Conversacion[];
  setConversaciones: (lista: Conversacion[]) => void;
  getProyectos: () => Workspace[];
  getProyectoActivo: () => Workspace | null;
  getProyectoActivoId: () => string | null;
  onCambioWorkspace: (fn: (ruta: string | null) => void) => void;
  sincronizarModeloDesdeSesion: (info: InfoSesion) => void;
  resincronizarSidebar: () => Promise<void>;
  refrescarProyectos: () => Promise<void>;
}

export function crearSesionVista(deps: SesionVistaDeps): SesionVista {
  let conversaciones = deps.conversacionesIniciales;
  let proyectos: Workspace[] = [];
  let proyectoActivo: Workspace | null = null;
  const suscriptoresCambioWorkspace: Array<(ruta: string | null) => void> = [];

  function sincronizarModeloDesdeSesion(info: InfoSesion): void {
    const corte = info.modelo.indexOf('/');
    if (corte < 0) return;
    const proveedor = info.modelo.slice(0, corte);
    const modelo = info.modelo.slice(corte + 1);
    const estadoVista = deps.getEstadoVista();
    if (
      proveedor === estadoVista.modelo.proveedor &&
      modelo === estadoVista.modelo.modelo
    )
      return;
    estadoVista.modelo = { proveedor, modelo, nombre: modelo };
    deps.paneles().forEach((p) => p.setModelo(estadoVista.modelo));
    deps.setModeloEnModal(estadoVista.modelo);
  }

  async function resincronizarSidebar(): Promise<void> {
    if (!deps.usaReal) return;
    await refrescarProyectos();
    const lista = await deps.listarConversaciones();
    conversaciones = lista.map((c) => ({
      id: c.id,
      titulo: c.titulo,
      archivada: c.archivada,
      workspaceId: c.workspace_id,
      workspaceNombre: c.workspace_nombre,
    }));
    deps.getSidebar().sustituir(conversaciones);
  }

  /** [069A-Proyectos] Refresca el estado de proyectos desde el backend. */
  async function refrescarProyectos(): Promise<void> {
    if (!deps.usaReal) return;
    try {
      const res = await deps.listarWorkspaces();
      proyectos = res.workspaces;
      const rutaAnterior = proyectoActivo?.ruta ?? null;
      const rutaNueva = res.activa?.ruta ?? null;
      proyectoActivo = res.activa;
      deps.getSidebar().sustituirProyectos(proyectos, proyectoActivo);
      // Sincroniza el selector de workspace de todos los paneles.
      const seleccionadoId = proyectoActivo?.id ?? null;
      deps
        .paneles()
        .forEach((p) => p.setWorkspaces(proyectos, seleccionadoId));
      // [089A-11] El área de trabajo cambió (ruta distinta): Files/Git
      // resuelven la raíz en el backend, así que recargan las tabs abiertas.
      if (rutaAnterior !== rutaNueva) {
        suscriptoresCambioWorkspace.forEach((fn) => fn(rutaNueva));
      }
    } catch {
      // silencioso: el sidebar conserva el último estado válido
    }
  }

  return {
    getConversaciones: () => conversaciones,
    setConversaciones: (lista) => {
      conversaciones = lista;
    },
    getProyectos: () => proyectos,
    getProyectoActivo: () => proyectoActivo,
    getProyectoActivoId: () => proyectoActivo?.id ?? null,
    onCambioWorkspace: (fn) => {
      suscriptoresCambioWorkspace.push(fn);
    },
    sincronizarModeloDesdeSesion,
    resincronizarSidebar,
    refrescarProyectos,
  };
}
