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
  /** [039A-3 P5] Se invoca al pulsar el botón × de un panel lateral. */
  onCerrar?: () => void;
}

/** Mecánica de renombrar inline compartida (fila de sidebar y cabecera). */
export function montarCabeceraChat(opts: CabeceraChatOpciones): CabeceraChat {
  const cab = el('div', 'cabecera-chat');
  let titulo = opts.titulo;
  const lateral = opts.lateral ?? false;

  // ---- acciones: × cerrar (lateral) + ⋯ ----
  // [089A-3] Los toggles globales (sidebar, panel derecho, visor) se mudaron
  // a la barra superior global (`barraSuperior.ts`): la cabecera queda con el
  // título + ⋯ (y el × en laterales). `acciones` se conserva como contenedor
  // (vacío en el principal) para no cambiar la estructura.
  const acciones = el('div', 'acciones-cabecera');
  acciones.setAttribute('aria-label', 'acciones de la conversación');

  // [089A-3] Sin toggles en la cabecera (mudados a la barra superior):
  // solo el × de cierre en laterales.
  let btnCerrar: HTMLButtonElement | null = null;

  if (lateral) {
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

  // [089A-3] Toggle del panel derecho mudado a la barra superior global.
  // Se conserva el campo (no-op) para no cambiar `CabeceraChatOpciones` ni
  // `panelChat.ts`.
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

  // [089A-3] Arrastre + botonera de ventana mudados a la barra superior
  // global (`barraSuperior.ts`). La cabecera ya no es chrome de ventana.

  return {
    raiz: cab,
    ponerTitulo(texto: string) {
      ponerTitulo(texto);
    },
    // [089A-3] No-op: el toggle de la lista vive en la barra superior
    // global (`barraSuperior.ts`). Se conserva el método para no cambiar
    // la interfaz `CabeceraChat` ni `panelChat.ts`.
    setSidebarAbierta(_abierta: boolean) {
      /* no-op */
    },
    // [089A-3] No-op: el toggle del panel derecho vive en la barra
    // superior global. Se conserva el método por la misma razón.
    setPanelDerechoAbierto(_abierto: boolean) {
      /* no-op */
    },
    empezarRenombrar(valor, onGuardar, onCancelar) {
      empezarRenombrar(valor, onGuardar, onCancelar);
    },
  };
}
