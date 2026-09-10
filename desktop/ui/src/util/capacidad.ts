/* [109A-4] Capacidad ausente en el entorno actual.
 *
 * Los adaptadores web rechazan las acciones que solo puede resolver el
 * escritorio (memorias, comandos del área). Distinguir "esta capacidad no
 * existe aquí" de "algo ha fallado" evita dos errores opuestos: no se
 * silencia un fallo real y no se avisa al usuario de algo esperado en ese
 * entorno (donde la UI ya degrada con lo que sí tiene).
 */

/** `name` compartido por los errores de capacidad ausente. */
export const CAPACIDAD_AUSENTE = 'CapacidadAusente';

export function errorCapacidadAusente(motivo: string): Error {
  const e = new Error(motivo);
  e.name = CAPACIDAD_AUSENTE;
  return e;
}

export function esCapacidadAusente(e: unknown): boolean {
  return e instanceof Error && e.name === CAPACIDAD_AUSENTE;
}
