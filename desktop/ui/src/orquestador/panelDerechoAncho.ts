/* Ancho del panel derecho: divisor arrastrable + persistencia.
 *
 * Extraído de `panelDerecho.ts` en 109A-6 para no rebasar el techo de líneas
 * del componente, sin cambiar comportamiento ni contrato persistido: el ancho
 * sigue viviendo en `--panel-derecho-ancho` sobre #cuerpo y #app, en el rango
 * [460, tope] y en la misma clave de preferencia. El tope es lo que cabe en
 * la fila sin desbordar (tope 70% con viewport enano).
 *
 * El grip se crea una sola vez (lo pide `asegurarPanelDerecho`) y se monta o
 * desmonta con el panel: este módulo no decide la visibilidad, solo mide,
 * aplica y persiste. */

import { el, marcarCuerpo } from '../util/dom';
import { seguirPuntero } from '../plataforma/ventana';
import { CLAVE_LATERAL_ANCHO, guardarSidebar, leerSidebar, type PersistenciaDeps } from './persistencia';

/** [07AA-1] Ancho mínimo del panel derecho en px (mínimo global de paneles). */
const ANCHO_MIN = 460;
/** Fracción máxima del ancho de #cuerpo que puede ocupar el panel. */
const FRACCION_MAX = 0.7;
/** Mínimo del chat principal en px (`--panel-ancho-min`): lo que la fila
 *  reserva sí o sí además de la sidebar. */
const CHAT_MIN = 460;

export interface AnchoPanelDerechoDeps {
  cuerpo: HTMLElement;
  app: HTMLElement;
  persistencia: PersistenciaDeps;
}

export interface AnchoPanelDerecho {
  /** Divisor listo para insertar en la raíz del panel. */
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
    const vista = deps.cuerpo.getBoundingClientRect().width;
    /* [07AA-1 F4] La fila (sidebar + chat + panel) no debe desbordar: si lo
     * hace, las tabs de la barra superior (ancladas a la derecha del
     * viewport) se despegan del panel (anclado a la fila con scroll) y todo
     * el arrastre "se mueve raro". El tope es lo que cabe una vez
     * reservados sidebar y mínimo del chat; con viewport enano se vuelve
     * al 70% y la fila desplaza (red de F2). */
    const sidebar = deps.cuerpo.querySelector<HTMLElement>('#sidebar');
    const sidebarAncho = sidebar ? sidebar.getBoundingClientRect().width : 0;
    const cabe = Math.round(vista - sidebarAncho - CHAT_MIN);
    const max = Math.max(
      ANCHO_MIN,
      Math.min(Math.round(vista * FRACCION_MAX), cabe),
    );
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
        /* El panel derecho está ANCLADO al final de la fila: su ancho es la
         * distancia del cursor hasta ese borde. [07AA-1 F3] En coordenadas
         * de contenido (con scroll horizontal, `rect.right` miente: el borde
         * real está en `scrollWidth - scrollLeft` desde el origen visible). */
        const rect = deps.cuerpo.getBoundingClientRect();
        const bordeContenido =
          rect.left + deps.cuerpo.scrollWidth - deps.cuerpo.scrollLeft;
        aplicar(acotar(bordeContenido - e.clientX));
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
