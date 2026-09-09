// Núcleo de mensajes (split 089A-16 F2-resto): constructores de nodos de
// mensaje de usuario/asistente, pie de turno y render estático por tipo
// (historial). Los bloques reutilizables viven en `mensajesBloques.ts` y
// las utilidades en `mensajesUtil.ts`; `mensajes.ts` re-exporta todo.

import type { Bloque } from '../dominio/tipos';
import { icono } from './iconos';
import { el } from '../util/dom';
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
