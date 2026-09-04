// ============================================================
// Render de bloques de #mensajes: usuario, asistente (con cursor
// vivo), razonamiento, herramienta (estados), aviso y aprobación.
// Port 1:1 del mockup, separando el markup reutilizable para que
// la capa de simulación mute nodos concretos (streaming sin
// re-render), igual que hacía el boceto.
// ============================================================

import type {
  Bloque,
  DecisionAprobacion,
  EstadoHerramienta,
  IconoNombre,
  ResultadoHerramienta,
} from '../dominio/tipos';
import { icono, iconoHtml, spinnerHtml } from './iconos';
import { el } from '../util/dom';

// ---------- Aplicar un resultado al cuadro (.resultado) ----------

function aplicarResultado(nodo: HTMLElement, r: ResultadoHerramienta): void {
  if (r.tipo === 'html') nodo.innerHTML = r.html;
  else nodo.textContent = r.texto;
}

// ---------- Mensajes ----------

export function crearMensajeUsuario(texto: string): HTMLElement {
  const m = el('div', 'msg-user');
  const inner = el('div');
  inner.textContent = texto;
  m.appendChild(inner);
  return m;
}

export function crearMensajeAsistente(texto: string): HTMLElement {
  const m = el('div', 'msg-asis');
  const txt = el('div', 'texto');
  txt.textContent = texto;
  m.appendChild(txt);
  return m;
}

export interface AsistenteVivo {
  raiz: HTMLElement;
  nodo: HTMLElement;
  cursor: HTMLElement;
}

/** Mensaje de asistente con cursor de streaming (para la simulación). */
export function crearMensajeAsistenteVivo(textoInicial: string): AsistenteVivo {
  const raiz = el('div', 'msg-asis');
  const nodo = el('div', 'texto');
  nodo.textContent = textoInicial;
  const cursor = el('span', 'cursor');
  nodo.appendChild(cursor);
  raiz.appendChild(nodo);
  return { raiz, nodo, cursor };
}

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
  r.nodoMeta.innerHTML = spinnerHtml(true);
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
      nodoMeta.innerHTML = spinnerHtml(true);
    },
    completada(meta: string, resultado: ResultadoHerramienta) {
      limpiarMeta();
      nodoMeta.textContent = meta;
      aplicarResultado(nodoResultado, resultado);
    },
    errored(_meta: string, resultado: ResultadoHerramienta) {
      limpiarMeta();
      // el error se distingue por su icono de fallo en el meta
      nodoMeta.innerHTML = iconoHtml('x-circulo', true);
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

export function crearAvisoSistema(texto: string, meta: string, detalle: string): HTMLElement {
  const raiz = el('details', 'aviso-sistema');
  const sum = el('summary');
  sum.appendChild(icono('terminal', true));
  const t = el('span', 'texto');
  t.textContent = texto;
  sum.appendChild(t);
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
      args.innerHTML = html;
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

// ---------- Render estático por tipo (historial) ----------

/** Renderiza un bloque del historial a un nodo listo para #mensajes. */
export function renderizarBloque(bloque: Bloque): HTMLElement {
  switch (bloque.tipo) {
    case 'usuario':
      return crearMensajeUsuario(bloque.texto);
    case 'asistente':
      return crearMensajeAsistente(bloque.texto);
    case 'razonamiento':
      if (bloque.meta === null) {
        const r = crearRazonamientoVivo();
        r.nodoResultado.textContent = bloque.texto;
        r.terminar('1.6 s');
        return r.raiz;
      }
      return crearRazonamientoCerrado(bloque.texto, bloque.meta);
    case 'herramienta':
      return crearHerramienta({
        icono: bloque.icono,
        titulo: bloque.titulo,
        estado: bloque.detalle,
      });
    case 'aviso':
      return crearAvisoSistema(bloque.texto, bloque.meta, bloque.detalle);
    case 'aprobacion':
      // Las tarjetas de aprobación solo existen en runtime (simulación):
      // nunca forman parte del historial estático de ejemplo.
      throw new Error('aprobacion no se renderiza desde historial');
  }
}
