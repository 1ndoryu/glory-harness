// Eventos del turno hacia la tab Consola: altas, chunks en vivo y fines.
// Mutación del store por contexto explícito (ver `panelConsola.ts`); el
// repintado y el refresco acotado vuelven por callbacks.

import type { EntradaConsola, EventoConsola } from './consolaModelo';
import { acotarLineas, nuevaEntradaConsola } from './consolaModelo';
import type { LineaConsola } from './consolaModelo';

export interface CtxEventoConsola {
  entradas: Map<string, EntradaConsola>;
  orden: string[];
  getActiva(): string | null;
  setActiva(id: string | null): void;
  repintar(): void;
  programarRefresco(): void;
}

export function manejarEventoConsola(ctx: CtxEventoConsola, ev: EventoConsola): void {
  const { entradas, orden } = ctx;
  if (ev.tipo === 'consola_inicio') {
    if (!entradas.has(ev.id_ejecucion)) {
      orden.push(ev.id_ejecucion);
    }
    entradas.set(ev.id_ejecucion, nuevaEntradaConsola(ev.id_ejecucion, {
      comando: ev.comando,
      conversacionId: ev.conversacion_id,
      // Lo que nace de un evento del turno lo abrió el agente.
      origen: 'agente',
      // Nace del vivo: el historial en vivo es el completo (sin backfill).
      transcript: true,
    }));
    // La última en arrancar toma el visor (el usuario puede cambiarla).
    ctx.setActiva(ev.id_ejecucion);
  } else if (ev.tipo === 'consola_chunk') {
    const e = entradas.get(ev.id_ejecucion);
    // Chunk sin inicio (p. ej. tab abierta a mitad de turno): se crea la
    // entrada sin comando antes que perder salida en silencio.
    const entrada = e ?? (() => {
      const creada: EntradaConsola = nuevaEntradaConsola(ev.id_ejecucion, {
        comando: '(comando desconocido: la consola arrancó antes de abrir la tab)',
        // Un chunk del turno solo lo emite una consola del agente.
        origen: 'agente',
        // Nace de un chunk en vivo: lo que llegue es el historial (el
        // comando real lo trae `sincronizar` si el backend la retiene).
        transcript: true,
      });
      entradas.set(ev.id_ejecucion, creada);
      orden.push(ev.id_ejecucion);
      if (ctx.getActiva() === null) ctx.setActiva(ev.id_ejecucion);
      return creada;
    })();
    const linea: LineaConsola = { flujo: ev.flujo, texto: ev.linea };
    // En plena carga de backfill, el vivo espera en `pendientes` y se
    // fusiona al resolver (sin duplicar lo ya volcado).
    if (entrada.cargando) {
      entrada.pendientes.push(linea);
      acotarLineas(entrada, entrada.pendientes);
    } else {
      entrada.lineas.push(linea);
      acotarLineas(entrada, entrada.lineas);
    }
    if (entrada.estado !== 'viva') entrada.estado = 'viva';
  } else {
    const e = entradas.get(ev.id_ejecucion);
    if (!e) return;
    // `codigo null` = matada o desacoplada que terminó fuera de vista: se
    // congela como fin sin código (el backend no distingue el motivo aquí).
    e.estado = 'fin';
    e.codigo = ev.codigo;
    e.truncada = ev.truncada;
    e.duracionMs = ev.duracion_ms;
  }
  ctx.repintar();
  // [219A-5 F1] Actividad nueva: el refresco acotado sigue a las vivas.
  ctx.programarRefresco();
}
