/* Bloques de razonamiento, herramienta, aviso y aprobación de `mensajes`.
 * Los builders vivos los usa la simulación; los cerrados, el historial. */
import type {
  DecisionAprobacion,
  EstadoHerramienta,
  IconoNombre,
  ResultadoHerramienta,
} from '../dominio/tipos';
import { icono, ponerIcono } from './iconos';
import { el } from '../util/dom';
import { aplicarResultado, ponerHtmlSeguro } from './mensajesUtil';

// ---------- Razonamiento ----------

export interface RazonamientoVivo {
  raiz: HTMLElement;
  nodoResultado: HTMLElement;
  nodoMeta: HTMLElement;
  /** Quita el spinner y fija el meta final (p. ej. '1.4 s'). */
  terminar(meta: string): void;
}

function baseRazonamiento(abierto: boolean): RazonamientoVivo {
  const raiz = el('details', 'razonamiento');
  if (abierto) raiz.open = true;
  const sum = el('summary');
  sum.appendChild(icono('cerebro'));
  const texto = el('span', 'texto');
  texto.textContent = 'Razonando';
  sum.appendChild(texto);
  const nodoMeta = el('span', 'meta');
  sum.appendChild(nodoMeta);
  const nodoResultado = el('div', 'resultado');
  raiz.appendChild(sum);
  raiz.appendChild(nodoResultado);
  return {
    raiz,
    nodoResultado,
    nodoMeta,
    terminar(meta: string) {
      nodoMeta.textContent = meta;
    },
  };
}

/** Razonamiento cerrado del historial (meta fijo, p. ej. '1.6 s'). */
export function crearRazonamientoCerrado(texto: string, meta: string): HTMLElement {
  const r = baseRazonamiento(false);
  r.nodoResultado.textContent = texto;
  r.nodoMeta.textContent = meta;
  return r.raiz;
}

/** Razonamiento vivo: abierto con spinner mientras el texto fluye. */
export function crearRazonamientoVivo(): RazonamientoVivo {
  const r = baseRazonamiento(true);
  ponerIcono(r.nodoMeta, 'spin', true, 'ic-spin');
  return r;
}

// ---------- Herramienta ----------

export interface HerramientaViva {
  raiz: HTMLElement;
  nodoMeta: HTMLElement;
  nodoResultado: HTMLElement;
  ejecutando(): void;
  completada(meta: string, resultado: ResultadoHerramienta): void;
  errored(meta: string, resultado: ResultadoHerramienta): void;
}

function baseHerramienta(iconoNombre: IconoNombre, titulo: string): HerramientaViva {
  const raiz = el('details', 'herramienta');
  const sum = el('summary');
  sum.appendChild(icono(iconoNombre));
  const texto = el('span', 'texto');
  texto.textContent = titulo;
  sum.appendChild(texto);
  const nodoMeta = el('span', 'meta');
  sum.appendChild(nodoMeta);
  const nodoResultado = el('div', 'resultado');
  raiz.appendChild(sum);
  raiz.appendChild(nodoResultado);

  function limpiarMeta(): void {
    raiz.classList.remove('ejecutando', 'error');
  }

  return {
    raiz,
    nodoMeta,
    nodoResultado,
    ejecutando() {
      raiz.classList.add('ejecutando');
      ponerIcono(nodoMeta, 'spin', true, 'ic-spin');
    },
    completada(meta: string, resultado: ResultadoHerramienta) {
      limpiarMeta();
      nodoMeta.textContent = meta;
      aplicarResultado(nodoResultado, resultado);
    },
    errored(_meta: string, resultado: ResultadoHerramienta) {
      limpiarMeta();
      // el error se distingue por su icono de fallo en el meta
      ponerIcono(nodoMeta, 'x-circulo', true);
      aplicarResultado(nodoResultado, resultado);
    },
  };
}

/** Render estático según el estado (historial). */
export function crearHerramienta(opts: {
  icono: IconoNombre;
  titulo: string;
  estado: EstadoHerramienta;
}): HTMLElement {
  const h = baseHerramienta(opts.icono, opts.titulo);
  const e = opts.estado;
  if (e.estado === 'ejecutando') h.ejecutando();
  else if (e.estado === 'completada') h.completada(e.meta, e.resultado);
  else h.errored(e.meta, e.resultado);
  return h.raiz;
}

