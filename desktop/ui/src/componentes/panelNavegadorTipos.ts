/* Tipos del panel Navegador (re-exportados por `panelNavegador.ts` para no
 * romper a sus importadores del orquestador). */
import type { ElementoSeleccionado } from '../dominio/tipos';

/** API pública que el orquestador usa para conectar el panel. */
export interface PanelNavegador {
  raiz: HTMLElement;
  /** Contenedor donde se monta la webview child (HWND). */
  contenedor: HTMLElement;
  /** Abre/cierra el panel (display). */
  mostrar(visible: boolean): void;
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
  /** [seleccionar] Se invoca cuando el usuario eligió un elemento de la página
   * (modo selección + hover + clic, solo Tauri). El orquestador lo entrega al
   * chat activo como badge pendiente. */
  onSeleccionar?: (elem: ElementoSeleccionado) => void;
  /** [109A-9] Canal de aviso visible al usuario, obligatorio: el panel no puede
   * resolver sus fallos con `console.*` (regla de «sin fallos silenciosos») y
   * no tiene UI propia de estado, así que el orquestador lo enruta al chat
   * activo (`avisoGlobal`). */
  onAviso: (texto: string, detalle?: string) => void;
}
