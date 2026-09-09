/* Acciones del panel de chat sobre sus mensajes: avisos de sistema, copia de
 * tramos al portapapeles, edición de mensajes de usuario y menú contextual
 * por mensaje (Editar / Volver a este punto / Copiar). */
import type { Entrada } from './entrada';
import { crearAvisoSistema } from './mensajes';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
} from './menu';
import { copiarAlPortapapeles } from '../util/portapapeles';

export interface AccionesDeps {
  mensajes: HTMLElement;
  entrada: Entrada;
  /** M1: true si cualquier panel tiene turno en curso. */
  hayTurnoGlobal(): boolean;
  /** Texto original de un mensaje de usuario (lo aporta el historial). */
  getTextoUsuario(id: string): string | undefined;
  /** "Volver a este punto" (lo aporta el módulo de carga; getter perezoso). */
  getVolverA(): (id: string) => Promise<void>;
  /** Vacía mensajes + mapa + cambios (lo aporta el historial). */
  limpiarHistorial(): void;
}

export interface AccionesChat {
  avisoChat(texto: string, meta: string, detalle: string): void;
  limpiarChat(): void;
  copiarUltimoTramo(): void;
  abrirAccionesMensaje(id: string, rect: DOMRect): void;
}

export function crearAcciones(deps: AccionesDeps): AccionesChat {
  const { mensajes, entrada } = deps;

  // ---------- helpers de aviso / copia ----------
  function avisoChat(texto: string, meta: string, detalle: string): void {
    mensajes.appendChild(crearAvisoSistema(texto, meta, detalle));
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  function limpiarChat(): void {
    deps.limpiarHistorial();
    // Se descarta una edición pendiente (su mensaje ya no está visible).
    entrada.cancelarEnEdicion();
  }

  function tramoParaCopiar(): string | null {
    const hijos = Array.from(mensajes.children);
    const ultimoIndice = (pred: (n: Element) => boolean): number => {
      for (let i = hijos.length - 1; i >= 0; i--) {
        if (pred(hijos[i])) return i;
      }
      return -1;
    };
    const ultimoUser = ultimoIndice((n) => n.classList.contains('msg-user'));
    if (ultimoUser < 0) return null;
    const ultimoAsis = ultimoIndice((n) => n.classList.contains('msg-asis'));
    const fin = ultimoAsis >= ultimoUser ? ultimoAsis : ultimoUser;
    const texto = hijos
      .slice(ultimoUser, fin + 1)
      .map((n) => {
        if (n.classList.contains('pie-turno')) return '';
        return (n.textContent ?? '').trim();
      })
      .filter((s) => s.length > 0)
      .join('\n\n');
    return texto || null;
  }

  function copiarUltimoTramo(): void {
    const texto = tramoParaCopiar();
    if (!texto) {
      avisoChat('no hay mensajes que copiar', '', '');
      return;
    }
    void copiarAlPortapapeles(texto)
      .then(() => avisoChat('tramo copiado al portapapeles', 'copiar', ''))
      .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
  }

  function tramoDesdeId(id: string): string | null {
    const nodo = mensajes.querySelector<HTMLElement>(`.msg-user[data-id="${CSS.escape(id)}"]`);
    if (!nodo) return null;
    const hijos = Array.from(mensajes.children);
    const i = hijos.indexOf(nodo);
    if (i < 0) return null;
    const partes: string[] = [];
    for (let j = i; j < hijos.length; j++) {
      const n = hijos[j];
      if (j > i && n.classList.contains('msg-user')) break;
      if (n.classList.contains('pie-turno')) continue;
      const t = (n.textContent ?? '').trim();
      if (t) partes.push(t);
    }
    return partes.length ? partes.join('\n\n') : null;
  }

  function copiarTramoMensaje(id: string): void {
    const texto = tramoDesdeId(id);
    if (!texto) {
      avisoChat('no hay texto que copiar', '', '');
      return;
    }
    void copiarAlPortapapeles(texto)
      .then(() => avisoChat('mensaje copiado al portapapeles', 'copiar', ''))
      .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
  }

  // ---------- acciones por mensaje (editar / volver / copiar) ----------
  function empezarEdicion(id: string): void {
    if (deps.hayTurnoGlobal()) {
      avisoChat('termina el turno antes de editar un mensaje', '', '');
      return;
    }
    const texto = deps.getTextoUsuario(id);
    if (texto === undefined) {
      avisoChat('el mensaje ya no está en esta conversación', '', '');
      return;
    }
    entrada.ponerEnEdicion(id, texto);
  }

  function abrirAccionesMensaje(id: string, rect: DOMRect): void {
    abrirMenuContextual({
      rect,
      construir(m) {
        m.appendChild(
          crearItemMenu({
            texto: 'Editar',
            onClick() {
              cerrarMenuActual();
              empezarEdicion(id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Volver a este punto',
            onClick() {
              cerrarMenuActual();
              void deps.getVolverA()(id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Copiar',
            onClick() {
              cerrarMenuActual();
              copiarTramoMensaje(id);
            },
          }),
        );
      },
    });
  }

  return { avisoChat, limpiarChat, copiarUltimoTramo, abrirAccionesMensaje };
}
