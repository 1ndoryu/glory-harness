/* Contratos de la barra lateral (extraído de barraLateral.ts [089A-16 B-ISP]).
 * Subinterfaces cohesivas por ISP; `BarraLateralDeps` las compone por `extends`
 * para que el objeto literal del orquestador no cambie. */
import type { BarraSuperior } from '../componentes/barraSuperior';
import type { Conversacion, Workspace } from '../dominio/tipos';
import type { PanelChat } from '../componentes/panelChat';
import type { Sidebar } from '../componentes/sidebar';
import type { AdaptadorReal } from '../tauri/real';
import type { PersistenciaDeps } from './persistencia';

/** Núcleo de montaje: DOM, barra, adaptador y persistencia. */
export interface BarraLateralNucleo {
  cuerpo: HTMLElement;
  barra: BarraSuperior;
  adaptador: AdaptadorReal;
  usaReal: boolean;
  usaMock: boolean;
  persistencia: PersistenciaDeps;
}

/** Datos iniciales y acceso al estado de conversaciones. */
export interface BarraLateralDatos {
  conversacionesIniciales: Conversacion[];
  proyectosIniciales: Workspace[];
  proyectoActivoInicial: Workspace | null;
  getConversaciones: () => Conversacion[];
  setConversaciones: (c: Conversacion[]) => void;
  getTurnoGlobal: () => boolean;
}

/** Paneles abiertos y activación. */
export interface BarraLateralPaneles {
  paneles: PanelChat[];
  panelActivo: () => PanelChat | null;
  activarPanel: (panel: PanelChat | null) => void;
  getPrincipal: () => PanelChat | null;
  abrirEnLateral: (id: string) => void;
}

/** Avisos y aperturas delegadas al orquestador. */
export interface BarraLateralAcciones {
  avisar: (texto: string, meta: string, detalle: string) => void;
  resincronizarSidebar: () => Promise<void>;
  abrirConfig: () => void;
  abrirModalProyecto: () => void;
  alternarNavegador: () => void;
}

export interface BarraLateralDeps
  extends BarraLateralNucleo, BarraLateralDatos, BarraLateralPaneles,
    BarraLateralAcciones {}

export interface BarraLateral {
  sidebar: Sidebar;
  grip: HTMLElement;
  alternarSidebar: () => void;
  pintarSidebar: () => void;
  fijarAbierta: (abierta: boolean) => void;
  renombrarEnLista: (id: string, titulo: string) => Promise<void>;
}
