/* Indicador circular de contexto de la entrada: arco SVG según
 * `ocupacion_pct` + menú hover con el detalle de la ventana. */
import type { EstadoContexto } from './entrada';
import { cuerpo, el } from '../util/dom';
import {
  alDesenfocar,
  alDesplazar,
  alRedimensionar,
  altoVentana,
  anchoVentana,
  finDesenfocar,
  finDesplazar,
  finRedimension,
} from '../plataforma/ventana';

export interface IndicadorContexto {
  indicador: HTMLButtonElement;
  pintarContexto(estado: EstadoContexto): void;
}

export function crearIndicadorContexto(): IndicadorContexto {
  // [039A-3 P6] Indicador circular de contexto, justo antes del botón enviar.
  // Círculo sin relleno (stroke) cuya circunferencia se rellena según el
  // `ocupacion_pct` del `ContextoDetalle` (fuente única, decisión §2.10).
  // Estética monocromo: solo trazo, sin relleno.
  const indicador = el('button', 'ctx-indicador') as HTMLButtonElement;
  indicador.type = 'button';
  indicador.setAttribute('aria-label', 'contexto');
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  const radio = 9;
  const perimetro = 2 * Math.PI * radio;
  const circuloFondo = document.createElementNS(NS, 'circle');
  circuloFondo.setAttribute('cx', '12');
  circuloFondo.setAttribute('cy', '12');
  circuloFondo.setAttribute('r', String(radio));
  circuloFondo.setAttribute('class', 'ctx-pista');
  const circulo = document.createElementNS(NS, 'circle');
  circulo.setAttribute('cx', '12');
  circulo.setAttribute('cy', '12');
  circulo.setAttribute('r', String(radio));
  circulo.setAttribute('class', 'ctx-lleno');
  // Rotado -90° para empezar arriba; dasharray = perimetro * pct.
  circulo.setAttribute('transform', 'rotate(-90 12 12)');
  svg.appendChild(circuloFondo);
  svg.appendChild(circulo);
  indicador.appendChild(svg);

  // [039A-3 P6+] Estado completo de contexto (fuente única para el arco y el
  // menú hover). El `title` nativo se omite: el detalle lo da el menú custom.
  let estadoCtx: EstadoContexto = {
    pct: null,
    maxVentana: null,
    reservaSalida: null,
    totalEntrada: null,
  };
  let detalleCtx: HTMLElement | null = null;

  function pintarContexto(estado: EstadoContexto): void {
    estadoCtx = estado;
    // `pctF` (0-100) es la fuente del arco; sin dato queda 0 (círculo vacío).
    const pctF = estado.pct === null ? 0 : Math.min(100, Math.max(0, estado.pct));
    const off = perimetro * (1 - pctF / 100);
    circulo.setAttribute('stroke-dasharray', `${perimetro} ${perimetro}`);
    circulo.setAttribute('stroke-dashoffset', String(off));
    // Rótulo accesible: "0%" o "N%" (sin datos → total configurado si hay).
    const rotulo =
      estado.pct === null
        ? estado.maxVentana !== null
          ? `contexto · hasta ${Math.round(estado.maxVentana / 1000)}k`
          : 'contexto'
        : `contexto ${Math.round(estado.pct)}%`;
    indicador.setAttribute('aria-label', rotulo);
    // Si el detalle está visible (cursor sobre el círculo), refrescar datos.
    if (detalleCtx) {
      const d = detalleCtx;
      while (d.firstChild) d.removeChild(d.firstChild);
      construirDetalle(d);
      posicionarDetalle(d, indicador.getBoundingClientRect());
    }
  }

  /** Número exacto con separador de miles (es). */
  function numeroExacto(n: number): string {
    return Number.isFinite(n) ? String(Math.round(n).toLocaleString('es')) : '—';
  }

  /** Filas etiqueta/valor del uso de la ventana (contenido del hover). */
  function filasDetalleContexto(): Array<[string, string]> {
    const filas: Array<[string, string]> = [];
    if (estadoCtx.pct === null) {
      filas.push(
        estadoCtx.maxVentana !== null
          ? ['configurada', numeroExacto(estadoCtx.maxVentana)]
          : ['uso', 'sin datos'],
      );
      return filas;
    }
    if (estadoCtx.maxVentana !== null) {
      const efectiva = Math.max(0, estadoCtx.maxVentana - (estadoCtx.reservaSalida ?? 0));
      const usados = Math.round((estadoCtx.pct / 100) * efectiva);
      filas.push([
        'usados',
        `${numeroExacto(usados)} de ${numeroExacto(estadoCtx.maxVentana)} (${Math.round(estadoCtx.pct)}%)`,
      ]);
    } else {
      filas.push(['usados', `${Math.round(estadoCtx.pct)}%`]);
    }
    if (estadoCtx.reservaSalida !== null) {
      filas.push(['reserva de salida', numeroExacto(estadoCtx.reservaSalida)]);
    }
    if (estadoCtx.totalEntrada !== null) {
      filas.push(['entrada del turno', numeroExacto(estadoCtx.totalEntrada)]);
    }
    return filas;
  }

  /** Rellena el contenido del detalle (título + filas etiqueta/valor). */
  function construirDetalle(d: HTMLElement): void {
    const titulo = el('div', 'ctx-detalle-titulo');
    titulo.textContent = 'ventana de contexto';
    d.appendChild(titulo);
    for (const [etiqueta, valor] of filasDetalleContexto()) {
      const fila = el('div', 'ctx-detalle-fila');
      const e = el('span', 'etiqueta');
      e.textContent = etiqueta;
      const v = el('span', 'valor');
      v.textContent = valor;
      fila.appendChild(e);
      fila.appendChild(v);
      d.appendChild(fila);
    }
  }

  /** Posiciona el detalle junto al indicador, con vuelco al viewport. */
  function posicionarDetalle(d: HTMLElement, rect: DOMRect): void {
    const margen = 8;
    const vw = anchoVentana();
    const vh = altoVentana();
    const altura = d.offsetHeight;
    const ancho = d.offsetWidth;
    const espacioAbajo = vh - rect.bottom - margen;
    let top =
      espacioAbajo < altura && rect.top > altura + margen ? rect.top - altura - 4 : rect.bottom + 4;
    if (top < margen) top = margen;
    if (top + altura > vh - margen) top = vh - altura - margen;
    // Posición medida (no de diseño): viaja como variable y la hoja la aplica.
    d.style.setProperty('--ctx-detalle-top', `${top}px`);
    let left = rect.left;
    if (left + ancho > vw - margen) left = Math.max(margen, vw - ancho - margen);
    d.style.setProperty('--ctx-detalle-left', `${left}px`);
  }

  function alClicFueraDetalle(): void {
    cerrarDetalle();
  }
  function alTeclaDetalle(e: KeyboardEvent): void {
    if (e.key === 'Escape') cerrarDetalle();
  }
  function alCambioLayoutDetalle(): void {
    cerrarDetalle();
  }

  function cerrarDetalle(): void {
    if (!detalleCtx) return;
    detalleCtx.remove();
    detalleCtx = null;
    document.removeEventListener('click', alClicFueraDetalle, true);
    document.removeEventListener('keydown', alTeclaDetalle, true);
    finRedimension(alCambioLayoutDetalle);
    finDesplazar(alCambioLayoutDetalle);
    finDesenfocar(alCambioLayoutDetalle);
  }

  /** Abre el pequeño menú de detalle bajo el círculo (hover/enfoque). */
  function abrirDetalle(): void {
    cerrarDetalle();
    // Nace oculto por la hoja (`visibility`, para poder medirlo) y `.visible`
    // lo destapa al final, ya colocado.
    const d = el('div', 'ctx-detalle');
    construirDetalle(d);
    cuerpo().appendChild(d);
    posicionarDetalle(d, indicador.getBoundingClientRect());
    detalleCtx = d;
    document.addEventListener('click', alClicFueraDetalle, true);
    document.addEventListener('keydown', alTeclaDetalle, true);
    alRedimensionar(alCambioLayoutDetalle);
    alDesplazar(alCambioLayoutDetalle);
    alDesenfocar(alCambioLayoutDetalle);
    d.classList.add('visible');
  }

  pintarContexto({ pct: null, maxVentana: null, reservaSalida: null, totalEntrada: null });
  // El detalle es informativo: se abre con el cursor encima del círculo (o con
  // el foco por teclado) y se cierra al salir. El clic no enfoca el chat.
  indicador.addEventListener('mouseenter', () => abrirDetalle());
  indicador.addEventListener('mouseleave', () => cerrarDetalle());
  indicador.addEventListener('focus', () => abrirDetalle());
  indicador.addEventListener('blur', () => cerrarDetalle());
  indicador.addEventListener('click', (e) => e.stopPropagation());

  return { indicador, pintarContexto };
}
