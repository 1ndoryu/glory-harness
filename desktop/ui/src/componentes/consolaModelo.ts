// Modelo de la tab Consola: tipos, factoría de entradas y ayudantes puros.
// Extraído de `panelConsola.ts` para no rebasar el techo de líneas;
// `panelConsola.ts` conserva el montaje DOM y la orquestación, `consolaLista.ts`
// la sub-barra, `consolaVisor.ts` el visor y `consolaEventos.ts` los eventos.

import type { AgenteEvento, InfoConsolaLista, NuevaConsola, TranscriptConsola } from '../tauri/realTipos';

/** Subconjunto del contrato que alimenta la tab (fiel a `evento.rs`). */
export type EventoConsola = Extract<
  AgenteEvento,
  { tipo: 'consola_inicio' | 'consola_chunk' | 'consola_fin' }
>;

export type EstadoConsola = 'viva' | 'fin' | 'matada';

export interface LineaConsola {
  flujo: 'stdout' | 'stderr';
  texto: string;
}

/** [ISP] Identidad de la entrada: quién la abrió y de qué turno viene. */
export interface IdentidadConsola {
  id: string;
  comando: string;
  conversacionId: string;
  /** [219A-4] Dueño: `agente` = la abrió el modelo con `comando`;
   * `usuario` = shell propia abierta con [+ Nueva] (sin jaula). */
  origen: 'agente' | 'usuario';
}

/** [ISP] Vista de la entrada: salida visible y estado de terminación. */
export interface VistaConsola {
  lineas: LineaConsola[];
  /** Líneas caídas por el tope de vista (el backend acota a 128 KB). */
  descartadas: number;
  estado: EstadoConsola;
  codigo: number | null;
  truncada: boolean;
  duracionMs: number | null;
}

/** [ISP] Backfill de la entrada: historial del backend en curso o listo.
 * [219A-3] Con `cargando`, los chunks van a `pendientes` y se fusionan al
 * resolver (sin duplicar lo que ya pinta la vista). */
export interface BackfillConsola {
  /** Backfill ya cargado (o innecesario: historial en vivo completo). */
  transcript: boolean;
  cargando: boolean;
  pendientes: LineaConsola[];
}

/** Entrada completa: identidad + vista + backfill. */
export interface EntradaConsola extends IdentidadConsola, VistaConsola, BackfillConsola {}

/** Líneas por entrada en la vista (el transcript completo vive en el
 * backend; la vista no es archivo: pasado el tope se descartan las más
 * antiguas y se cuenta cuántas). */
export const MAX_LINEAS_VISTA = 1000;

/** Factoría con defaults de entrada viva sin historial; cada origen
 * sobreescribe lo suyo (evita repetir el literal en 4 sitios). */
export function nuevaEntradaConsola(
  id: string,
  base: Pick<IdentidadConsola, 'comando' | 'origen'> & Partial<Omit<EntradaConsola, 'id' | 'comando' | 'origen'>>,
): EntradaConsola {
  return {
    id,
    conversacionId: '',
    lineas: [],
    descartadas: 0,
    estado: 'viva',
    codigo: null,
    truncada: false,
    duracionMs: null,
    transcript: false,
    cargando: false,
    pendientes: [],
    ...base,
  };
}

export function idCorto(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}

export function estadoTexto(e: EntradaConsola): string {
  if (e.estado === 'viva') return '● corriendo';
  if (e.estado === 'matada') return '✕ matada';
  return e.codigo === 0 ? `✓ fin (${e.codigo})` : `✓ fin (${e.codigo ?? '?'})`;
}

/** [219A-4] Etiqueta de dueño para la fila y el pie del visor. */
export function duenoTexto(e: EntradaConsola): string {
  return e.origen === 'usuario' ? 'mía' : 'agente';
}

export function contarVivas(entradas: Map<string, EntradaConsola>): number {
  let n = 0;
  for (const e of entradas.values()) if (e.estado === 'viva') n += 1;
  return n;
}

/** Acota un depósito de líneas al tope de vista (cuenta las caídas). */
export function acotarLineas(e: EntradaConsola, deposito: LineaConsola[]): void {
  if (deposito.length > MAX_LINEAS_VISTA) {
    const sobran = deposito.length - MAX_LINEAS_VISTA;
    deposito.splice(0, sobran);
    e.descartadas += sobran;
  }
}

export interface PanelConsola {
  raiz: HTMLElement;
  manejarEvento(ev: EventoConsola): void;
  /** Selecciona la entrada (sin abrir la tab: eso lo hace el orquestador). */
  revelar(id: string): void;
  hayVivas(): boolean;
  /** [219A-3] Backfill: vivas + recientes del backend (el orquestador lo
   * llama al abrir la tab). No roba la selección ni duplica el vivo. */
  sincronizar(): Promise<void>;
}

export interface OpcionesPanelConsola {
  onError?: (texto: string, detalle?: string) => void;
  /** [219A-5 F4] Aviso informativo (toast): resultado de matar (`true` =
   * matada, `false` = ya había terminado). */
  onInfo?: (texto: string) => void;
  /** [209A-1 F4-resto] El usuario pulsó × sobre una consola viva: el
   * orquestador la mata en el backend. Devuelve si la mató (`false` = ya
   * había terminado). El `consola_fin` posterior (o el refresco acotado F1)
   * la congela como terminada en la vista.
   * [219A-5 F4] × por fila: ya no solo la activa. */
  onMatar?: (idEjecucion: string) => Promise<boolean> | void;
  /** [219A-3] Puentes al backend (los cablea el orquestador a la sesión):
   * lista para la sub-barra, transcript por entrada, stdin de una viva.
   * Ausentes = panel solo-en-vivo (como hasta 209A-1).
   * [219A-4] `onCrear` abre una consola PROPIA (shell del SO, sin jaula). */
  onSincronizar?: () => Promise<InfoConsolaLista[]>;
  onLeerSalida?: (idEjecucion: string) => Promise<TranscriptConsola>;
  onEscribir?: (idEjecucion: string, texto: string) => Promise<number>;
  onCrear?: () => Promise<NuevaConsola>;
}
