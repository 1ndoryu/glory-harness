/* [089A-5] Historial de navegación de la app para Atrás/Adelante.
 *
 * Misma lógica que Synara (`synara/.../appNavigation.ts`): los botones gobiernan
 * la navegación de la APP (qué conversación + qué área de trabajo se visita),
 * no el historial interno de un panel (el Navegador ya tiene el suyo propio en
 * `panelNavegadorHistorial.ts`, 109A-11).
 *
 * Modelo de dos pilas como el precedente del Navegador: `registrar` guarda el
 * estado que se abandona, `atras`/`adelante` lo intercambian con el actual y
 * navegar de nuevo invalida el "adelante" (semántica de navegador real).
 * `puedeAtras`/`puedeAdelante` permiten habilitar los botones sin mutar (Synara
 * deriva `canGoBack`/`canGoForward` del estado del history).
 *
 * Regla del vacío inicial: el estado previo a la primera navegación no se
 * registra como destino de "atrás" (si no, Atrás quedaría habilitado apuntando a
 * la nada nada más abrir la app). */

/** Tope de cada pila. La app no es un navegador completo: encadenar visitas no
 * debe hacer crecer la memoria sin límite (mismo tope que el Navegador). */
const MAX_ENTRADAS = 50;

/** Una visita de la app: conversación visible + área activa (`null` = borrador
 * de conversación nueva / sin área en modo mock). */
export interface EntradaHistorialApp {
  conversaId: string | null;
  proyectoRuta: string | null;
}

/** Historial de navegación de la app (atrás/adelante de la barra superior). */
export interface HistorialApp {
  /** Registra el estado que se abandona antes de navegar a otro. */
  registrar(entrada: EntradaHistorialApp): void;
  /** Devuelve la visita anterior (o `null` si no hay) y mueve la actual a la
   * pila de adelante. */
  atras(actual: EntradaHistorialApp): EntradaHistorialApp | null;
  /** Devuelve la visita siguiente (o `null` si no hay) y mueve la actual a la
   * pila de atrás. */
  adelante(actual: EntradaHistorialApp): EntradaHistorialApp | null;
  /** Hay visita anterior sin mover nada (para habilitar el botón). */
  puedeAtras(): boolean;
  /** Hay visita siguiente sin mover nada (para habilitar el botón). */
  puedeAdelante(): boolean;
}

function mismaEntrada(a: EntradaHistorialApp, b: EntradaHistorialApp): boolean {
  return a.conversaId === b.conversaId && a.proyectoRuta === b.proyectoRuta;
}

export function crearHistorialApp(): HistorialApp {
  const atras: EntradaHistorialApp[] = [];
  const adelante: EntradaHistorialApp[] = [];

  function empujar(pila: EntradaHistorialApp[], entrada: EntradaHistorialApp): void {
    pila.push({ ...entrada });
    if (pila.length > MAX_ENTRADAS) pila.shift();
  }

  return {
    registrar(entrada) {
      // Navegar invalida el "adelante" (igual que en un navegador real).
      adelante.length = 0;
      // Regla del vacío inicial: no fabricar un destino de "atrás" desde el
      // estado previo a la primera navegación (ver cabecera del módulo).
      if (atras.length === 0 && entrada.conversaId === null) return;
      // No repetir la misma visita en dos entradas seguidas (seleccionar dos
      // veces la conversación actual dejaría un "atrás" que no hace nada).
      if (atras.length > 0 && mismaEntrada(atras[atras.length - 1], entrada)) return;
      empujar(atras, entrada);
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
    puedeAtras() {
      return atras.length > 0;
    },
    puedeAdelante() {
      return adelante.length > 0;
    },
  };
}