/** Herramienta mutable para la simulación (ejecutando → completada). */
export function crearHerramientaViva(iconoNombre: IconoNombre, titulo: string): HerramientaViva {
  return baseHerramienta(iconoNombre, titulo);
}

// ---------- Aviso de sistema ----------

/**
 * Opción de acción de un aviso ([039A-3 P3]): botón en la fila del aviso
 * (p. ej. "restaurar archivos") que dispara `onClick` y cierra el aviso
 * (el resultado de la acción se muestra en un aviso nuevo, no aquí).
 */
export interface AccionAviso {
  texto: string;
  onClick: () => void;
}

export function crearAvisoSistema(texto: string, meta: string, detalle: string, accion?: AccionAviso): HTMLElement {
  const raiz = el('details', 'aviso-sistema');
  const sum = el('summary');
  sum.appendChild(icono('terminal', true));
  const t = el('span', 'texto');
  t.textContent = texto;
  sum.appendChild(t);
  if (accion) {
    const b = el('button', 'aviso-accion') as HTMLButtonElement;
    b.type = 'button';
    b.textContent = accion.texto;
    b.addEventListener('click', (e) => {
      e.preventDefault();
      e.stopPropagation();
      accion.onClick();
    });
    sum.appendChild(b);
  }
  const m = el('span', 'meta');
  m.textContent = meta;
  sum.appendChild(m);
  const det = el('div', 'resultado');
  det.textContent = detalle;
  raiz.appendChild(sum);
  raiz.appendChild(det);
  return raiz;
}

// ---------- Tarjeta de aprobación ----------

export interface TarjetaAprobacionViva {
  raiz: HTMLElement;
  nodoEstado: HTMLElement;
  nodoArgs: HTMLElement;
  /** Texto del estado (p. ej. 'aprobada · ejecutando…'). */
  ponerEstado(texto: string): void;
  /** HTML de args (diff con .del/.add). */
  ponerArgsHtml(html: string): void;
  quitarAcciones(): void;
  marcarDenegada(): void;
}

/** Construye la tarjeta y enruta los 3 botones a onDecidir. */
export function crearTarjetaAprobacion(opts: {
  titulo: string;
  argsTexto: string;
  onDecidir: (decision: DecisionAprobacion, tarjeta: TarjetaAprobacionViva) => void;
}): TarjetaAprobacionViva {
  const raiz = el('div', 'aprobacion');

  const cab = el('div', 'cab');
  cab.appendChild(icono('lapiz', true));
  const nombre = el('span', 'nombre');
  nombre.textContent = opts.titulo;
  cab.appendChild(nombre);
  const estado = el('span', 'estado');
  estado.textContent = 'requiere aprobación · política ask';
  cab.appendChild(estado);

  const args = el('div', 'args');
  args.textContent = opts.argsTexto;

  const acciones = el('div', 'acciones');
  const bAprobar = el('button', 'btn primario');
  bAprobar.type = 'button';
  bAprobar.textContent = 'Aprobar';
  const bPermitir = el('button', 'btn');
  bPermitir.type = 'button';
  bPermitir.textContent = 'Permitir siempre';
  const bDenegar = el('button', 'btn');
  bDenegar.type = 'button';
  bDenegar.textContent = 'Denegar';
  acciones.appendChild(bAprobar);
  acciones.appendChild(bPermitir);
  acciones.appendChild(bDenegar);

  raiz.appendChild(cab);
  raiz.appendChild(args);
  raiz.appendChild(acciones);

  const tarjeta: TarjetaAprobacionViva = {
    raiz,
    nodoEstado: estado,
    nodoArgs: args,
    ponerEstado(texto: string) {
      estado.textContent = texto;
    },
    ponerArgsHtml(html: string) {
      ponerHtmlSeguro(args, html);
    },
    quitarAcciones() {
      acciones.remove();
    },
    marcarDenegada() {
      raiz.classList.add('denegada');
    },
  };

  bAprobar.onclick = () => opts.onDecidir('aprobar', tarjeta);
  bPermitir.onclick = () => opts.onDecidir('permitir', tarjeta);
  bDenegar.onclick = () => opts.onDecidir('denegar', tarjeta);

  return tarjeta;
}
