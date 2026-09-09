import '../estilos/toast.css';
import { el } from '../util/dom';

export interface ToastGlobal {
  raiz: HTMLElement;
  mostrar(texto: string, detalle?: string): void;
}

export function montarToastGlobal(): ToastGlobal {
  const raiz = el('div', 'toast-global');
  raiz.setAttribute('aria-live', 'polite');
  raiz.setAttribute('aria-atomic', 'true');

  let secuencia = 0;
  let timer: number | null = null;

  function mostrar(texto: string, detalle = ''): void {
    const id = ++secuencia;
    raiz.replaceChildren();
    const aviso = el('div', 'toast-item');
    const mensaje = el('div', 'toast-mensaje');
    mensaje.textContent = texto;
    aviso.appendChild(mensaje);
    if (detalle.trim()) {
      const detalleNodo = el('div', 'toast-detalle');
      detalleNodo.textContent = detalle;
      aviso.appendChild(detalleNodo);
    }
    raiz.appendChild(aviso);
    if (timer !== null) window.clearTimeout(timer);
    timer = window.setTimeout(() => {
      if (id !== secuencia) return;
      raiz.replaceChildren();
      timer = null;
    }, 5000);
  }

  return { raiz, mostrar };
}
