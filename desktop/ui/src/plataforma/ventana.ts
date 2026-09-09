// Adaptador de ventana (boundary `window` de Sentinel): único dueño de los
// accesos directos a `window.*` en la UI. Los componentes y el orquestador
// usan estas funciones (viewport, URL/entorno, suscripciones de ciclo de
// vida, seguimiento del puntero) en vez de tocar `window` a mano.

/** Valor de un parámetro de la URL (`?nombre=`); `null` si no existe. */
export function parametroUrl(nombre: string): string | null {
  return new URLSearchParams(window.location.search).get(nombre);
}

/** Protocolo del documento (`http:`, `https:`, `tauri:`, …). */
export function protocolo(): string {
  return window.location.protocol;
}

/** Lee una clave de localStorage; `null` si no existe o no hay acceso. */
export function leerMemoriaLocal(clave: string): string | null {
  try {
    return window.localStorage.getItem(clave);
  } catch {
    return null;
  }
}

/** Ancho interior de la ventana (viewport CSS px). */
export function anchoVentana(): number {
  return window.innerWidth;
}

/** Alto interior de la ventana (viewport CSS px). */
export function altoVentana(): number {
  return window.innerHeight;
}

/** Suscribe `fn` al redimensionado de la ventana (listener permanente). */
export function alRedimensionar(fn: () => void): void {
  window.addEventListener('resize', fn);
}

/** Retira una suscripción de `alRedimensionar`. */
export function finRedimension(fn: () => void): void {
  window.removeEventListener('resize', fn);
}

/** Suscribe `fn` a la pérdida de foco de la ventana. */
export function alDesenfocar(fn: () => void): void {
  window.addEventListener('blur', fn);
}

/** Retira una suscripción de `alDesenfocar`. */
export function finDesenfocar(fn: () => void): void {
  window.removeEventListener('blur', fn);
}

/** Suscribe `fn` a cualquier scroll en captura (cierra flotantes). */
export function alDesplazar(fn: () => void): void {
  window.addEventListener('scroll', fn, true);
}

/** Retira una suscripción de `alDesplazar`. */
export function finDesplazar(fn: () => void): void {
  window.removeEventListener('scroll', fn, true);
}

/**
 * Sigue el puntero a nivel de ventana hasta soltar (arrastre de grips).
 * Registra `mousemove`+`mouseup` permanentes; devuelve la limpieza por si
 * el dueño la necesita al desmontar.
 */
export function seguirPuntero(
  alMover: (e: MouseEvent) => void,
  alSoltar: () => void,
): () => void {
  window.addEventListener('mousemove', alMover);
  window.addEventListener('mouseup', alSoltar);
  return () => {
    window.removeEventListener('mousemove', alMover);
    window.removeEventListener('mouseup', alSoltar);
  };
}
