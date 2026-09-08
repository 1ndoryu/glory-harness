// ============================================================
// Cabecera del chat (título de la conversación activa + acciones).
// [039A-3 P4] Añade el botón ⋯ (menú de acciones de la conversación,
// reutilizando menu.ts) y el botón de colapsar/expandir la sidebar
// junto al título. El título puede entrar en edición inline (renombrar
// desde el ⋯) igual que una fila de la sidebar.
// [039A-3 P5] La cabecera es duplicable: por eso la raíz lleva la clase
// `.cabecera-chat` (no id). El botón de colapsar sidebar SOLO se monta
// en el panel principal; un panel lateral monta en su lugar el botón ×
// de cierre (`cerrable: true`). Los ids internos se sustituyen por un
// prefijo por instancia (`idPrefijo`) para no colisionar entre paneles.
// ============================================================

import { icono } from './iconos';
import { el } from '../util/dom';
import { esEntornoTauri } from '../tauri/real';
import { crearControlesVentana, hacerArrastrable } from './ventana';

export interface CabeceraChat {
  raiz: HTMLElement;
  /** Cambia el título mostrado. */
  ponerTitulo(texto: string): void;
  /** [089A-2] Refleja el estado visible/oculto de la lista en el botón de
   * alternar (siempre visible): con la lista visible muestra el icono de
   * ocultar y viceversa. No-op en un panel lateral (no tiene toggle). */
  setSidebarAbierta(abierta: boolean): void;
  /** [089A-2] Refleja el estado visible/oculto del panel derecho en el
   * botón de alternar (siempre visible en el principal, a la derecha del
   * título): abierto muestra el icono de plegar y viceversa. */
  setPanelDerechoAbierto(abierto: boolean): void;
  /** [039A-3 P4] Pone el título en edición inline (renombrar). Se usa desde
   * el ⋯ de la cabecera; al guardar llama `onGuardar(nuevo)`. */
  empezarRenombrar(
    titulo: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void;
}

export interface CabeceraChatOpciones {
  /** Prefijo de los ids internos (una instancia por panel). */
  idPrefijo: string;
  titulo: string;
  /** [039A-3 P5] Panel principal (`false`, default): botón toggle sidebar.
   * Panel lateral (`true`): botón × de cierre en lugar del toggle. */
  lateral?: boolean;
  /** Se invoca al pulsar el botón ⋯: abre el menú de acciones de la
   * conversación activa (el ⋯ solo dispara; main.ts construye el menú para
   * tener acceso a la conversación actual). */
  onAcciones: (rect: DOMRect) => void;
  /** Se invoca al pulsar el botón de colapsar/expandir la sidebar. */
  onToggleSidebar?: () => void;
  /** [089A-2] Se invoca al pulsar el botón de mostrar/ocultar el panel
   * derecho (solo principal, siempre visible). */
  onTogglePanelDerecho?: () => void;
  /** [089A-2] Se invoca al pulsar el botón del visor (solo principal). */
  onAbrirVisor?: () => void;
  /** [039A-3 P5] Se invoca al pulsar el botón × de un panel lateral. */
  onCerrar?: () => void;
}

/** Mecánica de renombrar inline compartida (fila de sidebar y cabecera). */
export function montarCabeceraChat(opts: CabeceraChatOpciones): CabeceraChat {
  const cab = el('div', 'cabecera-chat');
  let titulo = opts.titulo;
  const lateral = opts.lateral ?? false;

  // ---- acciones: colapsar sidebar (principal) / × cerrar (lateral) + ⋯ ----
  // [039A-3 P6b] El botón ⋯ va al EXTREMO DERECHO de la cabecera: se monta en
  // un grupo propio `acciones-mas` tras el título (el toggle/× queda a la
  // izquierda). Así el título queda alineado a la izquierda y ⋯ a la derecha.
  const acciones = el('div', 'acciones-cabecera');
  acciones.setAttribute('aria-label', 'acciones de la conversación');

  let btnToggle: HTMLButtonElement | null = null;
  let btnCerrar: HTMLButtonElement | null = null;

  if (!lateral) {
    // [089A-2] El botón de la lista SIEMPRE visible: alterna mostrar/ocultar
    // (antes solo aparecía para mostrar cuando la lista estaba oculta).
    btnToggle = el('button', 'cab-boton') as HTMLButtonElement;
    btnToggle.id = `${opts.idPrefijo}-alternar-sidebar`;
    btnToggle.type = 'button';
    btnToggle.title = 'ocultar lista de conversaciones';
    btnToggle.setAttribute('aria-label', 'ocultar lista de conversaciones');
    btnToggle.appendChild(icono('panel-izq-cerrar'));
    btnToggle.addEventListener('click', () => opts.onToggleSidebar?.());
    acciones.appendChild(btnToggle);
    // [089A-2] Botón del visor (archivo y cambios): solo en el principal,
    // junto al toggle de la lista. Abre el tab Visor del panel derecho.
    if (!lateral && opts.onAbrirVisor) {
      const btnVisor = el('button', 'cab-boton') as HTMLButtonElement;
      btnVisor.id = `${opts.idPrefijo}-abrir-visor`;
      btnVisor.type = 'button';
      btnVisor.title = 'visor de archivo y cambios';
      btnVisor.setAttribute('aria-label', 'visor de archivo y cambios');
      btnVisor.appendChild(icono('archivo'));
      btnVisor.addEventListener('click', () => opts.onAbrirVisor?.());
      acciones.appendChild(btnVisor);
    }
  } else {
    btnCerrar = el('button', 'cab-boton cab-cerrar') as HTMLButtonElement;
    btnCerrar.id = `${opts.idPrefijo}-cerrar-panel`;
    btnCerrar.type = 'button';
    btnCerrar.title = 'cerrar panel lateral';
    btnCerrar.setAttribute('aria-label', 'cerrar panel lateral');
    btnCerrar.appendChild(icono('x'));
    btnCerrar.addEventListener('click', () => opts.onCerrar?.());
    acciones.appendChild(btnCerrar);
  }

  const btnMas = el('button', 'cab-boton') as HTMLButtonElement;
  btnMas.id = `${opts.idPrefijo}-acciones-chat`;
  btnMas.type = 'button';
  btnMas.title = 'acciones de la conversación';
  btnMas.setAttribute('aria-label', 'acciones de la conversación');
  btnMas.appendChild(icono('mas-horizontal', true));
  btnMas.addEventListener('click', (e) => {
    e.stopPropagation();
    opts.onAcciones(btnMas.getBoundingClientRect());
  });
  const accionesMas = el('div', 'acciones-mas');
  accionesMas.setAttribute('aria-label', 'acciones de la conversación');

  // [089A-2] Toggle del panel derecho (espejo del de la lista): siempre
  // visible en el principal, a la izquierda del ⋯ (derecha del título).
  let btnToggleDer: HTMLButtonElement | null = null;
  if (!lateral && opts.onTogglePanelDerecho) {
    btnToggleDer = el('button', 'cab-boton') as HTMLButtonElement;
    btnToggleDer.id = `${opts.idPrefijo}-alternar-panel-derecho`;
    btnToggleDer.type = 'button';
    btnToggleDer.title = 'mostrar panel derecho';
    btnToggleDer.setAttribute('aria-label', 'mostrar panel derecho');
    btnToggleDer.appendChild(icono('panel-der-abrir'));
    btnToggleDer.addEventListener('click', () => opts.onTogglePanelDerecho?.());
    accionesMas.appendChild(btnToggleDer);
  }
  accionesMas.appendChild(btnMas);

  // ---- título (editado inline al renombrar) ----
  const t = el('span', 'titulo');
  t.textContent = titulo;
  t.title = titulo;

  let editando = false;
  function ponerTitulo(texto: string): void {
    titulo = texto;
    if (!editando) {
      t.textContent = texto;
      t.title = texto;
    }
  }

  function empezarRenombrar(
    valor: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void {
    if (editando) return;
    editando = true;
    const input = el('input') as HTMLInputElement;
    input.type = 'text';
    input.className = 'titulo-renombrar';
    input.value = valor;
    input.setAttribute('aria-label', 'renombrar conversación');
    cab.replaceChild(input, t);

    const fin = (guardar: boolean) => {
      if (!editando) return;
      editando = false;
      const nuevo = input.value.trim();
      if (guardar && nuevo && nuevo !== titulo) {
        titulo = nuevo;
        onGuardar(nuevo);
      } else if (!guardar) {
        onCancelar?.();
      }
      t.textContent = titulo;
      t.title = titulo;
      if (input.parentNode === cab) cab.replaceChild(t, input);
    };
    input.addEventListener('keydown', (e) => {
      e.stopPropagation();
      if (e.key === 'Enter') fin(true);
      else if (e.key === 'Escape') fin(false);
    });
    input.addEventListener('blur', () => fin(true));
    input.focus();
    input.select();
  }

  cab.appendChild(acciones);
  cab.appendChild(t);
  cab.appendChild(accionesMas);

  // [089A-1] Controles de ventana propios estilo Paseo, solo en el panel
  // principal bajo Tauri (en web siguen los nativos del navegador). La
  // cabecera actúa como barra de arrastre (`data-tauri-drag-region`) y la
  // botonera minimizar/maximizar/cerrar queda al extremo derecho, tras ⋯.
  // No aplica a paneles laterales (son chats, no chrome de ventana).
  if (!lateral && esEntornoTauri()) {
    hacerArrastrable(cab);
    cab.appendChild(crearControlesVentana());
  }

  return {
    raiz: cab,
    ponerTitulo(texto: string) {
      ponerTitulo(texto);
    },
    setSidebarAbierta(abierta: boolean) {
      if (btnToggle) {
        // [089A-2] Visible siempre: el icono refleja el estado (cerrar para
        // ocultar, abrir para mostrar). Ya no se usa `hidden`.
        btnToggle.hidden = false;
        btnToggle.replaceChildren(icono(abierta ? 'panel-izq-cerrar' : 'panel-izq-abrir'));
        const label = abierta
          ? 'ocultar lista de conversaciones'
          : 'mostrar lista de conversaciones';
        btnToggle.title = label;
        btnToggle.setAttribute('aria-label', label);
      }
    },
    setPanelDerechoAbierto(abierto: boolean) {
      if (btnToggleDer) {
        btnToggleDer.replaceChildren(icono(abierto ? 'panel-der-cerrar' : 'panel-der-abrir'));
        const label = abierto ? 'ocultar panel derecho' : 'mostrar panel derecho';
        btnToggleDer.title = label;
        btnToggleDer.setAttribute('aria-label', label);
      }
    },
    empezarRenombrar(valor, onGuardar, onCancelar) {
      empezarRenombrar(valor, onGuardar, onCancelar);
    },
  };
}
