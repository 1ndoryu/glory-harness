/* Fábrica de paneles de chat (extraída de main.ts [089A-16 F1b]).
 * El principal recibe los PROVEEDORES (selector de modelo) y el panelMeta
 * global montado DENTRO de su entrada (único M1). El lateral también recibe
 * PROVEEDORES (misma entrada completa, con modelo/modo/razonamiento
 * compartidos M1); no tiene panelMeta propio. Todo el estado compartido
 * llega por `deps`; sin importes del orquestador (sin ciclos). */

import type {
  Conversacion,
  ModeloSeleccionado,
  ProveedorModelo,
  Workspace,
} from '../dominio/tipos';
import type { ModoEjecucion } from '../componentes/entrada';
import { montarPanelChat, type PanelChat, type TipoPanel } from '../componentes/panelChat';
import type { PanelMeta } from '../componentes/panelMeta';
import type { Sidebar } from '../componentes/sidebar';
import type { ModalConfiguracion } from '../componentes/modal';
import type { AdaptadorReal } from '../tauri/real';
import { crearSimulacion } from '../simulacion/simulacion';

export interface CrearPanelDeps {
  paneles: PanelChat[];
  adaptador: AdaptadorReal;
  simulacion: ReturnType<typeof crearSimulacion>;
  usaReal: boolean;
  usaMock: boolean;
  panelMeta: PanelMeta;
  sidebar: Sidebar;
  modal: ModalConfiguracion;
  proveedores: ProveedorModelo[];
  getConversaciones: () => Conversacion[];
  getModelo: () => ModeloSeleccionado;
  setModelo: (m: ModeloSeleccionado) => void;
  getModo: () => ModoEjecucion;
  setModo: (m: ModoEjecucion) => void;
  getRazonamiento: () => string;
  setRazonamiento: (r: string) => void;
  getProyectos: () => Workspace[];
  getProyectoActivoId: () => string | null;
  getPrincipal: () => PanelChat | null;
  hayTurnoGlobal: () => boolean;
  notificarTurnoInicio: () => void;
  notificarTurnoFin: () => void;
  registrarUltimoEnvio: (panel: PanelChat) => void;
  panelActivo: () => PanelChat | null;
  activarPanel: (panel: PanelChat | null) => void;
  resincronizarSidebar: () => Promise<void>;
  sincronizarPanelMeta: () => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
  alternarSidebar: () => void;
  alternarPanelDerecho: () => void;
  abrirAcciones: (panel: PanelChat, rect: DOMRect) => void;
}

