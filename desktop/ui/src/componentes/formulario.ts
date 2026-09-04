// ============================================================
// Renderizador genérico de formularios a partir del esquema
// centralizado (dominio/opciones.ts). Sin cabecera ni pie:
// cada control aplica su cambio al vuelo (guardado automático).
// ============================================================

import type { GrupoOpciones, OpcionControl } from '../dominio/opciones';
import { icono } from './iconos';
import { el } from '../util/dom';

export interface FormularioApi {
  raiz: HTMLElement;
  /** Lee el valor actual de una opción por id. */
  obtenerValor(id: string): string | boolean | undefined;
  /** Actualiza el valor mostrado de una opción por id. */
  asignarValor(id: string, valor: string | boolean): void;
  /**
   * Sustituye el control de una opción por uno externo (p. ej. el selector
   * de modelo). La opción queda exenta de reconstrucción en asignarValor.
   */
  reemplazarControl(id: string, nodoNuevo: HTMLElement): void;
}

export interface FormularioOpciones {
  grupos: GrupoOpciones[];
  /** Se invoca al cambiar cualquier opción (guardado automático). */
  onCambio: (id: string, valor: string | boolean) => void;
}

/** Control de "sí/no" con check (monocromo). */
function controlBooleano(op: OpcionControl, alCambio: (v: boolean) => void): HTMLElement {
  const caja = el('span', 'check-box');
  caja.title = op.valor ? 'activado' : 'desactivado';
  if (op.valor) caja.appendChild(icono('check'));
  caja.addEventListener('click', () => {
    const nuevo = !op.valor;
    op.valor = nuevo;
    caja.title = nuevo ? 'activado' : 'desactivado';
    if (nuevo) caja.appendChild(icono('check'));
    else caja.innerHTML = '';
    alCambio(nuevo);
  });
  return caja;
}

/** Selector segmentado monocromo (predeterminado/autonomo, etc.). */
function controlSegmentado(
  op: OpcionControl,
  alCambio: (v: string) => void,
): HTMLElement {
  const toggle = el('div', 'toggle');
  (op.opciones ?? []).forEach((o) => {
    const opt = el('span', 'opt' + (o.valor === op.valor ? ' sel' : ''));
    opt.textContent = o.etiqueta;
    opt.addEventListener('click', () => {
      op.valor = o.valor;
      toggle.querySelectorAll('.opt').forEach((x) => x.classList.remove('sel'));
      opt.classList.add('sel');
      alCambio(o.valor);
    });
    toggle.appendChild(opt);
  });
  return toggle;
}

/** Selector desplegable nativo estilizado para conjuntos de opciones. */
function controlSeleccion(
  op: OpcionControl,
  alCambio: (v: string) => void,
): HTMLElement {
  const sel = el('select', 'select') as HTMLSelectElement;
  (op.opciones ?? []).forEach((o) => {
    const opt = el('option') as HTMLOptionElement;
    opt.value = o.valor;
    opt.textContent = o.etiqueta;
    if (o.valor === op.valor) opt.selected = true;
    sel.appendChild(opt);
  });
  sel.addEventListener('change', () => {
    op.valor = sel.value;
    alCambio(sel.value);
  });
  return sel;
}

/** Entrada de texto (guardado al soltar foco / Enter). */
function controlTexto(op: OpcionControl, alCambio: (v: string) => void): HTMLElement {
  const input = el('input', 'input-texto') as HTMLInputElement;
  input.type = 'text';
  input.value = String(op.valor);
  input.placeholder = op.placeholder ?? '';
  input.spellcheck = false;
  const aplicar = () => alCambio(input.value);
  input.addEventListener('change', aplicar); // al perder foco / Enter
  return input;
}

/** Fila lectura (etiqueta izq. valor der., estilo persistencia). */
function controlLectura(op: OpcionControl): HTMLElement {
  const fila = el('div', 'fila-dato');
  const s1 = el('span');
  s1.textContent = op.etiqueta;
  const s2 = el('span');
  s2.textContent = String(op.valor);
  fila.appendChild(s1);
  fila.appendChild(s2);
  return fila;
}

/** Valor de solo lectura en caja con borde (estilo modelo/claves del mockup). */
function controlValor(op: OpcionControl): HTMLElement {
  const valor = el('div', 'valor');
  const texto = el('span', 'texto');
  texto.textContent = String(op.valor);
  valor.appendChild(texto);
  return valor;
}

