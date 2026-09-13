/* [139A-2] Atribución de cambios a repos (puro, sin DOM): el vault habla en
 * rutas relativas al área y cada repo en rutas relativas a sí mismo.
 * `prefijo` = ruta del repo relativa al área (`""` = el área misma). */

export function normalizarRutaArea(ruta: string): string {
  return ruta.replaceAll('\\', '/').trim();
}

/** ¿Vive la ruta del vault (relativa al área) dentro del repo? */
export function dentroDePrefijo(ruta: string, prefijo: string): boolean {
  if (prefijo === '') return true;
  return ruta === prefijo || ruta.startsWith(`${prefijo}/`);
}

/** Traduce una ruta del área a relativa al repo (`null` si no lo contiene). */
export function relativaEnRepo(rutaArea: string, prefijo: string): string | null {
  const ruta = normalizarRutaArea(rutaArea);
  if (!dentroDePrefijo(ruta, prefijo)) return null;
  if (prefijo === '') return ruta;
  if (ruta === prefijo) return '';
  return ruta.slice(prefijo.length + 1);
}
