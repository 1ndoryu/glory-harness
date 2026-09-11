/* Barras y botón del compositor de entrada: barra "editando mensaje…"
 * del modo edición, badge del elemento del navegador seleccionado,
 * textarea autoexpandible (máx 5 líneas) y botón único enviar/detener. */
import type { ElementoSeleccionado } from '../dominio/tipos';
import { ponerIcono } from './iconos';
import { el } from '../util/dom';

export interface BarrasEntradaDeps {
  idPrefijo: string;
  caja: HTMLElement;
  textarea: HTMLTextAreaElement;
}

export interface BarrasEntrada {
  btnEnviar: HTMLButtonElement;
  /** Recalcula la altura del textarea (máx 5 líneas). */
  ajustarEntrada(): void;
  /** Icono/título del botón según esté corriendo un turno. */
  pintarBotonEnviar(corriendo: boolean): void;
  /** [039A-3 P2] Devuelve el id en edición y limpia el modo (para `enviar`). */
  tomarEdicionId(): string | null;
  ponerEnEdicion(id: string, texto: string): void;
  cancelarEnEdicion(): void;
  enEdicion(): boolean;
  edicionId(): string | null;
  /** [seleccionar] Antepone el descriptor del adjunto al texto. */
  textoConAdjunto(texto: string): string;
  quitarAdjunto(): void;
  adjuntarElemento(elem: ElementoSeleccionado): void;
  getElementoPendiente(): ElementoSeleccionado | null;
}

