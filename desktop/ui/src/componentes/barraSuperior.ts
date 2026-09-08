// ============================================================
// Barra superior global (089A-3, referencia Synara
// `apps/web/src/components/DesktopWindowControls.tsx` + `ChatHeader.tsx`).
// Ocupa todo el ancho por encima de sidebar/paneles/panel derecho
// (primera hija de #app, 46px como la title-bar de Synara): a la izquierda
// los toggles globales (sidebar, panel derecho, visor —mudados desde la
// cabecera del principal—), en el centro la marca (zona libre de arrastre)
// y a la derecha la botonera de ventana estilo caption de Synara.
// Arrastre + botonera solo bajo Tauri (`esEntornoTauri`); en web la barra
// queda con solo los toggles (siguen los nativos del navegador).
// ============================================================

import '../estilos/barraSuperior.css';
import { icono } from './iconos';
import { el } from '../util/dom';
import { esEntornoTauri } from '../tauri/real';
import { crearControlesVentana, hacerArrastrable } from './ventana';

export interface BarraSuperior {
  raiz: HTMLElement;
  /** [089A-3] Refleja la lista visible/oculta en el toggle izquierdo. */
  setSidebarAbierta(abierta: boolean): void;
  /** [089A-3] Refleja el panel derecho visible/oculto en su toggle. */
  setPanelDerechoAbierto(abierto: boolean): void;
}

export interface BarraSuperiorOpciones {
  /** Alterna la lista de conversaciones (sidebar). */
  onToggleSidebar?: () => void;
  /** Alterna el panel derecho (tabs). */
  onTogglePanelDerecho?: () => void;
  /** Abre el tab Visor del panel derecho. */
  onAbrirVisor?: () => void;
}

export function montarBarraSuperior(opts: BarraSuperiorOpciones): BarraSuperior {
  const barra = el('div', 'barra-superior');
  barra.id = 'barra-superior';
  barra.setAttribute('role', 'banner');

  const grupo = el('div', 'barra-grupo');
  grupo.setAttribute('aria-label', 'controles globales');

  const btnSidebar = el('button', 'bar-boton') as HTMLButtonElement;
  btnSidebar.type = 'button';
  btnSidebar.title = 'ocultar lista de conversaciones';
  btnSidebar.setAttribute('aria-label', 'ocultar lista de conversaciones');
  btnSidebar.appendChild(icono('panel-izq-cerrar'));
  btnSidebar.addEventListener('click', () => opts.onToggleSidebar?.());
  grupo.appendChild(btnSidebar);

  const btnDerecho = el('button', 'bar-boton') as HTMLButtonElement;
  btnDerecho.type = 'button';
  btnDerecho.title = 'mostrar panel derecho';
  btnDerecho.setAttribute('aria-label', 'mostrar panel derecho');
  btnDerecho.appendChild(icono('panel-der-abrir'));
  btnDerecho.addEventListener('click', () => opts.onTogglePanelDerecho?.());
  grupo.appendChild(btnDerecho);

  if (opts.onAbrirVisor) {
    const btnVisor = el('button', 'bar-boton') as HTMLButtonElement;
    btnVisor.type = 'button';
    btnVisor.title = 'visor de archivo y cambios';
    btnVisor.setAttribute('aria-label', 'visor de archivo y cambios');
    btnVisor.appendChild(icono('archivo'));
    btnVisor.addEventListener('click', () => opts.onAbrirVisor?.());
    grupo.appendChild(btnVisor);
  }

  // Centro libre: marca + zona de arrastre de la ventana.
  const centro = el('div', 'barra-centro');
  const marca = el('span', 'barra-marca');
  marca.textContent = 'glory-harness';
  centro.appendChild(marca);

  barra.appendChild(grupo);
  barra.appendChild(centro);

  if (esEntornoTauri()) {
    hacerArrastrable(barra);
    barra.appendChild(crearControlesVentana());
  }

  return {
    raiz: barra,
    setSidebarAbierta(abierta: boolean) {
      btnSidebar.replaceChildren(icono(abierta ? 'panel-izq-cerrar' : 'panel-izq-abrir'));
      const etiqueta = abierta
        ? 'ocultar lista de conversaciones'
        : 'mostrar lista de conversaciones';
      btnSidebar.title = etiqueta;
      btnSidebar.setAttribute('aria-label', etiqueta);
    },
    setPanelDerechoAbierto(abierto: boolean) {
      btnDerecho.replaceChildren(icono(abierto ? 'panel-der-cerrar' : 'panel-der-abrir'));
      const etiqueta = abierto ? 'ocultar panel derecho' : 'mostrar panel derecho';
      btnDerecho.title = etiqueta;
      btnDerecho.setAttribute('aria-label', etiqueta);
    },
  };
}
