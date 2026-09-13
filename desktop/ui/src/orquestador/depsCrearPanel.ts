/* Deps de la fábrica de paneles (extraídas de main.ts [fix 12-09] para el
 * techo de 300 líneas).
 *
 * `abrirAcciones` cierra sobre `depsLaterales` y `getPrincipal`/`alternarPanelDerecho`
 * sobre `principal`/`todoPanelDerecho` (declarados más abajo en main.ts): solo se
 * invocan desde eventos de UI posteriores al arranque, fuera de la TDZ. */

import type { ProveedorModelo } from '../dominio/tipos';
import type { crearSimulacion } from '../simulacion/simulacion';
import type { AdaptadorReal } from '../tauri/real';
import type { PanelChat } from '../componentes/panelChat';
import type { PanelMeta } from '../componentes/panelMeta';
import type { Sidebar } from '../componentes/sidebar';
import type { SesionVista } from './sesionVista';
import type { VistaMeta } from './vistaMeta';
import type { VistaModal } from './vistaModal';
import { type CrearPanelDeps } from './crearPanel';

export interface DepsCrearPanelCtx {
  paneles: PanelChat[];
  adaptador: AdaptadorReal;
  simulacion: ReturnType<typeof crearSimulacion>;
  usaReal: boolean;
  usaMock: boolean;
  panelMeta: PanelMeta;
  sidebar: Sidebar;
  vistaModal: VistaModal;
  proveedores: ProveedorModelo[];
  sesionVista: SesionVista;
  vistaMeta: VistaMeta;
  getPrincipal: () => PanelChat;
  panelActivo: () => PanelChat | null;
  activarPanel: (panel: PanelChat | null) => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
  alternarSidebar: () => void;
  alternarPanelDerecho: () => void;
  abrirAcciones: (panel: PanelChat, rect: DOMRect) => void;
  verEnCambios: (ruta: string) => void;
  /** [139A-2] Cambios se revalida al cerrar cada turno y al cambiar la
   * conversación enfocada (cierres perezosos de runtime, como `verEnCambios`). */
  alTerminarTurno: () => void;
  alCambiarConversacion: () => void;
  /** [129A-10 F1] El orquestador levanta vetos de UI al iniciar cada turno
   * (cierre perezoso de runtime, como `verEnCambios`). */
  alIniciarTurno: () => void;
}

export function crearDepsCrearPanel(c: DepsCrearPanelCtx): CrearPanelDeps {
  return {
    paneles: c.paneles,
    adaptador: c.adaptador,
    simulacion: c.simulacion,
    usaReal: c.usaReal,
    usaMock: c.usaMock,
    panelMeta: c.panelMeta,
    sidebar: c.sidebar,
    modal: c.vistaModal.modal,
    proveedores: c.proveedores,
    getConversaciones: c.sesionVista.getConversaciones,
    getModelo: () => c.vistaModal.estado.modelo,
    setModelo: (m) => {
      c.vistaModal.estado.modelo = m;
    },
    getModo: () => c.vistaModal.estado.modo,
    setModo: (m) => {
      c.vistaModal.estado.modo = m;
    },
    getRazonamiento: () => c.vistaModal.estado.razonamiento,
    setRazonamiento: (r) => {
      c.vistaModal.estado.razonamiento = r;
    },
    getProyectos: c.sesionVista.getProyectos,
    getProyectoActivoId: c.sesionVista.getProyectoActivoId,
    getPrincipal: c.getPrincipal,
    hayTurnoGlobal: () => c.vistaMeta.hayTurnoGlobal(),
    notificarTurnoInicio: () => {
      c.vistaMeta.notificarTurnoInicio();
      c.alIniciarTurno();
    },
    notificarTurnoFin: () =>
      c.vistaMeta.notificarTurnoFin(() => {
        if (c.usaReal) void c.sesionVista.resincronizarSidebar();
        // [139A-2] El turno cerró: git + vault pueden haber cambiado.
        c.alTerminarTurno();
      }),
    registrarUltimoEnvio: (panel) => c.vistaMeta.registrarUltimoEnvio(panel),
    panelActivo: c.panelActivo,
    activarPanel: c.activarPanel,
    resincronizarSidebar: c.sesionVista.resincronizarSidebar,
    sincronizarPanelMeta: c.vistaMeta.sincronizarPanelMeta,
    avisar: c.avisar,
    alternarSidebar: c.alternarSidebar,
    alternarPanelDerecho: c.alternarPanelDerecho,
    abrirAcciones: c.abrirAcciones,
    verEnCambios: c.verEnCambios,
    alTerminarTurno: c.alTerminarTurno,
    alCambiarConversacion: c.alCambiarConversacion,
  };
}
