/* Entorno de ejecución (extraído de main.ts [089A-16 F1b]): detecta
 * dónde corre la UI (Tauri IPC / web HTTP-SSE / sin backend) y aporta
 * constantes puras. Sin dependencias del orquestador. */
import { leerMemoriaLocal, parametroUrl, protocolo } from '../plataforma/ventana';

/** [129A-12 F1] Clave vigente del tema. Sustituye a `temaOscuro` (boolean),
 * que sigue leyéndose solo para migrar la preferencia guardada. */
export const CLAVE_TEMA = 'tema';
/** Clave histórica (boolean true/false) — solo migración. */
export const CLAVE_TEMA_LEGACY = 'temaOscuro';

/** Tema visual de la app. `claro`/`oscuro` son los monocromos de siempre;
 * `synara` es la variante oscura del seed de Synara (acentos y diffs en
 * color). Solo `synara` rompe el monocromo estricto, por decisión del usuario
 * (12-09) y sin tocar las otras dos. */
export type Tema = 'claro' | 'oscuro' | 'synara';

export const TEMAS: readonly Tema[] = ['claro', 'oscuro', 'synara'];

/** Normaliza cualquier valor persistido a un tema válido; `null` si no hay
 * valor reconocible. Acepta los formatos históricos de `temaOscuro`. */
export function normalizarTema(valor: unknown): Tema | null {
  if (valor === true || valor === 'true' || valor === '1') return 'oscuro';
  if (valor === false || valor === 'false' || valor === '0') return 'claro';
  if (valor === 'claro' || valor === 'oscuro' || valor === 'synara') return valor;
  return null;
}

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
  const q = parametroUrl('api');
  if (q) return q.replace(/\/$/, '');
  try {
    const g = leerMemoriaLocal('gh_api');
    if (g) return g.replace(/\/$/, '');
  } catch {
    /* sin localStorage */
  }
  // UI servida por `glory-harness web`: el backend existe trivialmente.
  const p = protocolo();
  if (p === 'http:' || p === 'https:') return '';
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

/** Refleja el tema en `data-tema` (fuente única para el CSS). `claro`
 * conserva la base `:root`; ninguna regla apunta a ese valor. */
export function aplicarTema(tema: Tema): void {
  document.documentElement.dataset.tema = tema;
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
