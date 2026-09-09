/* Tipos del panel de chat (re-exportados por `panelChat.ts` para no romper a
 * sus importadores del orquestador). Solo tipos: sin ciclo de runtime. */
import type {
  Conversacion,
  ElementoSeleccionado,
  ModeloSeleccionado,
  ProveedorModelo,
  Workspace,
} from '../dominio/tipos';
import type { EstadoContexto, ModoEjecucion } from './entrada';
import type { PanelMeta } from './panelMeta';
import type { Sidebar } from './sidebar';
import type { crearSimulacion } from '../simulacion/simulacion';
import type { AdaptadorReal } from '../tauri/real';

/** 'principal' | 'lateral' — coincide con el panel_id del backend. */
export type TipoPanel = 'principal' | 'lateral';

/**
 * Dependencias compartidas del runtime M1 que el orquestador posee y
 * entrega a cada panel. Lo que NO es por-panel vive aquí.
 */
export interface DepsPanel {
  adaptador: AdaptadorReal;
  simulacion: ReturnType<typeof crearSimulacion>;
  usaReal: boolean;
  usaMock: boolean;
  /** PanelMeta ÚNICO (global M1). El orquestador lo monta en el principal. */
  panelMeta: PanelMeta;
  /** Sidebar (dueña de la lista). El panel la usa desde el ⋯ de su cabecera. */
  sidebar: Sidebar;
  /** Lista actual de conversaciones (para el título/archivada del ⋯). */
  conversaciones(): Conversacion[];
  /** Estado compartido M1 (fuente: panel principal). */
  getModelo(): ModeloSeleccionado;
  getModo(): ModoEjecucion;
  getRazonamiento(): string;
  /** Workspaces disponibles y destino actual de las conversaciones nuevas. */
  getWorkspaces(): Workspace[];
  getWorkspaceSeleccionadoId(): string | null;
  /** Activa el destino seleccionado en la sesión antes de crear la conversación. */
  onWorkspaceCambiado(id: string | null): void;
  /** Prepara el workspace destino justo antes de enviar (create-on-write). */
  prepararWorkspaceSeleccionado(): Promise<void>;
  /** true si CUALQUIER panel tiene turno en curso (M1: 1 a la vez). */
  hayTurnoGlobal(): boolean;
  /** El panel que lanza avisa → el orquestador pone TODOS en 'corriendo'. */
  notificarTurnoInicio(): void;
  /** Al terminar → el orquestador pone todos en reposo y refresca la lista. */
  notificarTurnoFin(): void;
  /** Registra qué panel envió el último mensaje (para reanudar del panelMeta). */
  registrarUltimoEnvio(panel: PanelChat): void;
  /** Refresca la lista desde el backend (tras renombrar/auto-nombrar). */
  resincronizarSidebar(): Promise<void>;
  /** El panel cambió de conversación → la sidebar marca el panel enfocado. */
  onConversacionCambio(id: string | null): void;
}

