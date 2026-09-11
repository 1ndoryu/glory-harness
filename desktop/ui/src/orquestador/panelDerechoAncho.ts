/* Ancho del panel derecho: divisor arrastrable + persistencia.
 *
 * Extraído de `panelDerecho.ts` en 109A-6 para no rebasar el techo de líneas
 * del componente, sin cambiar comportamiento ni contrato persistido: el ancho
 * sigue viviendo en `--panel-derecho-ancho` sobre #cuerpo y #app, en el rango
 * [260, 70%] y en la misma clave de preferencia.
 *
 * El grip se crea una sola vez (lo pide `asegurarPanelDerecho`) y se monta o
 * desmonta con el panel: este módulo no decide la visibilidad, solo mide,
 * aplica y persiste. */

import { el, marcarCuerpo } from '../util/dom';
import { seguirPuntero } from '../plataforma/ventana';
import { CLAVE_LATERAL_ANCHO, guardarSidebar, leerSidebar, type PersistenciaDeps } from './persistencia';

/** Ancho mínimo del panel derecho en px (por debajo su contenido no cabe). */
const ANCHO_MIN = 260;
/** Fracción máxima del ancho de #cuerpo que puede ocupar el panel. */
const FRACCION_MAX = 0.7;

export interface AnchoPanelDerechoDeps {
  cuerpo: HTMLElement;
  app: HTMLElement;
  persistencia: PersistenciaDeps;
}

export interface AnchoPanelDerecho {
  /** Divisor listo para insertar en #cuerpo. */
  crearGrip(): HTMLElement;
  /** Aplica un ancho concreto (px) a las dos raíces que lo consumen. */
  aplicar(px: number): void;
  /** Acota un ancho pedido al rango permitido con el #cuerpo actual. */
  acotar(px: number): number;
  /** Restaura el ancho persistido si aún no se fijó ninguno en esta sesión. */
  restaurar(): void;
}

export function crearAnchoPanelDerecho(deps: AnchoPanelDerechoDeps): AnchoPanelDerecho {
  let grip: HTMLElement | null = null;
  // `null` = nunca fijado en esta sesión: solo entonces se restaura lo guardado.
  let fijado: number | null = null;

  function acotar(px: number): number {
    const max = Math.round(deps.cuerpo.getBoundingClientRect().width * FRACCION_MAX);
    return Math.min(max, Math.max(ANCHO_MIN, Math.round(px)));
  }

  function aplicar(px: number): void {
    fijado = px;
    deps.cuerpo.style.setProperty('--panel-derecho-ancho', `${px}px`);
    deps.app.style.setProperty('--panel-derecho-ancho', `${px}px`);
  }

  function crearGrip(): HTMLElement {
    if (grip) return grip;
    const g = el('div', 'panel-derecho-grip');
    g.setAttribute('aria-hidden', 'true');
    let arrastrando = false;
    g.addEventListener('mousedown', (e) => {
      e.preventDefault();
      arrastrando = true;
      marcarCuerpo('redimensionando-lateral', true);
    });
    seguirPuntero(
      (e) => {
        if (!arrastrando) return;
        /* El panel derecho está ANCLADO al borde derecho de #cuerpo: su ancho
         * es la distancia del cursor hasta ese borde. */
        const rect = deps.cuerpo.getBoundingClientRect();
        aplicar(acotar(rect.right - e.clientX));
      },
      () => {
        if (!arrastrando) return;
        arrastrando = false;
        marcarCuerpo('redimensionando-lateral', false);
        if (fijado !== null) {
          guardarSidebar(deps.persistencia, CLAVE_LATERAL_ANCHO, String(fijado));
        }
      },
    );
    grip = g;
    return g;
  }

  function restaurar(): void {
    if (fijado !== null) return;
    // Sin preferencia se parte de la mitad del ancho disponible.
    let px = Math.round(deps.cuerpo.getBoundingClientRect().width * 0.5);
    const base = leerSidebar(deps.persistencia, CLAVE_LATERAL_ANCHO);
    if (base) {
      const n = Number(base);
      if (Number.isFinite(n)) px = n;
    }
    aplicar(acotar(px));
  }

  return { crearGrip, aplicar, acotar, restaurar };
}
