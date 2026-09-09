// Utilidades DOM mínimas (sin framework): crear elementos con
// clase, vaciar, desplazar scroll al final. Port del helper el()
// del mockup a TS seguro.

/** Crea un elemento. `cls` opcional. Sin sink HTML: el contenido se fija
 * con textContent/appendChild en cada llamada (regla innerhtml-variable). */
export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (cls) e.className = cls;
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

/** El `<body>` del documento (boundary DOM: único acceso directo a `body`). */
export function cuerpo(): HTMLElement {
  return document.body;
}

/** Busca por id en todo el documento; `null` si no existe. */
export function porId(id: string): HTMLElement | null {
  return document.getElementById(id);
}

/** Activa/desactiva una clase de estado global en `<body>`. */
export function marcarCuerpo(clase: string, activa: boolean): void {
  cuerpo().classList.toggle(clase, activa);
}

/** Lleva el scroll de un contenedor al final. */
export function scrollAlFinal(nodo: HTMLElement): void {
  nodo.scrollTop = nodo.scrollHeight;
}
