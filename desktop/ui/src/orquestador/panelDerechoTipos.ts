// Contratos del panel derecho del orquestador: núcleo, navegador, piezas,
// visibilidad y aperturas. Extraído de `panelDerecho.ts` para no rebasar el
// techo de líneas; sin cambios de contrato.

import type { PanelDerecho } from '../componentes/panelDerecho';
import type { PanelFiles } from '../componentes/panelFiles';
import type { PanelCambios } from '../componentes/panelCambios';
import type { EventoConsola, PanelConsola } from '../componentes/panelConsola';
import type { ToastGlobal } from '../componentes/toastGlobal';
import type { PanelChat } from '../componentes/panelChat';
import type { FichaSubagente } from '../componentes/subagentesViva';
import type { BarraSuperior } from '../componentes/barraSuperior';
import type { AdaptadorReal } from '../tauri/real';
import type { PersistenciaDeps } from './persistencia';

/** Núcleo del panel derecho: montaje, adaptador y paneles. */
export interface PanelDerechoNucleo {
  cuerpo: HTMLElement;
  app: HTMLElement;
  barra: BarraSuperior;
  adaptador: AdaptadorReal;
  usaTauri: boolean;
  persistencia: PersistenciaDeps;
  paneles: PanelChat[];
  panelActivo: () => PanelChat | null;
  getConversaciones: () => Array<{ id: string }>;
  /** Turno en curso (para la supresión de auto-apertura: igual que F1). */
  turnoEnCurso: () => boolean;
}

/** Navegador embebido y aperturas delegadas. */
export interface PanelDerechoNavegador {
  onCambioWorkspace: (accion: (ruta: string | null) => void) => void;
  navegadorRaiz: HTMLElement;
  mostrarNavegador: (visible: boolean) => void;
  estaNavegadorAbierto: () => boolean;
  abrirNavegador: () => void;
  abrirChatLateral: () => void;
  abrirChatLateralPorId: (id: string) => Promise<void>;
}

export interface PanelDerechoDeps
  extends PanelDerechoNucleo, PanelDerechoNavegador {}

/** Piezas montadas del panel derecho. */
export interface PanelDerechoPiezas {
  panelDerecho: PanelDerecho;
  files: PanelFiles;
  /** [129A-7] La tab "Git local" ahora es "Cambios" (filesystem por turno +
   * estado git debajo). El id de tab 'git' se conserva. */
  cambios: PanelCambios;
  /** [209A-1 F3] La tab Consola: visor de ejecuciones `comando` en vivo. */
  consola: PanelConsola;
  toastGlobal: ToastGlobal;
}

/** Visibilidad del panel derecho (vacío = oculto). */
export interface PanelDerechoVisibilidad {
  asegurarPanelDerecho: () => void;
  ocultarPanelDerecho: () => void;
  alternarPanelDerecho: () => void;
  cerrarPanelDerechoSiVacio: () => void;
  pintarToggleDerecho: () => void;
  restaurarEstado: () => Promise<void>;
}

/** [129A-11 F3] Apertura de la ficha de solo lectura del subagente (tab
 * `subagente:<n>`, sin caja de escritura por construcción). Interfaz aparte
 * para no engordar `PanelDerechoAperturas` (ISP). */
export interface PanelDerechoSubagentes {
  abrirSubagenteEn: (ficha: FichaSubagente) => void;
}

/** Aperturas delegadas (archivos, git, reposicionado del webview). */
export interface PanelDerechoAperturas extends PanelDerechoSubagentes {
  abrirFiles: () => void;
  abrirGit: () => void;
  /** [129A-8] Abre Cambios y revela el archivo (enlace del resumen). */
  abrirCambiosEn: (ruta: string) => void;
  /** [129A-10 F2] Abre Files y previsualiza la ruta (vista del agente). */
  abrirFilesEn: (ruta: string) => void;
  /** [209A-1 F3] Abre la Consola (tab manual: inicio, menú +, persistencia). */
  abrirConsola: () => void;
  /** [209A-1 F3] Abre la Consola y revela la ejecución (enlace del resumen). */
  abrirConsolaEn: (id: string) => void;
  /** [209A-1 F3] Auto-apertura por `consola_inicio` del agente (misma
   * máquina de supresión que F1: 'suprimida' = el usuario la cerró a mitad
   * de turno y solo se acumula hasta el próximo turno). */
  abrirConsolaPorAgente: (id: string) => 'abierta' | 'ya' | 'suprimida';
  /** [209A-1 F3] Streaming de consola hacia el store (hook del adaptador).
   * Devuelve lo que decidió la auto-apertura (para el aviso visible). */
  onConsolaEvento: (ev: EventoConsola) => 'abierta' | 'ya' | 'suprimida';
  /** [209A-1 F3] Reinicia la supresión al empezar un turno (igual que F1). */
  notificarTurnoInicioConsola: () => void;
  reposicionarWebview: () => void;
}

export interface PanelDerechoTodo
  extends PanelDerechoPiezas, PanelDerechoVisibilidad, PanelDerechoAperturas {}
