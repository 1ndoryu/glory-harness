// Ayudantes puros del panel Files: normalización de rutas y texto de error
// (ver `panelFiles.ts`). Sin DOM ni estado.

import type { ErrorFilesystem } from '../dominio/tipos';

/* Compara rutas para localizar filas: separadores unificados y sin
 * importar mayúsculas (el agente puede escribir `test/x` y el workspace
 * tener `Test/`; en Windows son el mismo archivo). Para listar/leer se
 * usa siempre la ruta canónica de la fila (`dataset.ruta`), no esta. */
export function normalizarRutaFiles(ruta: string): string {
  return ruta.replace(/\\/g, '/').toLowerCase();
}

export function errorTextoFiles(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'mensaje' in error) {
    return String((error as ErrorFilesystem).mensaje);
  }
  return String(error);
}
