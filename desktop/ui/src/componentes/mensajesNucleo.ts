// Núcleo de mensajes (split 089A-16 F2-resto): constructores de nodos de
// mensaje de usuario/asistente, pie de turno y render estático por tipo
// (historial). Los bloques reutilizables viven en `mensajesBloques.ts` y
// las utilidades en `mensajesUtil.ts`; `mensajes.ts` re-exporta todo.

import type { Bloque, LogroMetaVisible } from '../dominio/tipos';
import { icono } from './iconos';
import { el } from '../util/dom';
import { formatearDuracion } from '../util/duracion';
import {
  crearAvisoSistema,
  crearHerramienta,
  crearRazonamientoCerrado,
  crearRazonamientoVivo,
} from './mensajesBloques';

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
    mas.appendChild(icono('mas-horizontal', true));
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

/** Aviso "pensando…" que se muestra al enviar, hasta que llega el primer
 * evento con contenido (o el turno falla). El llamador lo retira con
 * `.remove()`; sobre nodo huérfano es no-op. */
export function crearMensajePendiente(): HTMLElement {
  const raiz = el('div', 'msg-asis pendiente');
  const nodo = el('div', 'texto');
  nodo.textContent = 'pensando…';
  raiz.appendChild(nodo);
  return raiz;
}

/** Datos del pie de turno (P1 039A-3): tokens + modelo + contexto. */
export interface PieTurnoDatos {
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
  /** [109A-5 F3] Id del turno que este pie cierra. El evento `meta_lograda`
   * llega entre turnos y ancla el badge al pie de ESE turno, así que el id
   * tiene que estar en el DOM (`data-turno`); sin él no habría dónde
   * colocarlo cuando el logro se declara después de cerrar el turno. */
  turnoId?: string | null;
  /** [129A-2] Velocidad medida del turno (tokens compleción / s de turno).
   * `null`/`undefined` = sin medir (historial recargado): la parte se omite. */
  velocidadTokS?: number | null;
  /** [fix 12-09] Hora del cierre del turno (ms epoch). Visible como `HH:MM`
   * al final del pie; el `title` lleva la fecha exacta. `null`/ausente =
   * turno antiguo sin hora (se omite la parte). */
  creadoEnMs?: number | null;
}

/** Acciones del pie de turno (copiar + ver log). */
export interface PieTurnoAcciones {
  /** Copiar desde el último mensaje de usuario hasta el último assistant. */
  alCopiar: () => void;
  /** [129A-4 F4] Ver el log del turno (eventos persistidos). Ausente = este
   * pie no conoce su turno (recarga sin id): no se pinta el botón. */
  alVerLog?: () => void;
}

/** Datos del pie de turno (P1 039A-3): tokens + modelo + contexto + copiar. */
export interface PieTurno extends PieTurnoDatos, PieTurnoAcciones {}

/**
 * [109A-5 F3] Pinta el badge "meta lograda" en el pie de un turno.
 *
 * Se llama desde el evento (no al crear el pie) porque el logro puede
 * declararse entre turnos: el pie ya existe y solo hay que añadirle el badge.
 * El reloj procede de `elapsed_ms` (neto de pausas, calculado por el dominio)
 * y se OMITE si es 0: sin dato real, el badge dice "Meta lograda" en vez de un
 * `00:00` que parecería una medición.
 */