/** Un chat duplicable montado por `montarPanelChat`. */
export interface PanelChat {
  raiz: HTMLElement;
  /** Nodo `.mensajes` de este panel (para pintar bloques del mock). */
  mensajes: HTMLElement;
  tipo: TipoPanel;
  idPrefijo: string;
  /** Conversación que este panel muestra (null = sin conversación). */
  conversaId: string | null;
  getCorriendo(): boolean;
  /** Marca el panel como ENFOCADO (el orquestador pinta la sidebar). */
  activar(): void;
  /** Marca el panel como NO enfocado (lo usa el orquestador al activar otro). */
  desenfocar(): void;
  /** Carga una conversación en ESTE panel (backend real o mock). */
  cargarConversacion(id: string): Promise<void>;
  /** Crea una conversación nueva en ESTE panel (solo real). */
  nuevaConversacion(): Promise<void>;
  /** Reenvía el último mensaje de ESTE panel (play del panelMeta global). */
  reanudarUltimo(): void;
  /** El orquestador marca el estado de turno (M1: afecta a todos). */
  setCorriendoGlobal(corriendo: boolean): void;
  /** Inicio (ms) del turno en curso de este panel (o null). */
  getInicioTurno(): number | null;
  /** Enfoca el textarea de este panel. */
  enfocarEntrada(): void;
  /** Pone el título de la cabecera de este panel. */
  ponerTitulo(texto: string): void;
  /** Vacía los mensajes y cancela una edición pendiente. */
  limpiar(): void;
  /** [069A-7] Pone el panel en BORRADOR local: `conversaId=null`, chat vacío
   * y título "Nueva conversación", SIN tocar el backend (create-on-write: la
   * fila solo se crea al enviar el primer mensaje). */
  ponerBorrador(): void;
  /** Recalcula la altura del textarea (llamar tras montar al DOM). */
  medir(): void;
  /** Entra en modo renombrar inline en la cabecera de ESTE panel. */
  empezarRenombrarCabecera(
    valor: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void;
  /** [039A-3 P6b] Sincroniza los controles de la entrada de ESTE panel. Todos
   * los paneles usan la variante completa y comparten el estado M1 del
   * orquestador (modelo/modo/razonamiento), así que los tres setter existen
   * en principal y lateral. */
  setModelo(modelo: ModeloSeleccionado): void;
  setModo(modo: ModoEjecucion): void;
  setRazonamiento(valor: string): void;
  /** [039A-3 P4/P6b] Refleja lista visible/oculta en el botón de la cabecera
   * (solo el principal lo tiene). */
  setSidebarAbierta(abierta: boolean): void;
  /** [089A-2] Refleja panel derecho visible/oculto en el botón de la
   * cabecera (solo el principal lo tiene). */
  setPanelDerechoAbierto(abierto: boolean): void;
  /** Repinta un aviso en este panel (mock/próximamente, sin backend). */
  avisoLocal(texto: string, meta: string, detalle: string): void;
  /** [039A-3 P6] Actualiza el indicador circular de contexto de la entrada. */
  setContexto(estado: EstadoContexto): void;
  /** Actualiza el selector de área de trabajo de la conversación nueva. */
  setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null): void;
  /** Oculta/muestra el selector de workspace según si la conversación es nueva. */
  setConversaId(id: string | null): void;
  /** [seleccionar] Muestra un elemento del navegador como badge pendiente en
   * la entrada de ESTE panel (se antepone al próximo mensaje enviado). */
  adjuntarElemento(elem: ElementoSeleccionado): void;
  /** Cambios de archivos de la conversación cargada para sincronizar Files. */
  listarCambios(): CambioArchivoPanel[];
}

/** Cambio de archivo de la conversación (ruta + diff ya formateado). */
export interface CambioArchivoPanel {
  origen: 'tool';
  tool: 'file_write' | 'file_patch';
  ruta: string;
  titulo: string;
  resumen: string;
  diff: string | null;
}

/** Identidad del panel y dependencias de dominio. */
export interface PanelChatBase {
  tipo: TipoPanel;
  /** Prefijo de ids DOM (coincide con `tipo`; p. ej. 'principal'). */
  idPrefijo: string;
  /** Catálogo de proveedores de la entrada completa (principal y lateral). */
  proveedores?: ProveedorModelo[];
  deps: DepsPanel;
}

/** Botones de la cabecera de ESTE panel. */
export interface PanelChatCabecera {
  /** Se invoca al pulsar el botón ⋯ de la cabecera de ESTE panel. */
  onAcciones(rect: DOMRect): void;
  /** Panel lateral: se invoca al pulsar el × de cierre. */
  onCerrar?: () => void;
  /** Panel principal: se invoca al pulsar el botón de MOSTRAR la lista
   * (solo aparece si la lista está oculta por ancho; no hay ocultación manual). */
  onToggleSidebar?: () => void;
  /** [089A-2] Panel principal: muestra/oculta el panel derecho. */
  onTogglePanelDerecho?: () => void;
}

/** Cambios de modelo/modo/razonamiento/área hechos en la barra de ESTE panel
 * (todos los paneles son completos; el orquestador propaga M1 al resto). */
export interface PanelChatEstadoVista {
  onModeloCambiado?: (modelo: ModeloSeleccionado) => void;
  onModoCambiado?: (modo: ModoEjecucion) => void;
  onRazonamientoCambiado?: (razonamiento: string) => void;
  /** El usuario eligió un área de trabajo distinta para la conversación nueva. */
  onWorkspaceCambiado?: (workspaceId: string | null) => void;
}

export interface PanelChatOpciones
  extends PanelChatBase, PanelChatCabecera, PanelChatEstadoVista {}
