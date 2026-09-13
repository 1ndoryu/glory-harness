/* [139A-7] Single-flight con trailing para recargas del panel Cambios: una
 * sola ejecución en vuelo; las peticiones que llegan durante el vuelo
 * colapsan en UNA repetición posterior (el modo `total` arrastra a `vault`,
 * nunca al revés). Evita el re-análisis N× ante ráfagas (fin de turno +
 * escritura viva + abrir la tab en la misma ventana). */
export type ModoVuelo = 'total' | 'vault';

export interface VueloUnico {
  solicitar(modo: ModoVuelo): void;
}

export function crearVueloUnico(ejecutar: (modo: ModoVuelo) => Promise<void>): VueloUnico {
  let enVuelo = false;
  let pendiente: ModoVuelo | null = null;
  function despachar(modo: ModoVuelo): void {
    if (enVuelo) {
      if (pendiente !== 'total') pendiente = modo;
      return;
    }
    enVuelo = true;
    void ejecutar(modo).finally(() => {
      enVuelo = false;
      const siguiente = pendiente;
      pendiente = null;
      if (siguiente !== null) despachar(siguiente);
    });
  }
  return { solicitar: despachar };
}
