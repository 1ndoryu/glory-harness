// Sub-barra de la tab Consola: pinta la lista de entradas con su marca de
// estado, dueño, comando y meta (ver `panelConsola.ts`: solo pinta, la
// orquestación —selección y matar— vuelve por callbacks).

import { el } from '../util/dom';
import type { EntradaConsola } from './consolaModelo';
import { duenoTexto, estadoTexto, idCorto } from './consolaModelo';

export interface CtxListaConsola {
  lista: HTMLElement;
  orden: string[];
  entradas: Map<string, EntradaConsola>;
  activaId: string | null;
  puedeMatar: boolean;
  alElegir(id: string): void;
  alMatar(id: string): void;
}

export function pintarListaConsola(ctx: CtxListaConsola): void {
  ctx.lista.textContent = '';
  for (const id of ctx.orden) {
    const e = ctx.entradas.get(id);
    if (!e) continue;
    const fila = el('button', 'consola-fila') as HTMLButtonElement;
    fila.type = 'button';
    fila.setAttribute('role', 'listitem');
    if (id === ctx.activaId) fila.classList.add('activa');
    const marca = el('span', `consola-marca consola-${e.estado}`);
    marca.textContent = e.estado === 'viva' ? '●' : e.estado === 'matada' ? '✕' : '✓';
    // [219A-4] Dueño de la entrada (la barra interna mezcla mías + agente).
    const dueno = el('span', 'consola-dueno');
    dueno.textContent = duenoTexto(e);
    dueno.title = e.origen === 'usuario'
      ? 'Consola propia: la abriste tú con + (shell, sin jaula)'
      : 'Consola del agente: la abrió el modelo con comando (con jaula)';
    const cmd = el('span', 'consola-fila-comando');
    cmd.textContent = e.comando;
    cmd.title = e.comando;
    const meta = el('span', 'consola-fila-meta');
    // KB de salida real (suma de líneas en vista), no del comando.
    const bytesSalida = e.lineas.reduce((n, l) => n + l.texto.length, 0);
    const kb = Math.round(bytesSalida / 1024);
    meta.textContent = `${idCorto(id)} · ${estadoTexto(e)} · ${e.lineas.length} líneas${kb > 0 ? ` · ${kb} KB` : ''}`;
    fila.append(marca, dueno, cmd, meta);
    // [219A-5 F4] × por fila (solo vivas): mata esa entrada sin pasar
    // por la activa. `span` con rol botón (un `button` no puede anidarse
    // en el `button` de la fila); frena la propagación para no cambiar
    // la activa al matar.
    if (e.estado === 'viva' && ctx.puedeMatar) {
      const matar = el('span', 'consola-matar') as HTMLSpanElement;
      matar.textContent = '✕';
      matar.title = 'Matar esta consola';
      matar.setAttribute('role', 'button');
      matar.tabIndex = 0;
      matar.setAttribute('aria-label', `Matar la consola ${e.comando}`);
      matar.addEventListener('click', (ev) => {
        ev.stopPropagation();
        ctx.alMatar(id);
      });
      matar.addEventListener('keydown', (ev) => {
        if (ev.key === 'Enter' || ev.key === ' ') {
          ev.preventDefault();
          ev.stopPropagation();
          ctx.alMatar(id);
        }
      });
      fila.appendChild(matar);
    }
    fila.addEventListener('click', () => {
      ctx.alElegir(id);
    });
    ctx.lista.appendChild(fila);
  }
}
