// Contratos del panel Files: transporte al backend, cambios del agente y
// opciones de montaje. Extraído de `panelFiles.ts` para no rebasar el techo
// de líneas; sin cambios de contrato.

import type { ListadoWorkspace, ResultadoBusqueda } from '../dominio/tipos';

export interface FilesTransport {
  listar(ruta: string, profundidad?: number): Promise<ListadoWorkspace>;
  buscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda>;
  leer(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }>;
  abrirCon(ruta: string): Promise<void>;
}

export interface CambioArchivoFiles {
  origen: 'tool';
  tool: 'file_write' | 'file_patch';
  ruta: string;
  titulo: string;
  resumen: string;
  diff: string | null;
}

export interface PanelFiles {
  raiz: HTMLElement;
  recargar(): void;
  registrarCambio(cambio: CambioArchivoFiles): void;
  sincronizarCambios(cambios: CambioArchivoFiles[]): void;
  /** [129A-10 F2] Previsualiza una ruta cualquiera (vista del agente, no
   * cambio: no toca el mapa de cambios). */
  mostrarArchivo(ruta: string): void;
}

export interface OpcionesPanelFiles {
  transporte: FilesTransport;
  onError?: (texto: string, detalle?: string) => void;
}
