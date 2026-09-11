// Barra superior global (089A-3, referencia Synara
// `apps/web/src/components/SidebarHeaderNavigationControls.tsx` y
// `AppNavigationButtons.tsx`).
// Orden fiel a Synara, de izquierda a derecha: toggle de la lista,
// atrás, adelante … marca … zona derecha con tabs, toggle del panel y
// botonera de ventana (min, max/restaurar, cerrar).
// Todos los iconos son Lucide (`iconos.ts`); la botonera conserva el
// estilo caption nativo (46px, planos, cerrar hover #c42b1c).
// La barra es la zona arrastrable de la ventana; arrastre + botonera
// solo bajo Tauri (en web la marca ocupa el centro y no hay botonera).
// Sin botón de visor: es redundante (decisión del usuario 08-09).
// Atrás/adelante replican la navegación por historial de Synara (089A-5) y
// arrancan deshabilitados hasta la primera navegación.

import '../estilos/barraSuperior.css';
import { esEntornoTauri } from '../tauri/real';
import { icono } from './iconos';
import { el } from '../util/dom';
import { crearControlesVentana, hacerArrastrable } from './ventana';

export interface BarraSuperiorOpciones {
  /** Alterna la lista de conversaciones (sidebar). */
  onAlternarSidebar(): void;
  /** Atrás en el historial de la app (misma lógica que Synara). */
  onAtras(): void;
  /** Adelante en el historial de la app (misma lógica que Synara). */
  onAdelante(): void;
  /** Muestra/oculta el panel derecho. */
  onAlternarPanelDerecho(): void;
  /** Fallo del arrastre de ventana (llega al toast, nunca a consola). */
  onErrorVentana?(detalle: string): void;
}

export interface BarraSuperior {
  raiz: HTMLElement;
  /** Icono del toggle según esté abierta la lista (cerrar/abrir). */
  setSidebarAbierta(abierta: boolean): void;
  /** Icono del toggle según esté visible el panel derecho. */
  setPanelDerechoAbierto(abierto: boolean): void;
  /** Habilita atrás/adelante según haya a dónde ir (historial 089A-5). */
  setPuedeNavegar(atras: boolean, adelante: boolean): void;
  /** Monta la única barra de tabs en la zona derecha superior. */
  montarTabs(tabsBarra: HTMLElement): void;
}

/** Botón de la barra (Lucide, monocromo). */
function botonBarra(
  iconoAbierto: 'panel-izq-cerrar' | 'panel-izq-abrir' | 'flecha-izq' | 'flecha-der' | 'panel-der-cerrar' | 'panel-der-abrir',
  etiqueta: string,
  alPulsar: () => void,
): HTMLButtonElement {
  const btn = el('button', 'barra-boton') as HTMLButtonElement;
  btn.type = 'button';
  btn.title = etiqueta;
  btn.setAttribute('aria-label', etiqueta);
  btn.appendChild(icono(iconoAbierto));
  btn.addEventListener('click', alPulsar);
  return btn;
}

export function montarBarraSuperior(opts: BarraSuperiorOpciones): BarraSuperior {
  const barra = el('div', 'barra-superior');
  barra.setAttribute('aria-label', 'barra superior');

  // ---- Grupo izquierdo (orden Synara): lista, atrás, adelante ----
  const grupoIzq = el('div', 'barra-grupo');
  const btnSidebar = botonBarra('panel-izq-cerrar', 'ocultar lista de conversaciones', () =>
    opts.onAlternarSidebar(),
  );
  grupoIzq.appendChild(btnSidebar);
  const btnAtras = botonBarra('flecha-izq', 'atrás', () => opts.onAtras());
  const btnAdelante = botonBarra('flecha-der', 'adelante', () => opts.onAdelante());
  // Historial de la app (089A-5): deshabilitados hasta la primera
  // navegación, como en Synara cuando no hay a dónde ir
  // (`canGoBack`/`canGoForward`).
  btnAtras.disabled = true;
  btnAdelante.disabled = true;
  grupoIzq.appendChild(btnAtras);
  grupoIzq.appendChild(btnAdelante);
  barra.appendChild(grupoIzq);

  // ---- Marca (zona central, arrastrable) ----
  const marca = el('div', 'barra-marca');
  const nombre = el('span', 'barra-nombre');
  nombre.textContent = 'glory-harness';
  marca.appendChild(nombre);
  barra.appendChild(marca);

  // ---- Zona derecha: tabs + toggle del panel + botonera ----
  const zonaDerecha = el('div', 'barra-zona-derecha');
  const grupoDer = el('div', 'barra-grupo');
  const btnDerecho = botonBarra('panel-der-abrir', 'mostrar panel derecho', () =>
    opts.onAlternarPanelDerecho(),
  );
  grupoDer.appendChild(btnDerecho);
  zonaDerecha.appendChild(grupoDer);
  barra.appendChild(zonaDerecha);

  if (esEntornoTauri()) {
    hacerArrastrable(barra, opts.onErrorVentana);
    grupoDer.appendChild(crearControlesVentana());
  }

  return {
    raiz: barra,
    setSidebarAbierta(abierta: boolean) {
      btnSidebar.replaceChildren(
        icono(abierta ? 'panel-izq-cerrar' : 'panel-izq-abrir'),
      );
      const etiqueta = abierta
        ? 'ocultar lista de conversaciones'
        : 'mostrar lista de conversaciones';
      btnSidebar.title = etiqueta;
      btnSidebar.setAttribute('aria-label', etiqueta);
    },
    setPanelDerechoAbierto(abierto: boolean) {
      zonaDerecha.classList.toggle('abierto', abierto);
      btnDerecho.replaceChildren(
        icono(abierto ? 'panel-der-cerrar' : 'panel-der-abrir'),
      );
      const etiqueta = abierto ? 'ocultar panel derecho' : 'mostrar panel derecho';
      btnDerecho.title = etiqueta;
      btnDerecho.setAttribute('aria-label', etiqueta);
    },
    setPuedeNavegar(atras: boolean, adelante: boolean) {
      btnAtras.disabled = !atras;
      btnAdelante.disabled = !adelante;
    },
    montarTabs(tabsBarra) {
      zonaDerecha.insertBefore(tabsBarra, grupoDer);
    },
  };
}