export function crearBarrasEntrada(deps: BarrasEntradaDeps): BarrasEntrada {
  const { caja, textarea } = deps;

  // botón único: enviar / detener (en completa es el 4º control de la barra;
  // en mínima es el único hijo de .controles, anclado a la derecha).
  const btnEnviar = el('button', 'btn-enviar') as HTMLButtonElement;
  btnEnviar.id = `${deps.idPrefijo}-btn-enviar`;
  btnEnviar.type = 'button';

  // ---------- [039A-3 P2] modo edición de mensaje ----------
  // Al editar un mensaje de usuario del historial, el textarea rellena su
  // texto y se muestra una barra "editando mensaje… [cancelar]". Al enviar
  // en este modo, main.ts hace rewind(editar=true) + reenvío (P2 §2.5).
  let edicion: { id: string } | null = null;

  const barraEdicion = el('div', 'editando-msg');
  barraEdicion.hidden = true;
  const edicionTexto = el('span', 'editando-msg-texto');
  const btnCancelarEdicion = el('button', 'editando-msg-cancelar') as HTMLButtonElement;
  btnCancelarEdicion.type = 'button';
  btnCancelarEdicion.textContent = 'cancelar';
  btnCancelarEdicion.title = 'cancelar edición';
  barraEdicion.appendChild(edicionTexto);
  barraEdicion.appendChild(btnCancelarEdicion);
  // La barra va entre el textarea y los controles.
  caja.insertBefore(barraEdicion, textarea);

  function pintarBarraEdicion(): void {
    if (edicion) {
      edicionTexto.textContent = 'editando mensaje';
      barraEdicion.hidden = false;
    } else {
      barraEdicion.hidden = true;
    }
  }

  function cancelarEnEdicion(): void {
    if (!edicion) return;
    edicion = null;
    pintarBarraEdicion();
    textarea.focus();
  }

  btnCancelarEdicion.addEventListener('click', (e) => {
    e.stopPropagation();
    cancelarEnEdicion();
  });

  function tomarEdicionId(): string | null {
    const id = edicion ? edicion.id : null;
    edicion = null;
    pintarBarraEdicion();
    return id;
  }

  function ponerEnEdicion(id: string, texto: string): void {
    edicion = { id };
    textarea.value = texto;
    ajustarEntrada();
    pintarBarraEdicion();
    textarea.focus();
  }

  // ---------- [seleccionar] badge de elemento del navegador ----------
  // El usuario eligió un elemento de la página (hover+clic en el panel
  // navegador). Se muestra como badge sobre el textarea y, al enviar, se
  // antepone al texto un descriptor para que el modelo lo reciba y pueda
  // actuar sobre él (p. ej. con la tool `navegador_reflejo`).
  let adjunto: ElementoSeleccionado | null = null;

  const barraAdjunto = el('div', 'adjunto-badge');
  barraAdjunto.hidden = true;
  const adjuntoInfo = el('span', 'adjunto-badge-info');
  const btnQuitarAdjunto = el('button', 'adjunto-badge-quitar') as HTMLButtonElement;
  btnQuitarAdjunto.type = 'button';
  btnQuitarAdjunto.textContent = 'quitar';
  btnQuitarAdjunto.title = 'quitar elemento seleccionado';
  barraAdjunto.appendChild(adjuntoInfo);
  barraAdjunto.appendChild(btnQuitarAdjunto);
  // El badge va sobre el textarea (primera fila de la caja del compositor).
  caja.insertBefore(barraAdjunto, caja.firstChild);

  function pintarAdjunto(): void {
    if (!adjunto) {
      barraAdjunto.hidden = true;
      return;
    }
    const textoRecorte = adjunto.texto ? ` · “${adjunto.texto.slice(0, 40)}”` : '';
    adjuntoInfo.textContent = `elemento: ${adjunto.etiqueta}${textoRecorte}`;
    barraAdjunto.title = `selector: ${adjunto.selector}\npágina: ${adjunto.pagina}`;
    barraAdjunto.hidden = false;
  }

  function quitarAdjunto(): void {
    adjunto = null;
    pintarAdjunto();
  }

  btnQuitarAdjunto.addEventListener('click', (e) => {
    e.stopPropagation();
    quitarAdjunto();
  });

  /** Al enviar, antepone el descriptor del elemento adjunto al texto. */
  function textoConAdjunto(texto: string): string {
    if (!adjunto) return texto;
    const base = adjunto.texto ? adjunto.texto.trim().slice(0, 200) : '';
    const contexto = base ? ` ("${base}")` : '';
    const descriptor =
      `[elemento de la página ${adjunto.pagina} — selector CSS: ${adjunto.selector}` +
      ` — etiqueta: ${adjunto.etiqueta}${contexto}]`;
    return `${descriptor}\n\n${texto}`;
  }

  function adjuntarElemento(elem: ElementoSeleccionado): void {
    adjunto = elem;
    pintarAdjunto();
    textarea.focus();
  }

  // ---------- textarea autoexpandible (máx 5 líneas) ----------
  // El alto nace de una medida (`scrollHeight`), no del diseño: se publica como
  // variable y lo aplica `entrada.css`. El tope se calcula aquí porque depende
  // del `line-height` ya resuelto por el navegador.
  function ajustarEntrada(): void {
    textarea.style.setProperty('--entrada-alto', 'auto');
    const lh = getComputedStyle(textarea).lineHeight;
    const linea = lh === 'normal' ? 18 : parseFloat(lh);
    const max = linea * 5;
    const alto = textarea.scrollHeight > max ? max : textarea.scrollHeight;
    textarea.style.setProperty('--entrada-alto', `${alto}px`);
  }
  textarea.addEventListener('input', ajustarEntrada);
  ajustarEntrada();

  // ---------- botón enviar/detener ----------
  function pintarBotonEnviar(corriendo: boolean): void {
    if (corriendo) {
      ponerIcono(btnEnviar, 'detener', true);
      btnEnviar.title = 'detener';
      btnEnviar.setAttribute('aria-label', 'detener');
    } else {
      ponerIcono(btnEnviar, 'flecha-arriba', true);
      btnEnviar.title = 'enviar';
      btnEnviar.setAttribute('aria-label', 'enviar');
    }
  }
  pintarBotonEnviar(false);

  return {
    btnEnviar,
    ajustarEntrada,
    pintarBotonEnviar,
    tomarEdicionId,
    ponerEnEdicion,
    cancelarEnEdicion,
    enEdicion() {
      return edicion !== null;
    },
    edicionId() {
      return edicion ? edicion.id : null;
    },
    textoConAdjunto,
    quitarAdjunto,
    adjuntarElemento,
    getElementoPendiente() {
      return adjunto;
    },
  };
}
