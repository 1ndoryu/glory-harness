/* Tipos del compositor de entrada (re-exportados por `entrada.ts` para no
 * romper a sus importadores: `panelChat`, `modal`, `entradaContexto`). */
import type {
  ElementoSeleccionado,
  ModeloSeleccionado,
  ProveedorModelo,
  Workspace,
} from '../dominio/tipos';
import type { ComandoProyecto } from '../dominio/comandosSlash';

/* [109A-4 F4] El modo `meta` se retiró del segmentado: ahora es el comando
 * `/meta <texto>`, que fuerza la política solo-lectura de UN turno sin cambiar
 * el modo de la sesión. `permiso_por_modo("meta")` sigue siendo la política
 * interna de ese turno en el core. */
export type ModoEjecucion = 'predeterminado' | 'autonomo';

/** [039A-3 P5] Variante 'completa' (controles: modelo/razonamiento/modo) o
 * 'minima' (solo textarea + enviar/detener; hereda el runtime M1). */
export type VarianteEntrada = 'completa' | 'minima';

/** Modos reales del core (permiso.rs) con su etiqueta visible. */
export const MODOS_EJECUCION: Array<{ valor: ModoEjecucion; etiqueta: string }> = [
  { valor: 'predeterminado', etiqueta: 'Predeterminado' },
  { valor: 'autonomo', etiqueta: 'Autónomo' },
];

export const ETIQUETA_MODO: Record<ModoEjecucion, string> = {
  predeterminado: 'Predeterminado',
  autonomo: 'Autónomo',
};

/** [039A-1 04-09 H7] Niveles de razonamiento (nivel_razonamiento del core). */
export const VALORES_RAZONAMIENTO: Array<{ valor: string; etiqueta: string }> = [
  { valor: 'low', etiqueta: 'Bajo' },
  { valor: 'medium', etiqueta: 'Medio' },
  { valor: 'high', etiqueta: 'Alto' },
];

/** Etiqueta visible de un nivel ('low' → 'Bajo'). */
export const ETIQUETA_RAZONAMIENTO: Record<string, string> = {
  low: 'Bajo',
  medium: 'Medio',
  high: 'Alto',
};

/** [039A-3 P6+] Estado de contexto del indicador circular y su menú hover.
 * Campos en `null` = sin dato (la UI los omite o muestra "sin datos"). */
export interface EstadoContexto {
  /** Ocupación 0-100 de la ventana efectiva (o `null` sin dato). */
  pct: number | null;
  /** Ventana máxima configurada (p. ej. 150000). */
  maxVentana: number | null;
  /** Reserva de salida que se descuenta de la ventana para el cálculo. */
  reservaSalida: number | null;
  /** Tokens totales de entrada del último desglose de contexto. */
  totalEntrada: number | null;
}

