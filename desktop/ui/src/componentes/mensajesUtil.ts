/* Utilidades de render seguro de `mensajes`: escape HTML, formato de
 * resultados de herramienta, inserción HTML saneada y aplicación. */
import type { ResultadoHerramienta } from '../dominio/tipos';
import { el } from '../util/dom';

function escaparHtml(s: string): string {
  return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
}

/** Convierte el resumen y el diff en HTML legible: resumen capitalizado,
 * rótulo con el conteo real y una línea por clase (add/del/ctx) para que el
 * CSS atenúe el contexto y resalte los cambios. */
export function formatearResultadoHerramienta(resumen: string, diff?: string | null): string {
  const texto = resumen.trim();
  const encabezado = texto ? texto[0].toUpperCase() + texto.slice(1) : '';
  if (!diff?.trim()) {
    return `<span class="resumen">${escaparHtml(encabezado)}</span>`;
  }

  let añadidas = 0;
  let eliminadas = 0;
  const cuerpo: string[] = [];
  for (const linea of diff.split('\n')) {
    if (linea.startsWith('@@')) continue; // cabecera de hunk: sin valor para la UI
    let clase = 'ctx';
    let contenido = linea;
    if (linea.startsWith('+')) {
      clase = 'add';
      contenido = linea.slice(1);
      añadidas += 1;
    } else if (linea.startsWith('-')) {
      clase = 'del';
      contenido = linea.slice(1);
      eliminadas += 1;
    } else if (linea.startsWith(' ')) {
      contenido = linea.slice(1);
    } else if (linea.trimStart().startsWith('…')) {
      clase = 'elididas';
    }
    cuerpo.push(`<span class="${clase}">${escaparHtml(contenido)}</span>`);
  }

  if (añadidas + eliminadas === 0) {
    return `<span class="resumen">${escaparHtml(encabezado)}</span>`;
  }
  const conteo = [
    eliminadas > 0 ? `-${eliminadas}` : '',
    añadidas > 0 ? `+${añadidas}` : '',
  ].filter(Boolean).join(' ');
  return [
    `<span class="resumen">${escaparHtml(encabezado)}</span>`,
    `<span class="rotulo-cambios">${conteo}</span>`,
    cuerpo.join(''),
  ].join('\n');
}

// ---------- HTML seguro (sin innerHTML) ----------

/**
 * [089A-16] Inserta el HTML de diffs sin sink innerHTML. Los productores
 * (`formatearResultadoHerramienta`, `diffAuto`, `diffTarjetaHtml`) solo
 * emiten texto escapado + `<span class="…">` + `<br>`; todo lo demás se
 * degrada a texto. Sin atributos salvo `class` saneada, sin eventos.
 */
export function ponerHtmlSeguro(nodo: HTMLElement, html: string): void {
  while (nodo.firstChild) nodo.removeChild(nodo.firstChild);
  const doc = new DOMParser().parseFromString(`<div>${html}</div>`, 'text/html');
  const raiz = doc.body.firstElementChild;
  if (!raiz) {
    nodo.textContent = html;
    return;
  }
  function esClaseSegura(clase: string): boolean {
    return /^[A-Za-z0-9 _-]+$/.test(clase);
  }
  function importar(hijo: ChildNode, destino: Node): void {
    if (hijo.nodeType === 3) {
      destino.appendChild(document.createTextNode(hijo.textContent ?? ''));
      return;
    }
    if (hijo.nodeType !== 1) return;
    const elem = hijo as Element;
    const etiqueta = elem.tagName.toLowerCase();
    if (etiqueta === 'br') {
      destino.appendChild(el('br'));
      return;
    }
    if (etiqueta === 'span') {
      const s = el('span');
      const clase = elem.getAttribute('class') ?? '';
      if (clase && esClaseSegura(clase)) s.className = clase;
      for (const nieto of Array.from(elem.childNodes)) importar(nieto, s);
      destino.appendChild(s);
      return;
    }
    // Fail-closed: cualquier otra etiqueta se degrada a su texto.
    destino.appendChild(document.createTextNode(elem.textContent ?? ''));
  }
  for (const hijo of Array.from(raiz.childNodes)) importar(hijo, nodo);
}

export function aplicarResultado(nodo: HTMLElement, r: ResultadoHerramienta): void {
  if (r.tipo === 'html') ponerHtmlSeguro(nodo, r.html);
  else nodo.textContent = r.texto;
}
