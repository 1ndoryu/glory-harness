// ============================================================
// Controles de ventana propios (089A-1 + 089A-3, referencias Paseo
// `packages/app/src/components/desktop/window-controls.tsx` y Synara
// `apps/web/src/components/DesktopWindowControls.tsx`).
// Orden: minimizar, maximizar/restaurar (dinámico según
// `isMaximized()`), cerrar. Estilo caption nativo Windows como Synara:
// botones de 46px de ancho y alto completo de la barra, planos (sin
// radio ni borde), glifo centrado con "Segoe Fluent Icons" (Windows 11)
// y fallback a "Segoe MDL2 Assets" (Windows 10). Solo visual bajo
// Tauri: `barraSuperior.ts` los monta únicamente cuando hay
// `__TAURI__`; en web no se montan (siguen los nativos).
// El cableado usa import dinámico de `@tauri-apps/api/window` para
// no romper el bundle web; si falla, los botones quedan
// deshabilitados en vez de fallar en silencio.
// ============================================================

import '../estilos/ventana.css';
import { el } from '../util/dom';

// Glifos caption nativos Windows (Synara): minimizar (E921),
// maximizar (E922), restaurar (E923, cuadrados solapados) y
// cerrar (E8BB).
const GLIFOS_VENTANA = {
  minimizar: '',
  maximizar: '',
  restaurar: '',
  cerrar: '',
} as const;

/** Ventana Tauri precargada (el import dinámico no bloquea el bundle web). */
let ventanaFutura: Promise<{ startDragging(): Promise<void> }> | null = null;
function ventana(): Promise<{ startDragging(): Promise<void> }> {
  if (!ventanaFutura) {
    ventanaFutura = import('@tauri-apps/api/window').then((m) => m.getCurrentWindow());
  }
  return ventanaFutura;
}

/**
 * Hace una zona arrastrable (mover la ventana).
 * Combina `data-tauri-drag-region` con `startDragging()` programático en
 * mousedown: el atributo solo a veces no basta (p. ej. según el runtime
 * WebView2); el programático siempre funciona. Los clics en botones,
 * inputs y enlaces no arrastran (siguen clicando).
 * El módulo de ventana se precarga al montar (no en el primer mousedown):
 * `startDragging()` debe despacharse dentro del gesto del ratón y la
 * primera carga diferida llegaría tarde. El fallo se avisa por consola
 * (nunca mudo) para diagnosticar permisos/capabilities.
 */
export function hacerArrastrable(zona: HTMLElement): void {
  zona.setAttribute('data-tauri-drag-region', '');
  // Precarga inmediata: cuando el usuario pulse, el módulo ya está listo.
  void ventana().catch(() => {});
  zona.addEventListener('mousedown', (e) => {
    if (e.button !== 0) return;
    const objetivo = e.target as HTMLElement | null;
    if (objetivo?.closest('button, input, textarea, select, a, [contenteditable="true"]')) return;
    void ventana()
      .then((v) => v.startDragging())
      .catch((err) => {
        console.warn('[ventana] startDragging falló (revisa capabilities window):', err);
      });
  });
}

/** Crea la botonera de ventana (sin anclar; la monta `barraSuperior.ts`). */
export function crearControlesVentana(): HTMLElement {
  const grupo = el('div', 'controles-ventana');
  grupo.setAttribute('aria-label', 'controles de la ventana');

  const btnMin = botonVentana('minimizar', 'minimizar ventana');
  const { boton: btnMax, glifo: glifoMax } = botonVentanaMax();
  const btnCerrar = botonVentana('cerrar', 'cerrar ventana');
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
            glifoMax.textContent = GLIFOS_VENTANA[max ? 'restaurar' : 'maximizar'];
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

/** Botón caption con glifo Segoe (minimizar / cerrar). */
function botonVentana(tipo: 'minimizar' | 'cerrar', etiqueta: string): HTMLButtonElement {
  const btn = el('button', 'caption-boton') as HTMLButtonElement;
  btn.type = 'button';
  btn.title = etiqueta;
  btn.setAttribute('aria-label', etiqueta);
  const glifo = el('span', 'caption-glifo');
  glifo.setAttribute('aria-hidden', 'true');
  glifo.textContent = GLIFOS_VENTANA[tipo];
  btn.appendChild(glifo);
  return btn;
}

/** Botón caption de maximizar/restaurar (expone su glifo para el refresco). */
function botonVentanaMax(): { boton: HTMLButtonElement; glifo: HTMLElement } {
  const btn = el('button', 'caption-boton') as HTMLButtonElement;
  btn.type = 'button';
  btn.title = 'maximizar ventana';
  btn.setAttribute('aria-label', 'maximizar ventana');
  const glifo = el('span', 'caption-glifo');
  glifo.setAttribute('aria-hidden', 'true');
  glifo.textContent = GLIFOS_VENTANA.maximizar;
  btn.appendChild(glifo);
  return { boton: btn, glifo };
}

function deshabilitar(btn: HTMLButtonElement): void {
  btn.disabled = true;
  btn.title += ' (no disponible)';
}
