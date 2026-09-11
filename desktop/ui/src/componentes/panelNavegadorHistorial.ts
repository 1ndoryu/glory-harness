/* [109A-11] Historial propio del panel Navegador en modo web.
 *
 * El panel pilota un `<iframe>` que casi siempre apunta a otro origen
 * (`https://example.com` de arranque). En ese caso el padre NO puede leer ni
 * tocar `contentWindow.history`: `back()`/`forward()` lanzan `SecurityError` y
 * `location` no es legible. La única forma honesta de ofrecer Atrás/Adelante es
 * llevar la pila de URLs que el propio panel ha navegado y volver asignando
 * `iframe.src`.
 *
 * En la app de escritorio esto no se usa: ahí el historial lo lleva el WebView2
 * nativo y se pilota por CDP (`Runtime.evaluate` con `history.back()`).
 *
 * Limitación declarada: una navegación disparada DENTRO del iframe (un clic del
 * usuario en un enlace) no se puede observar si es de otro origen, así que no
 * entra en la pila. Es una consecuencia del aislamiento del navegador, no un
 * fallo silencioso: los botones siguen funcionando sobre lo que el panel
 * navegó. */

/** Tope de la pila. El panel no es un navegador completo: encadenar
 * navegaciones no debe hacer crecer la memoria sin límite. */
const MAX_ENTRADAS = 50;

/** Historial de URLs del panel en modo web. */
export interface HistorialNavegador {
  /** Registra la URL que se abandona antes de navegar a otra. */
  registrar(url: string): void;
  /** Devuelve la URL anterior (o `null` si no hay) y mueve la actual a la pila
   * de adelante. */
  atras(actual: string): string | null;
  /** Devuelve la URL siguiente (o `null` si no hay) y mueve la actual a la pila
   * de atrás. */
  adelante(actual: string): string | null;
}

export function crearHistorialNavegador(): HistorialNavegador {
  const atras: string[] = [];
  const adelante: string[] = [];

  function empujar(pila: string[], url: string): void {
    if (!url) return;
    pila.push(url);
    if (pila.length > MAX_ENTRADAS) pila.shift();
  }

  return {
    registrar(url) {
      // Navegar invalida el "adelante" (igual que en un navegador real).
      adelante.length = 0;
      // No repetir la misma URL en dos entradas seguidas (Ir dos veces o
      // volver a la dirección actual dejarían un "atrás" que no hace nada).
      if (atras[atras.length - 1] === url) return;
      empujar(atras, url);
    },
    atras(actual) {
      const anterior = atras.pop();
      if (anterior === undefined) return null;
      empujar(adelante, actual);
      return anterior;
    },
    adelante(actual) {
      const siguiente = adelante.pop();
      if (siguiente === undefined) return null;
      empujar(atras, actual);
      return siguiente;
    },
  };
}
