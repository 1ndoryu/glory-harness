// Acciones de la tab Consola contra el backend: abrir consola propia y
// backfill de vivas + recientes (ver `panelConsola.ts`). Estado por contexto
// explícito; el repintado y los puentes al backend vuelven por callbacks.

import type { InfoConsolaLista, NuevaConsola } from '../tauri/realTipos';
import type { EntradaConsola } from './consolaModelo';
import { nuevaEntradaConsola } from './consolaModelo';

export interface CtxRemotoConsola {
  entradas: Map<string, EntradaConsola>;
  orden: string[];
  getActiva(): string | null;
  setActiva(id: string | null): void;
  repintar(): void;
  cargarTranscript(id: string): void;
  recargarSalida(id: string): void;
  programarRefresco(): void;
  setNuevaHabilitada(habilitada: boolean): void;
  onCrear?: () => Promise<NuevaConsola>;
  onSincronizar?: () => Promise<InfoConsolaLista[]>;
  onError?: (texto: string, detalle?: string) => void;
}

/** [219A-4] [+ Nueva]: abre la shell propia, la ancla como activa y carga
 * su transcript (vacío al nacer). El backend responde el id + etiqueta. */
export function crearPropiaConsola(ctx: CtxRemotoConsola): void {
  if (ctx.onCrear === undefined) return;
  ctx.setNuevaHabilitada(false);
  void ctx.onCrear().then((n) => {
    if (!ctx.entradas.has(n.id_ejecucion)) ctx.orden.push(n.id_ejecucion);
    ctx.entradas.set(n.id_ejecucion, nuevaEntradaConsola(n.id_ejecucion, {
      comando: n.comando,
      origen: 'usuario',
    }));
    ctx.setActiva(n.id_ejecucion);
    ctx.repintar();
    ctx.cargarTranscript(n.id_ejecucion);
    // [219A-5 F1] La propia nace viva: seguirla hasta el fin.
    ctx.programarRefresco();
  }).catch((err: unknown) => {
    ctx.onError?.('no se pudo abrir la consola propia', String(err));
  }).finally(() => {
    ctx.setNuevaHabilitada(true);
  });
}

/** [219A-3] Backfill de la sub-barra: crea las que faltan (vivas y
 * recientes) y congela las que el backend ya dio por terminadas. No borra:
 * una archivada que el runner ya olvidó sigue visible hasta limpiar. No
 * roba la selección: solo ancla la última si no había ninguna. */
export async function sincronizarConsolas(ctx: CtxRemotoConsola): Promise<void> {
  if (ctx.onSincronizar === undefined) return;
  let remotas: InfoConsolaLista[];
  try {
    remotas = await ctx.onSincronizar();
  } catch (err: unknown) {
    ctx.onError?.('no se pudieron listar las consolas', String(err));
    return;
  }
  for (const c of remotas) {
    const e = ctx.entradas.get(c.id_ejecucion);
    if (!e) {
      ctx.entradas.set(c.id_ejecucion, nuevaEntradaConsola(c.id_ejecucion, {
        comando: c.comando,
        estado: c.viva ? 'viva' : 'fin',
        codigo: c.codigo_salida,
        // [219A-4] El backend es la fuente del dueño.
        origen: c.origen,
      }));
      ctx.orden.push(c.id_ejecucion);
    } else if (!c.viva && e.estado === 'viva') {
      e.estado = 'fin';
      e.codigo = c.codigo_salida ?? e.codigo;
    } else if (e.comando.startsWith('(comando desconocido')) {
      // Entrada rescatada por un chunk sin inicio: el backend sí sabe el
      // comando verbatim (el historial en vivo ya pintado no se toca).
      e.comando = c.comando;
    }
    const actual = ctx.entradas.get(c.id_ejecucion);
    if (actual) actual.origen = c.origen;
  }
  if (ctx.getActiva() === null && ctx.orden.length > 0) {
    ctx.setActiva(ctx.orden[ctx.orden.length - 1]);
  }
  ctx.repintar();
  const activa = ctx.getActiva();
  if (activa) ctx.cargarTranscript(activa);
  // [219A-4] Las propias no emiten chunks (no hay turno que las emita):
  // se refrescan aquí (al abrir la tab) en vez de con polling.
  for (const c of remotas) {
    if (c.viva && c.origen === 'usuario') ctx.recargarSalida(c.id_ejecucion);
  }
  // [219A-5 F1] Si quedan vivas, el refresco acotado las sigue hasta el fin.
  ctx.programarRefresco();
}
