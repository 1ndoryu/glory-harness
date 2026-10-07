// Visor de la tab Consola: pinta el comando, la salida, el stdin y el pie
// de la entrada activa (ver `panelConsola.ts`: solo pinta, sin estado).

import { el } from '../util/dom';
import type { EntradaConsola } from './consolaModelo';
import { duenoTexto, estadoTexto } from './consolaModelo';

export interface NodosVisorConsola {
  visor: HTMLElement;
  vacio: HTMLElement;
  visorComando: HTMLElement;
  visorSalida: HTMLElement;
  stdinFila: HTMLElement;
  visorPie: HTMLElement;
}

export function pintarVisorConsola(
  nodos: NodosVisorConsola,
  e: EntradaConsola | null,
  puedeEscribir: boolean,
): void {
  const hay = e !== null;
  // `hidden` + regla CSS (sin estilo inline: regla cssInlineScript).
  nodos.visor.hidden = !hay;
  nodos.vacio.hidden = hay;
  if (!e) return;
  nodos.visorComando.textContent = '';
  nodos.visorComando.title = e.comando;
  const prompt = el('span', 'consola-prompt');
  prompt.textContent = '$ ';
  const cmdTexto = el('span', 'consola-comando-texto');
  // textContent: el comando verbatim, sin interpretar (puede traer `$`, `<`).
  cmdTexto.textContent = e.comando;
  nodos.visorComando.append(prompt, cmdTexto);
  // Seguir el vivo solo si ya estaba al fondo (no robar el scroll al releer).
  const alFondo = nodos.visorSalida.scrollHeight - nodos.visorSalida.scrollTop - nodos.visorSalida.clientHeight < 40;
  nodos.visorSalida.textContent = '';
  for (const l of e.lineas) {
    const div = el('div', 'consola-linea');
    if (l.flujo === 'stderr') div.classList.add('error');
    // textContent: la salida puede traer HTML/ANSI sin escapar.
    div.textContent = l.texto;
    nodos.visorSalida.appendChild(div);
  }
  if (alFondo) nodos.visorSalida.scrollTop = nodos.visorSalida.scrollHeight;
  // [219A-4] El pie abre con el dueño (el visor mezcla mías + agente).
  const partes = [duenoTexto(e), estadoTexto(e)];
  if (e.duracionMs !== null) partes.push(`${(e.duracionMs / 1000).toFixed(1)} s`);
  if (e.truncada) partes.push('salida truncada por el backend');
  if (e.descartadas > 0) partes.push(`${e.descartadas} líneas antiguas fuera de vista`);
  if (e.cargando) partes.push('cargando historial…');
  nodos.visorPie.textContent = partes.join(' · ');
  // `hidden` + regla CSS (sin estilo inline: regla cssInlineScript).
  nodos.stdinFila.hidden = e.estado !== 'viva' || !puedeEscribir;
}
