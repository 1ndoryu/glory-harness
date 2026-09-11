// [119A-3 F1] Orden de hilos de la sidebar: botón junto al + de la
// cabecera Proyectos con el menú `Sort threads` de la referencia
// (Último mensaje / Creación, ✓ en el activo). La comparación vive aquí
// para no engordar `sidebar.ts`; F2 añade la sección `Sort projects` al
// mismo menú y F4 persiste la preferencia (hoy solo estado de sesión).

import type { Conversacion } from '../dominio/tipos';
import { icono } from './iconos';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { el } from '../util/dom';

/** Criterio de orden de hilos (referencia Paseo). */
export type CriterioHilos = 'actividad' | 'creacion';

/** Etiquetas visibles del menú (español, como el resto de la sidebar). */
export const ETIQUETAS_CRITERIO_HILOS: Record<CriterioHilos, string> = {
  actividad: 'Último mensaje',
  creacion: 'Creación',
};

/** RFC3339 a ms; lo ilegible/ausente vale 0 (queda al final). */
function aTiempo(valor: string | undefined): number {
  if (!valor) return 0;
  const t = Date.parse(valor);
  return Number.isNaN(t) ? 0 : t;
}

/** Comparador descendente según el criterio (recientes primero). */
export function compararConversaciones(criterio: CriterioHilos) {
  return (a: Conversacion, b: Conversacion): number => {
    const campo = criterio === 'creacion' ? 'creadaEn' : 'actualizadaEn';
    return aTiempo(b[campo]) - aTiempo(a[campo]);
  };
}

/** Ordena una copia del grupo según el criterio (no muta la entrada). */
export function ordenarGrupo(grupo: Conversacion[], criterio: CriterioHilos): Conversacion[] {
  return [...grupo].sort(compararConversaciones(criterio));
}

export interface OrdenHilosDeps {
  /** Criterio actual (lo conserva la sidebar; F4 lo persistirá). */
  criterio: () => CriterioHilos;
  /** Se invoca al elegir otro criterio (la sidebar repinta). */
  alCambiar: (criterio: CriterioHilos) => void;
}

/** Botón `.prog-orden` con el menú de criterio de hilos. */
export function crearBotonOrdenHilos(deps: OrdenHilosDeps): HTMLButtonElement {
  const b = el('button', 'prog-orden') as HTMLButtonElement;
  b.type = 'button';
  b.title = 'Ordenar hilos';
  b.setAttribute('aria-label', 'Ordenar hilos');
  b.appendChild(icono('ordenar', true));
  b.addEventListener('click', (e) => {
    e.stopPropagation();
    const rect = b.getBoundingClientRect();
    const actual = deps.criterio();
    abrirMenuContextual({
      rect,
      construir: (menu) => {
        (Object.keys(ETIQUETAS_CRITERIO_HILOS) as CriterioHilos[]).forEach((c) => {
          menu.appendChild(
            crearItemMenu({
              texto: ETIQUETAS_CRITERIO_HILOS[c],
              marcado: c === actual,
              onClick: () => {
                cerrarMenuActual();
                if (c !== deps.criterio()) deps.alCambiar(c);
              },
            }),
          );
        });
      },
    });
  });
  return b;
}
