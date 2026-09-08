// ============================================================
// Controles de ventana propios (089A-1, referencia Paseo
// `packages/app/src/components/desktop/window-controls.tsx`).
// Orden: minimizar, maximizar/restaurar (dinámico según
// `isMaximized()`), cerrar. Solo visual bajo Tauri: `cabecera.ts`
// los monta únicamente en el panel principal cuando hay
// `__TAURI__`; en web no se montan (siguen los nativos).
// El cableado usa import dinámico de `@tauri-apps/api/window` para
// no romper el bundle web; si falla, los botones quedan
// deshabilitados en vez de fallar en silencio.
// ============================================================

import '../estilos/ventana.css';
import { icono } from './iconos';
import { el } from '../util/dom';

/** Ventana Tauri precargada (el import dinámico no bloquea el bundle web). */
let ventanaFutura: Promise<{ startDragging(): Promise<void> }> | null = null;
function ventana(): Promise<{ startDragging(): Promise<void> }> {
  if (!ventanaFutura) {
    ventanaFutura = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow());
  }
  return ventanaFutura;
}

/**
 * Hace una cabecera arrastrable (mover la ventana).
 * Combina `data-tauri-drag-region` con `startDragging()` programático en
 * mousedown: el atributo solo a veces no basta (p. ej. según el runtime
 * WebView2); el programático siempre funciona. Los clics en botones,
 * inputs y enlaces no arrastran (siguen clicando).
 */
export function hacerArrastrable(cabecera: HTMLElement): void {
  cabecera.setAttribute('data-tauri-drag-region', '');
  cabecera.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const objetivo = e.target as HTMLElement | null;
    if (objetivo?.closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
    void ventana()
      .then((v) => v.startDragging())
      .catch(() => {});
  });
}

/** Crea la botonera de ventana (sin anclar; lo hace `cabecera.ts`). */
export function crearControlesVentana(): HTMLElement {
  const grupo = el('div', 'controles-ventana');
  grupo.setAttribute('aria-label', 'controles de la ventana');

  const btnMin = botonVentana('minimizar', 'minimizar ventana');
  const btnMax = botonVentana('maximizar', 'maximizar ventana');
  const btnCerrar = botonVentana('x', 'cerrar ventana');
  btnCerrar.classList.add('ven-cerrar');
  grupo.appendChild(btnMin);
  grupo.appendChild(btnMax);
  grupo.appendChild(btnCerrar);

  // Solo llega aquí bajo Tauri, pero el import es dinámico para que el
  // bundle web no dependa de `@tauri-apps/api/window` en estático.
  void import('@tauri-apps/api/window')
    .then(({ getCurrentWindow }) => {
      const ventana = getCurrentWindow();
      const refrescarMax = () => {
        ventana
          .isMaximized()
          .then((max) => {
            btnMax.replaceChildren(icono(max ? 'restaurar' : 'maximizar'));
            const etiqueta = max ? 'restaurar ventana' : 'maximizar ventana';
            btnMax.title = etiqueta;
            btnMax.setAttribute('aria-label', etiqueta);
          })
          .catch(() => deshabilitar(btnMax));
      };
      btnMin.addEventListener('click', () => {
        ventana.minimize().catch(() => deshabilitar(btnMin));
      });
      btnMax.addEventListener('click', () => {
        ventana
          .toggleMaximize()
          .then(() => refrescarMax())
          .catch(() => deshabilitar(btnMax));
      });
      btnCerrar.addEventListener('click', () => {
        ventana.close().catch(() => deshabilitar(btnCerrar));
      });
      refrescarMax();
    })
    .catch(() => {
      // Sin API de ventana (no debería pasar bajo Tauri): botones
      // visibles pero deshabilitados, nunca muertos en silencio.
      deshabilitar(btnMin);
      deshabilitar(btnMax);
      deshabilitar(btnCerrar);
    });

  return grupo;
}

function botonVentana(
  iconoNombre: 'minimizar' | 'maximizar' | 'x',
  etiqueta: string,
): HTMLButtonElement {
  const btn = el('button', 'cab-boton ven-boton') as HTMLButtonElement;
  btn.type = 'button';
  btn.title = etiqueta;
  btn.setAttribute('aria-label', etiqueta);
  btn.appendChild(icono(iconoNombre));
  return btn;
}

function deshabilitar(btn: HTMLButtonElement): void {
  btn.disabled = true;
  btn.title += ' (no disponible)';
}
