/* Tipos del panel Navegador (re-exportados por `panelNavegador.ts` para no
 * romper a sus importadores del orquestador). Solo tipos + constante. */
import type { ElementoSeleccionado } from '../dominio/tipos';

/** Evento de acción del agente en el navegador (para el log). */
export interface AccionNavegador {
  herramienta: string;
  descripcion: string;
  ok: boolean;
  tiempo: number;
}

/** API pública que el orquestador (main.ts) usa para conectar el panel. */
export interface PanelNavegador {
  raiz: HTMLElement;
  /** Contenedor donde se monta la webview child (HWND). */
  contenedor: HTMLElement;
  /** Abre/cierra el panel (display). */
  mostrar(visible: boolean): void;
  /** Registra una acción en el log. */
  registrarAccion(accion: AccionNavegador): void;
  /** Limpia el log de acciones. */
  limpiarLog(): void;
  /** Establece la URL actual en la barra (tras navegar). */
  fijarURL(url: string): void;
  /** [069A-2 fix] Navega a una URL: IPC en Tauri, iframe en modo web. */
  irA(url: string): void;
  /** [069A-1 F6] Muestra una captura base64 del navegador. */
  actualizarCaptura(base64: string): void;
  /** [seleccionar] Desactiva el modo selección (si está activo) y limpia el
   * JS inyectado. Lo usa el orquestador al cerrar el navegador. */
  desactivarSeleccion(): void;
}

export interface PanelNavegadorOpciones {
  /** Prefijo de ids DOM. */
  idPrefijo?: string;
  /** Ancho inicial del panel (px). Default 480. */
  ancho?: number;
  /** [069A-2 fix] Se invoca al cerrar desde el botón interno para que el
   * orquestador sincronice estado y grip. Opcional: sin él el panel se cierra
   * igualmente (y en Tauri cierra la webview él mismo). */
  onCerrar?: () => void;
  /** [seleccionar] Se invoca cuando el usuario eligió un elemento de la página
   * (modo selección + hover + clic, solo Tauri). El orquestador lo entrega al
   * chat activo como badge pendiente. */
  onSeleccionar?: (elem: ElementoSeleccionado) => void;
}

/** Tope de entradas del log de acciones del agente. */
export const MAX_LOG = 200;
