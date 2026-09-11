/* Formato de duraciones para el reloj del panel meta y el badge de logro
 * ([109A-5 F3]). Un solo formateador evita que el pie y el panel muestren
 * tiempos distintos del mismo logro. */

/**
 * Formatea segundos como `MM:SS`. Los minutos no se truncan a dos dígitos:
 * una meta de dos horas se muestra `120:00`, no `00:00` como si hubiera
 * vuelto a empezar. Entradas negativas o no finitas caen a `00:00` en vez de
 * propagar `NaN` al DOM.
 */
export function formatearSegundos(segundos: number): string {
  if (!Number.isFinite(segundos) || segundos <= 0) return '00:00';
  const s = Math.floor(segundos);
  const minutos = Math.floor(s / 60);
  return `${String(minutos).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

/** Igual que `formatearSegundos` pero desde milisegundos. */
export function formatearDuracion(ms: number): string {
  return formatearSegundos(ms / 1000);
}
