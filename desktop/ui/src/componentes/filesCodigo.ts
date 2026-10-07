// Visor de código del panel Files: pinta el contenido con números de línea
// (ver `panelFiles.ts`). Solo DOM: sin estado ni transporte.

import { el } from '../util/dom';

export function pintarCodigoFiles(codigo: HTMLElement, contenidoArchivo: string): void {
  codigo.replaceChildren();
  const lineas = contenidoArchivo.split('\n');
  if (lineas.length > 2000) {
    const pre = el('pre', 'files-visor-plano');
    pre.textContent = contenidoArchivo;
    codigo.appendChild(pre);
    return;
  }
  const frag = document.createDocumentFragment();
  lineas.forEach((texto, i) => {
    const fila = el('div', 'files-visor-linea');
    const numero = el('span', 'files-visor-numero');
    numero.textContent = String(i + 1);
    const textoNodo = el('span', 'files-visor-texto');
    textoNodo.textContent = texto === '' ? ' ' : texto;
    fila.append(numero, textoNodo);
    frag.appendChild(fila);
  });
  codigo.appendChild(frag);
}
