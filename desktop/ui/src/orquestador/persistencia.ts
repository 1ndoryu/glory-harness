/* Persistencia de preferencias de layout (extraído de main.ts
 * [089A-16 F1b]): backend real → configGuardar; si no, localStorage.
 * Las dependencias se inyectan por parámetro (sin estado de módulo). */

export interface PersistenciaDeps {
  usaReal: boolean;
  usaTauri: boolean;
  guardarConfig: (clave: string, valor: string) => Promise<unknown>;
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
  // El modo web usa localStorage para estas claves; Tauri las restaura por IPC
  // cuando termina de abrir la sesión persistida en SQLite.
  if (deps.usaTauri) return null;
  try {
    return window.localStorage.getItem(clave);
  } catch {
    return null;
  }
}