export function crearPanel(
  deps: CrearPanelDeps,
  tipo: TipoPanel,
  idPrefijo: string,
  opts: { onCerrar?: () => void } = {},
): PanelChat {
  const panel = montarPanelChat({
    tipo,
    idPrefijo,
    proveedores: deps.proveedores,
    deps: {
      adaptador: deps.adaptador,
      simulacion: deps.simulacion,
      usaReal: deps.usaReal,
      usaMock: deps.usaMock,
      panelMeta: deps.panelMeta,
      sidebar: deps.sidebar,
      conversaciones: deps.getConversaciones,
      getModelo: deps.getModelo,
      getModo: deps.getModo,
      getRazonamiento: deps.getRazonamiento,
      getWorkspaces: deps.getProyectos,
      getWorkspaceSeleccionadoId: deps.getProyectoActivoId,
      prepararWorkspaceSeleccionado: async () => {
        // No-op: la activación se delega al callback.
      },
      onWorkspaceCambiado(id) {
        // Siempre hay un workspace activo (el agente trabaja en algún
        // área). Si id es null no se hace nada (fallback al activo actual).
        if (id === null) return;
        const ws = deps.getProyectos().find((p) => p.id === id);
        if (!ws) return;
        if (deps.hayTurnoGlobal()) return;
        void (async () => {
          try {
            await deps.adaptador.sesion.workspaces.activarPorRuta(ws.ruta);
            // onSesion refrescará sidebar + lista.
            const principal = deps.getPrincipal();
            if (principal) {
              principal.ponerBorrador();
              deps.activarPanel(principal);
            }
          } catch (e: unknown) {
            deps.avisar(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
          }
        })();
      },
      hayTurnoGlobal: deps.hayTurnoGlobal,
      notificarTurnoInicio: deps.notificarTurnoInicio,
      notificarTurnoFin: deps.notificarTurnoFin,
      registrarUltimoEnvio: deps.registrarUltimoEnvio,
      resincronizarSidebar: deps.resincronizarSidebar,
      onConversacionCambio(id) {
        // Al cambiar la conversación del panel ENFOCADO, la sidebar lo marca.
        // [069A-7] `null` (borrador) → deselecciona la lista.
        if (deps.panelActivo() === panel) {
          if (id) deps.sidebar.seleccionar(id);
          else deps.sidebar.seleccionar('');
        }
        // El panel meta no forma parte del borrador inicial: aparece al
        // escribir el primer mensaje o al cargar una conversación real.
        deps.sincronizarPanelMeta();
      },
    },
    onAcciones(rect) {
      // Menú ⋯ de la cabecera de ESTE panel.
      deps.abrirAcciones(panel, rect);
    },
    onCerrar: opts.onCerrar,
    onToggleSidebar() {
      deps.alternarSidebar();
    },
    // Toggle del panel derecho (solo el principal lo muestra).
    onTogglePanelDerecho: tipo === 'principal' ? () => deps.alternarPanelDerecho() : undefined,
    onModeloCambiado(nuevo) {
      deps.setModelo(nuevo);
      // Propaga a TODOS los paneles (ambos son completos y comparten el
      // runtime M1): el selector del panel que cambió ya está actualizado;
      // el resto se sincroniza aquí.
      deps.paneles.forEach((p) => p.setModelo(nuevo));
      deps.modal.setModelo(nuevo);
      if (deps.usaReal) {
        void deps.adaptador.sesion
          .configGuardar('proveedor', nuevo.proveedor)
          .then(() => deps.adaptador.sesion.configGuardar('modelo', nuevo.modelo))
          .catch((e: unknown) => deps.avisar(`no se pudo guardar el modelo: ${String(e)}`, '', ''));
      }
    },
    onModoCambiado(nuevo) {
      deps.setModo(nuevo);
      // Sincroniza el modo en ambos paneles (M1 compartido).
      deps.paneles.forEach((p) => p.setModo(nuevo));
      deps.modal.asignarValor('modo', nuevo);
      if (deps.usaReal) {
        void deps.adaptador.sesion
          .configGuardar('modo', nuevo)
          .catch((e: unknown) => deps.avisar(`no se pudo guardar el modo: ${String(e)}`, '', ''));
      }
      deps.sincronizarPanelMeta();
    },
    onRazonamientoCambiado(nuevo) {
      deps.setRazonamiento(nuevo);
      // Sincroniza el razonamiento en ambos paneles (M1).
      deps.paneles.forEach((p) => p.setRazonamiento(nuevo));
      deps.modal.asignarValor('nivelRazonamiento', nuevo);
      if (deps.usaReal) {
        void deps.adaptador.sesion
          .configGuardar('nivelRazonamiento', nuevo)
          .catch((e: unknown) =>
            deps.avisar(`no se pudo guardar el razonamiento: ${String(e)}`, '', ''),
          );
      }
    },
    onWorkspaceCambiado(id) {
      // Siempre hay un workspace activo. Si id es null, no se hace nada.
      if (id === null) return;
      const ws = deps.getProyectos().find((p) => p.id === id);
      if (!ws) return;
      if (deps.hayTurnoGlobal()) return;
      void (async () => {
        try {
          await deps.adaptador.sesion.workspaces.activarPorRuta(ws.ruta);
          const principal = deps.getPrincipal();
          if (principal) {
            principal.ponerBorrador();
            deps.activarPanel(principal);
          }
        } catch (e: unknown) {
          deps.avisar(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
        }
      })();
    },
  });

  if (tipo === 'principal') {
    // El panel meta comparte la entrada principal, pero queda después del
    // selector de workspace. Además permanece oculto hasta que el primer
    // mensaje cree una conversación real; así el borrador inicial solo ocupa
    // el selector y la caja, sin solapamientos.
    const entradaRaiz = panel.raiz.querySelector<HTMLElement>('.entrada');
    const selectorWorkspace = entradaRaiz?.querySelector('.selector-workspace-box');
    if (entradaRaiz && selectorWorkspace) {
      entradaRaiz.insertBefore(deps.panelMeta.raiz, selectorWorkspace.nextSibling);
    }
  }

  deps.paneles.push(panel);
  return panel;
}
