/* [129A-10 F1] El agente abre la tab del navegador al navegar, pero un
 * cierre intencional a mitad de turno se respeta (solo toast, sin reabrir)
 * hasta que empiece el siguiente turno o el usuario la reabra. Máquina pura
 * (sin DOM) para poder probarla aislada; la posee `navegadorVista`. */

export interface SupresionNavegador {
  /** Cierre manual (× de la tab o toggle): solo suprime a mitad de turno. */
  alCerrarManual(turnoEnCurso: boolean): void;
  /** Apertura manual: el usuario la quiere ver, se olvida la supresión. */
  alAbrirManual(): void;
  /** Nuevo turno: contexto nuevo, la supresión anterior ya no vale. */
  alIniciarTurno(): void;
  /** true = el auto-abrir del agente debe abstenerse (solo toast). */
  suprimido(): boolean;
}

export function crearSupresionNavegador(): SupresionNavegador {
  let suprimir = false;
  return {
    alCerrarManual(turnoEnCurso: boolean): void {
      if (turnoEnCurso) suprimir = true;
    },
    alAbrirManual(): void {
      suprimir = false;
    },
    alIniciarTurno(): void {
      suprimir = false;
    },
    suprimido(): boolean {
      return suprimir;
    },
  };
}
