// ============================================================
// Utilidades DOM mínimas (sin framework): crear elementos con
// clase, vaciar, desplazar scroll al final. Port del helper el()
// del mockup a TS seguro.
// ============================================================

/** Crea un elemento. `cls` opcional, `html` opcional (innerHTML). */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  html?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
  if (html !== undefined) e.innerHTML = html;
  return e;
}

/** Busca un hijo por selector bajo un padre, lanzando si falta. */
export function qs<T extends Element>(padre: ParentNode, sel: string): T {
  const n = padre.querySelector<T>(sel);
  if (!n) throw new Error('falta nodo: ' + sel);
  return n;
}

/** Vacía un contenedor (elimina todos los hijos). */
export function vaciar(nodo: HTMLElement): void {
  while (nodo.firstChild) nodo.removeChild(nodo.firstChild);
}

/** Lleva el scroll de un contenedor al final. */
export function scrollAlFinal(nodo: HTMLElement): void {
  nodo.scrollTop = nodo.scrollHeight;
}
