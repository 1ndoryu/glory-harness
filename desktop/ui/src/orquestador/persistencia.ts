/* Persistencia de preferencias de layout (extraído de main.ts
 * [089A-16 F1b]): backend real → configGuardar; si no, localStorage.
 * Las dependencias se inyectan por parámetro (sin estado de módulo). */

export const CLAVE_LATERAL_ANCHO = 'lateral_ancho';
export const CLAVE_PANEL_DERECHO = 'panel_derecho_estado';

export interface PersistenciaDeps {
  usaReal: boolean;
  usaTauri: boolean;
  guardarConfig: (clave: string, valor: string) => Promise<unknown>;
  leerConfig: (clave: string) => Promise<string | null>;
  avisar: (texto: string) => void;
}

export function guardarSidebar(deps: PersistenciaDeps, clave: string, valor: string): void {
  if (deps.usaReal) {
    void deps
      .guardarConfig(clave, valor)
      .catch((e: unknown) => deps.avisar(`no se pudo guardar ${clave}: ${String(e)}`));
  } else {
    try {
      window.localStorage.setItem(clave, valor);
    } catch {
      /* sin persistencia local */
    }
  }
}

export function leerSidebar(deps: PersistenciaDeps, clave: string): string | null {
  // El modo web puede restaurar síncronamente; Tauri se lee tras abrir la sesión.
  if (deps.usaTauri) return null;
  try {
    return window.localStorage.getItem(clave);
  } catch {
    return null;
  }
}

/** Lee una preferencia en ambos entornos sin duplicar la política de acceso. */
export async function leerPreferencia(
  deps: PersistenciaDeps,
  clave: string,
): Promise<string | null> {
  if (deps.usaReal) return deps.leerConfig(clave);
  return leerSidebar(deps, clave);
}