export interface Entrada {
  raiz: HTMLElement;
  /** Recalcula la altura del textarea (llamar tras montar al DOM). */
  medir(): void;
  /** Estado del botón enviar/detener. */
  setCorriendo(corriendo: boolean): void;
  /** Nombre del modelo mostrado. No-op en la variante mínima. */
  setModeloNombre(nombre: string): void;
  /** Modelo mostrado (reemplaza proveedor/modelo/nombre). No-op mínima. */
  setModelo(modelo: ModeloSeleccionado): void;
  /** [039A-1 04-09 H7] Nivel de razonamiento activo ('low'|'medium'|'high').
   * No-op en la variante mínima (lo fija el panel principal). */
  setRazonamientoValor(valor: string): void;
  /** Nivel de razonamiento activo. */
  getRazonamiento(): string;
  /** Etiqueta del modo. No-op en la variante mínima (lo fija el principal). */
  setModo(modo: ModoEjecucion): void;
  /** Pide foco al textarea (tras enviar). */
  enfocar(): void;
  /** Permite a la app conocer el estado interno del textarea. */
  getCorriendo(): boolean;
  getModo(): ModoEjecucion;
  /** [039A-3 P2] Pone el textarea en modo edición de un mensaje de usuario:
   * rellena su texto y muestra la barra "editando…" con cancelar. Al enviar
   * en este modo, `main.ts` hace rewind(`editar=true`) + reenvío. */
  ponerEnEdicion(id: string, texto: string): void;
  /** [039A-3 P2] Sale del modo edición (limpia barra) sin tocar el texto. */
  cancelarEnEdicion(): void;
  /** [039A-3 P2] `true` si hay una edición pendiente (mensaje objetivo). */
  enEdicion(): boolean;
  /** [039A-3 P2] Id del mensaje en edición (`null` si no). */
  edicionId(): string | null;
  /** [039A-3 P2] Texto actual del textarea (sin recortar). */
  getTexto(): string;
  /** [039A-3 P6] Actualiza el indicador circular de contexto y su detalle de
   * hover. Con `estado.pct` se pinta el arco (0-100); `null` deja el círculo
   * vacío. El resto de campos alimenta el pequeño menú `.ctx-detalle` que
   * aparece al poner el cursor sobre el círculo (uso de la ventana). */
  setContexto(estado: EstadoContexto): void;
  /** [109A-4] Último estado de contexto conocido (`null` si aún no hay dato):
   * lo consume el comando `/contexto`. */
  getContexto(): EstadoContexto | null;
  /** Actualiza las áreas disponibles sin reconstruir el composer. */
  setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null): void;
  /** Oculta/muestra el selector de workspace (conversación nueva vs existente). */
  setConversaId(id: string | null): void;
  /** [seleccionar] Adjunta un elemento del navegador como badge pendiente:
   * se muestra sobre el textarea y se antepone al próximo mensaje enviado. */
  adjuntarElemento(elem: ElementoSeleccionado): void;
  /** [seleccionar] Badge pendiente actual (`null` si no hay). */
  getElementoPendiente(): ElementoSeleccionado | null;
  /** [109A-4] Reemplaza los comandos `/` del área activa (menú del
   * compositor). Los integrados no se pasan aquí: son catálogo fijo. */
  setComandosProyecto(comandos: ComandoProyecto[]): void;
}

/** Identidad y variante de la entrada (núcleo mínimo que todo consumidor usa). */
export interface EntradaBase {
  /** Prefijo de los ids internos (una instancia por panel). */
  idPrefijo: string;
  /** [039A-3 P5] 'completa' (default) o 'minima'. */
  variante?: VarianteEntrada;
  modo: ModoEjecucion;
}

/** Catálogo y selección de modelo (solo variante 'completa'). */
export interface EntradaModelo {
  /** Solo variante 'completa': catálogo de proveedores del selector. */
  proveedores?: ProveedorModelo[];
  /** Modelo inicial (obligatorio en 'completa'; se ignora en 'minima'). */
  modeloActual?: ModeloSeleccionado;
  /** Se invoca al elegir un modelo del menú (solo variante 'completa'). */
  onModeloCambiado?: (modelo: ModeloSeleccionado) => void;
}

/** Envío, detención y cambios de modo/razonamiento. */
export interface EntradaEnvio {
  /** Se invoca al enviar un mensaje.
   *  [039A-3 P2] `editandoId` trae el id del mensaje de usuario reescrito
   *  (edición), o `null` para un mensaje nuevo. El consumidor decide si
   *  ejecutar rewind(editar=true) antes de enviar el texto. */
  onEnviar: (texto: string, editandoId?: string | null) => void;
  /** Se invoca al pulsar detener durante un turno. */
  onDetener: () => void;
  /** Se invoca al cambiar el modo de ejecución desde la barra (completa). */
  onModoCambiado?: (modo: ModoEjecucion) => void;
  /** [039A-1 04-09 H7] Nivel de razonamiento inicial ('low'|'medium'|'high'). */
  razonamiento?: string;
  /** [039A-1 04-09 H7] Se invoca al elegir un nivel de razonamiento (completa). */
  onRazonamientoCambiado?: (razonamiento: string) => void;
}

/** Área destino de la conversación nueva. */
export interface EntradaArea {
  /** Áreas disponibles para la conversación nueva. */
  workspaces?: Workspace[];
  /** Área destino seleccionada; `null` = conversación sin proyecto. */
  workspaceSeleccionadoId?: string | null;
  /** Se invoca al cambiar el área destino de la conversación nueva. */
  onWorkspaceCambiado?: (workspaceId: string | null) => void;
}

/** [109A-4] Comandos `/` propios del área activa (`.glory/comandos`). */
export interface EntradaComandosOpc {
  /** Catálogo inicial de comandos del área (se puede refrescar después). */
  comandosProyecto?: ComandoProyecto[];
}

export interface EntradaOpciones
  extends EntradaBase, EntradaModelo, EntradaEnvio, EntradaArea, EntradaComandosOpc {}
