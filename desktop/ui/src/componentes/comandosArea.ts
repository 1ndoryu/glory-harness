/* Catálogo vivo de comandos `/` del área activa (plan 109A-4 F2).
 *
 * El backend resuelve la carpeta (`.glory/comandos` del área activa) sin que
 * el front envíe ninguna ruta, así que este módulo solo se encarga de:
 *
 * - cachear el último catálogo leído,
 * - no repetir lecturas que no aportan (`refrescar` recargable),
 * - avisar a quien lo consuma (el compositor) cuando cambia.
 *
 * Un fallo de lectura NO borra el catálogo anterior: el menú seguiría
 * mostrando comandos de otra área si el área actual fallara, y eso es peor
 * que quedarse con lo último bueno mientras se avisa.
 */

import type { ComandoProyecto } from '../dominio/comandosSlash';
import { esCapacidadAusente } from '../util/capacidad';

export interface ComandosArea {
  /** Catálogo vigente (lista vacía mientras no haya lectura correcta). */
  lista(): ComandoProyecto[];
  /** Recarga desde el backend (idempotente y reentrante sin solapar). */
  refrescar(): Promise<void>;
  /** `true` si ya se intentó una lectura (con éxito o sin él). */
  cargado(): boolean;
  /** Registra un oyente de cambios de catálogo. */
  alCambiar(fn: () => void): void;
}

export interface ComandosAreaDeps {
  /** Lectura del backend (Tauri: `comandos_listar`). */
  listar(): Promise<ComandoProyecto[]>;
  /** Aviso no bloqueante (el menú sigue funcionando con los integrados). */
  avisar(texto: string): void;
}

export function crearComandosArea(deps: ComandosAreaDeps): ComandosArea {
  let comandos: ComandoProyecto[] = [];
  let cargado = false;
  let enCurso: Promise<void> | null = null;
  const oyentes: Array<() => void> = [];

  function aplicar(nuevos: ComandoProyecto[]): void {
    // Comparación por contenido: evitar repintados inútiles del menú.
    const igual =
      nuevos.length === comandos.length &&
      nuevos.every((c, i) => c.nombre === comandos[i].nombre && c.descripcion === comandos[i].descripcion);
    comandos = nuevos;
    if (!igual) for (const fn of oyentes) fn();
  }

  async function leer(): Promise<void> {
    try {
      aplicar(await deps.listar());
    } catch (e: unknown) {
      // En entornos sin la capacidad (web) no se avisa: el menú sigue con los
      // comandos integrados. Cualquier otro fallo sí se muestra.
      if (!esCapacidadAusente(e)) {
        deps.avisar(`no se pudieron leer los comandos del área: ${String(e)}`);
      }
    } finally {
      cargado = true;
      enCurso = null;
    }
  }

  return {
    lista: () => comandos,
    refrescar() {
      // Sin solapar: dos cambios de área seguidos comparten la misma lectura.
      enCurso ??= leer();
      return enCurso;
    },
    cargado: () => cargado,
    alCambiar(fn) {
      oyentes.push(fn);
    },
  };
}