export function pintarLogroEnPie(pie: HTMLElement, logro: LogroMetaVisible): void {
  const badge = el('span', 'pie-logro');
  badge.title = logro.meta;
  badge.appendChild(icono('meta', true));
  const texto = el('span');
  texto.textContent =
    logro.elapsed_ms > 0
      ? `Meta lograda en ${formatearDuracion(logro.elapsed_ms)}`
      : 'Meta lograda';
  badge.appendChild(texto);
  pie.appendChild(badge);
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
 * asistente con tokens enviados/recibidos, modelo corto (ruta completa en
 * hover), velocidad si se midió, hora de cierre (fecha exacta en hover) y
 * un botón Copiar (copia del último mensaje de usuario al último assistant).
 * Sin medidor de ventana: vive en el indicador circular de la entrada.
 * Se repinta desde `turnos`+mensajes al recargar (misma fuente: tokens reales
 * persistidos por el backend y modelo real tras fallback).
 */
export function crearPieTurno(datos: PieTurno): HTMLElement {
  const raiz = el('div', 'pie-turno');
  // [109A-5 F3] Ancla del badge de meta lograda (ver `pintarLogroEnPie`).
  if (datos.turnoId) raiz.dataset.turno = datos.turnoId;

  const meta = el('div', 'pie-meta');
  const txt = el('span', 'pie-texto');
  const partes: string[] = [];
  // [fix 12-09] Sin tokens ni modelo conocidos (pie de turno antiguo al
  // recargar) el pie muestra solo la hora: `0 → 0 · desconocido` mentía.
  const hayTokens = datos.tokensPrompt > 0 || datos.tokensComplecion > 0;
  if (hayTokens) {
    partes.push(`${tokensCortos(datos.tokensPrompt)} → ${tokensCortos(datos.tokensComplecion)}`);
    if (!datos.modelo) partes.push('desconocido');
  }
  // [129A-2] Velocidad del turno cuando se midió (en vivo, no recarga).
  if (datos.velocidadTokS !== null && datos.velocidadTokS !== undefined && Number.isFinite(datos.velocidadTokS)) {
    partes.push(`${datos.velocidadTokS.toFixed(1)} tok/s`);
  }
  // [fix 12-09] El medidor de ventana (`15k/150k (12%)`) NO va en el pie:
  // vive en el indicador circular de la entrada (`ganchos.ts`).
  if (partes.length > 0) {
    txt.textContent = partes.join(' · ');
    meta.appendChild(txt);
  }
  // [fix 12-09] Modelo corto (último segmento) + ruta completa en hover.
  if (datos.modelo) {
    if (meta.hasChildNodes()) {
      const sepMod = el('span', 'pie-texto');
      sepMod.textContent = ' · ';
      meta.appendChild(sepMod);
    }
    const mod = el('span', 'pie-modelo');
    mod.textContent = datos.modelo.split('/').pop() || datos.modelo;
    mod.title = datos.modelo;
    meta.appendChild(mod);
  }
  // [fix 12-09] Hora visible + fecha exacta en hover (`title` nativo).
  if (datos.creadoEnMs !== null && datos.creadoEnMs !== undefined && Number.isFinite(datos.creadoEnMs)) {
    const fecha = new Date(datos.creadoEnMs);
    if (meta.hasChildNodes()) {
      const sep = el('span', 'pie-texto');
      sep.textContent = ' · ';
      meta.appendChild(sep);
    }
    const hora = el('span', 'pie-hora');
    hora.textContent = fecha.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    hora.title = fecha.toLocaleString([], {
      day: 'numeric',
      month: 'long',
      year: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    });
    meta.appendChild(hora);
  }

  const bCopiar = el('button', 'pie-copiar') as HTMLButtonElement;
  bCopiar.type = 'button';
  bCopiar.title = 'copiar del último mensaje del usuario al último del asistente';
  bCopiar.appendChild(icono('copiar', true));
  bCopiar.addEventListener('click', () => datos.alCopiar());

  raiz.appendChild(meta);
  // [129A-4 F4] "Ver log" solo con turno conocido (el vivo trae `turnoId`;
  // la recarga no lo persiste y no muestra un botón que mentiría).
  if (datos.turnoId && datos.alVerLog) {
    const bLog = el('button', 'pie-log') as HTMLButtonElement;
    bLog.type = 'button';
    bLog.title = 'ver log del turno';
    bLog.setAttribute('aria-label', 'ver log del turno');
    bLog.appendChild(icono('terminal', true));
    bLog.addEventListener('click', () => datos.alVerLog?.());
    raiz.appendChild(bLog);
  }
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