/** Construye un control según el tipo declarado en la opción. */
function construirControl(
  op: OpcionControl,
  alCambio: (id: string, valor: string | boolean) => void,
): HTMLElement {
  switch (op.tipo) {
    case 'booleano':
      return controlBooleano(op, (v) => alCambio(op.id, v));
    case 'segmentado':
      return controlSegmentado(op, (v) => alCambio(op.id, v));
    case 'seleccion':
      return controlSeleccion(op, (v) => alCambio(op.id, v));
    case 'texto':
      return controlTexto(op, (v) => alCambio(op.id, v));
    case 'valor':
      return controlValor(op);
    case 'lectura':
      return controlLectura(op);
    default:
      return controlLectura(op);
  }
}

export function montarFormulario(opts: FormularioOpciones): FormularioApi {
  const raiz = el('div', 'formulario');
  const controlesPorId = new Map<string, { op: OpcionControl; nodo: HTMLElement }>();
  // Opciones cuyo control se sustituyó externamente (reemplazarControl):
  // asignarValor no debe reconstruir el control, solo actualizar el valor.
  const exentas = new Set<string>();

  /** Fila en línea (etiqueta izq. + control/valor der.), estilo persistencia del mockup. */
  function crearFila(op: OpcionControl, control: HTMLElement): HTMLElement {
    const fila = el('div', 'fila-dato');
    const s1 = el('span');
    s1.textContent = op.etiqueta;
    fila.appendChild(s1);
    control.classList.add('al-final');
    fila.appendChild(control);
    return fila;
  }

  opts.grupos.forEach((grupo: GrupoOpciones) => {
    const sec = el('section', 'grupo');
    const tit = el('div', 'grupo-titulo');
    tit.textContent = grupo.titulo;
    sec.appendChild(tit);

    grupo.opciones.forEach((op: OpcionControl) => {
      // lectura y booleano se muestran como fila-dato en línea (como en el mockup)
      if (op.tipo === 'lectura' || op.tipo === 'booleano') {
        const fila =
          op.tipo === 'lectura'
            ? controlLectura(op)
            : crearFila(op, controlBooleano(op, (v) => opts.onCambio(op.id, v)));
        fila.dataset.opcion = op.id;
        sec.appendChild(fila);
        controlesPorId.set(op.id, { op, nodo: fila });
        return;
      }

      const campo = el('div', 'campo');
      const etiq = el('span', 'etiqueta');
      etiq.textContent = op.etiqueta;
      campo.appendChild(etiq);
      const nodo = construirControl(op, opts.onCambio);
      nodo.dataset.opcion = op.id;
      campo.appendChild(nodo);
      if (op.nota) {
        const nota = el('span', 'nota');
        nota.textContent = op.nota;
        campo.appendChild(nota);
      }
      sec.appendChild(campo);
      controlesPorId.set(op.id, { op, nodo });
    });

    raiz.appendChild(sec);
  });

  return {
    raiz,
    obtenerValor(id: string) {
      return controlesPorId.get(id)?.op.valor;
    },
    asignarValor(id: string, valor: string | boolean) {
      const entrada = controlesPorId.get(id);
      if (!entrada) return;
      entrada.op.valor = valor;
      // Control reemplazado externamente (selector de modelo): no se reconstruye.
      if (exentas.has(id)) return;
      const nodo = entrada.nodo;
      const reemplazo = construirControl(entrada.op, opts.onCambio);
      reemplazo.dataset.opcion = id;
      // según la forma montada se refresca el control o se reconstruye la fila
      const contenedor =
        nodo.classList.contains('fila-dato') || nodo.closest('.fila-dato');
      if (contenedor) {
        const fila = nodo.classList.contains('fila-dato')
          ? (nodo as HTMLElement)
          : (nodo.closest('.fila-dato') as HTMLElement);
        const control = reemplazo;
        control.classList.add('al-final');
        fila.replaceChild(control, fila.querySelector('.al-final') ?? nodo);
        controlesPorId.set(id, { op: entrada.op, nodo: control });
      } else {
        const campo = nodo.closest('.campo') ?? nodo.parentElement;
        if (campo) {
          campo.replaceChild(reemplazo, nodo);
          controlesPorId.set(id, { op: entrada.op, nodo: reemplazo });
        }
      }
    },
    reemplazarControl(id: string, nodoNuevo: HTMLElement) {
      const entrada = controlesPorId.get(id);
      if (!entrada) return;
      nodoNuevo.dataset.opcion = id;
      entrada.nodo.replaceWith(nodoNuevo);
      controlesPorId.set(id, { op: entrada.op, nodo: nodoNuevo });
      exentas.add(id);
    },
  };
}
