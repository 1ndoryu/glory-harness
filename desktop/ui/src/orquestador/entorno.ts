/* Entorno de ejecución (extraído de main.ts [089A-16 F1b]): detecta
 * dónde corre la UI (Tauri IPC / web HTTP-SSE / sin backend) y aporta
 * constantes puras. Sin dependencias del orquestador. */

export const CLAVE_TEMA_OSCURO = 'temaOscuro';

export const RAZONAMIENTO_ETIQUETA: Record<string, string> = {
  low: 'Bajo',
  medium: 'Medio',
  high: 'Alto',
};

/** Modo de backend resuelto una vez en el arranque. */
export interface Entorno {
  usaTauri: boolean;
  baseApi: string | null;
  usaReal: boolean;
  usaMock: boolean;
  modoTexto: string;
}

/** [069A-2 F4] Factoría simple: Tauri → IPC; `?api=`/`gh_api`/origen http
 * → adaptador HTTP/SSE; sin backend → mock o maqueta con aviso claro.
 * `?token=` aporta el maestro (solo memoria); la sesión viaja en cookie. */
export function detectarBaseApi(esTauri: boolean): string | null {
  if (esTauri) return null;
  const q = new URLSearchParams(window.location.search).get('api');
  if (q) return q.replace(/\/$/, '');
  try {
    const g = window.localStorage.getItem('gh_api');
    if (g) return g.replace(/\/$/, '');
  } catch {
    /* sin localStorage */
  }
  // UI servida por `glory-harness web`: el backend existe trivialmente.
  if (window.location.protocol === 'http:' || window.location.protocol === 'https:') return '';
  return null;
}

export function resolverEntorno(esTauri: boolean): Entorno {
  const baseApi = detectarBaseApi(esTauri);
  const usaReal = esTauri || baseApi !== null;
  return {
    usaTauri: esTauri,
    baseApi,
    usaReal,
    usaMock: !usaReal && import.meta.env.VITE_MOCK === '1',
    modoTexto: esTauri
      ? 'tauri in-process'
      : baseApi !== null
        ? `web (${baseApi || 'mismo origen'})`
        : 'sin backend',
  };
}

export function aplicarTemaOscuro(activo: boolean): void {
  if (activo) document.documentElement.dataset.tema = 'oscuro';
  else delete document.documentElement.dataset.tema;
}

/** Opciones de turno para `asegurarSesion` en el arranque real.
 * Campos vacíos significan "resolver desde la configuración persistida";
 * enviar los defaults aquí sobrescribiría la configuración guardada. */
export function opcionesArranque(): {
  proveedor: string;
  modelo: string;
  modo: string;
  razonamiento: string;
} {
  return {
    proveedor: '',
    modelo: '',
    modo: '',
    razonamiento: '',
  };
}
