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

/**
 * [039A-3 P2] Crea un mensaje de usuario. Con `id` (UUID persistido) añade el
 * `data-id` y el botón `⋯` flotante que abre el menú de acciones (editar /
 * volver a este punto / copiar). El menú lo abre `main.ts` vía `onAcciones`.
 */
export function crearMensajeUsuario(
  texto: string,
  id?: string,
  onAcciones?: (id: string, rect: DOMRect) => void,
): HTMLElement {
  const m = el('div', 'msg-user');
  if (id) m.dataset.id = id;
  const inner = el('div');
  inner.textContent = texto;
  m.appendChild(inner);
  if (id && onAcciones) {
    const mas = el('button', 'msg-mas') as HTMLButtonElement;
    mas.type = 'button';
    mas.title = 'acciones del mensaje';
    mas.setAttribute('aria-label', 'acciones del mensaje');
    mas.textContent = '⋯';
    mas.addEventListener('click', (e) => {
      e.stopPropagation();
      onAcciones(id, mas.getBoundingClientRect());
    });
    m.appendChild(mas);
  }
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

/** Datos del pie de turno (P1 039A-3): tokens + modelo + contexto + copiar. */
export interface PieTurno {
  tokensPrompt: number;
  tokensComplecion: number;
  /** Modelo que respondió de verdad (`proveedor/modelo` o `null`). */
  modelo: string | null;
  /** Uso de contexto: 0-100 (% de la ventana efectiva) o `null` si se ignora. */
  ocupacionPct: number | null;
  /** Ventana máxima configurada (p. ej. 150000). `null` si se ignora. */
  maxVentana: number | null;
  /** Reserva de salida (se descuenta de la ventana para el cálculo). */
  reservaSalida: number | null;
  /** Copiar desde el último mensaje de usuario hasta el último assistant. */
  alCopiar: () => void;
}

/** Número corto con sufijo k (redondeo hacia abajo, mínimo 1). */
export function tokensCortos(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return '0';
  if (n < 1000) return String(Math.round(n));
  const k = Math.floor(n / 1000);
  return `${k}k`;
}

/**
 * [039A-3 P1] Pie de turno: bloque estático que cierra cada respuesta del
 * asistente con tokens enviados/recibidos, modelo usado, uso de contexto y
 * un botón Copiar (copia del último mensaje de usuario al último assistant).
 * Se repinta desde `turnos`+mensajes al recargar (misma fuente: tokens reales
 * persistidos por el backend y modelo real tras fallback).
 */
export function crearPieTurno(datos: PieTurno): HTMLElement {
  const raiz = el('div', 'pie-turno');

  const meta = el('div', 'pie-meta');
  const txt = el('span', 'pie-texto');
  const partes: string[] = [];
  partes.push(`${tokensCortos(datos.tokensPrompt)} → ${tokensCortos(datos.tokensComplecion)}`);
  partes.push(datos.modelo ?? 'desconocido');
  if (datos.ocupacionPct !== null && datos.maxVentana !== null) {
    const usados = Math.round((datos.ocupacionPct / 100) * (datos.maxVentana - (datos.reservaSalida ?? 0)));
    partes.push(`${tokensCortos(usados)}/${tokensCortos(datos.maxVentana)} (${Math.round(datos.ocupacionPct)}%)`);
  }
  txt.textContent = partes.join(' · ');
  meta.appendChild(txt);

  const bCopiar = el('button', 'pie-copiar') as HTMLButtonElement;
  bCopiar.type = 'button';
  bCopiar.title = 'copiar del último mensaje del usuario al último del asistente';
  bCopiar.appendChild(icono('copiar', true));
  bCopiar.addEventListener('click', () => datos.alCopiar());

  raiz.appendChild(meta);
  raiz.appendChild(bCopiar);
  return raiz;
}

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
