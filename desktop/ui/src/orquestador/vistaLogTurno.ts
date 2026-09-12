/* Visor del log de un turno [129A-4 F4]: overlay de solo lectura con los
 * eventos persistidos por el backend (`log_turno`). Reutiliza la base de
 * diálogo `.modal-fondo`/`.modal` (modal.css); las clases `log-turno-*` son
 * propias (estilos/logTurno.css). Cada fila es un `<details>` con hora, tipo
 * y JSON íntegro; "copiar" deja el JSON del evento en el portapapeles. */

import { cuerpo, el } from '../util/dom';
import { icono } from '../componentes/iconos';
import type { EventoTurnoLog } from '../tauri/real';

function resumenDe(payload: string): string {
  try {
    const v = JSON.parse(payload) as Record<string, unknown>;
    const utiles = ['decision', 'motivo', 'pregunta', 'ack', 'tool', 'peticion_id'];
    for (const k of utiles) {
      const x = v[k];
      if (typeof x === 'string' && x.trim().length > 0) return x.slice(0, 140);
    }
  } catch {
    /* payload no-JSON: se muestra tal cual recortado */
  }
  return payload.slice(0, 140);
}

function filaEvento(ev: EventoTurnoLog): HTMLElement {
  const raiz = el('details', 'log-turno-evento');
  const cab = el('summary', 'log-turno-cab');
  const hora = el('span', 'log-turno-hora');
  const ms = Date.parse(ev.creado_en);
  hora.textContent = Number.isFinite(ms) ? new Date(ms).toLocaleTimeString() : ev.creado_en;
  const tipo = el('span', 'log-turno-tipo');
  tipo.textContent = ev.tipo;
  const resumen = el('span', 'log-turno-resumen');
  resumen.textContent = resumenDe(ev.payload_json);
  cab.append(hora, tipo, resumen);
  const cuerpo = el('div', 'log-turno-cuerpo');
  const pre = el('pre', 'log-turno-json');
  try {
    pre.textContent = JSON.stringify(JSON.parse(ev.payload_json), null, 2);
  } catch {
    pre.textContent = ev.payload_json;
  }
  const bCopiar = el('button', 'log-turno-copiar') as HTMLButtonElement;
  bCopiar.type = 'button';
  bCopiar.title = 'copiar JSON del evento';
  bCopiar.setAttribute('aria-label', 'copiar JSON del evento');
  bCopiar.appendChild(icono('copiar', true));
  bCopiar.addEventListener('click', (e) => {
    e.stopPropagation();
    void navigator.clipboard?.writeText(pre.textContent ?? '').catch(() => {});
  });
  cuerpo.append(bCopiar, pre);
  raiz.append(cab, cuerpo);
  return raiz;
}

export function mostrarLogTurno(turnoId: string, eventos: EventoTurnoLog[]): void {
  const fondo = el('div', 'modal-fondo');
  fondo.id = 'log-turno-fondo';
  const modal = el('div', 'modal log-turno-modal');
  modal.setAttribute('role', 'dialog');
  modal.setAttribute('aria-modal', 'true');
  modal.setAttribute('aria-label', 'log del turno');

  const titulo = el('div', 'log-turno-titulo');
  titulo.appendChild(icono('terminal', true));
  const tTexto = el('span', '');
  tTexto.textContent = `log del turno · ${eventos.length} evento${eventos.length === 1 ? '' : 's'}`;
  titulo.appendChild(tTexto);
  const bCerrar = el('button', 'log-turno-cerrar') as HTMLButtonElement;
  bCerrar.type = 'button';
  bCerrar.title = 'cerrar log';
  bCerrar.setAttribute('aria-label', 'cerrar log');
  bCerrar.textContent = '×';
  bCerrar.addEventListener('click', cerrar);
  titulo.appendChild(bCerrar);
  modal.appendChild(titulo);

  const aviso = el('div', 'log-turno-aviso');
  aviso.textContent = `turno ${turnoId}`;
  aviso.title = turnoId;
  modal.appendChild(aviso);

  const lista = el('div', 'log-turno-lista');
  for (const ev of eventos) lista.appendChild(filaEvento(ev));
  modal.appendChild(lista);
  fondo.appendChild(modal);
  cuerpo().appendChild(fondo);

  function cerrar(): void {
    fondo.remove();
    document.removeEventListener('keydown', alTeclado);
  }
  function alTeclado(e: KeyboardEvent): void {
    if (e.key === 'Escape') cerrar();
  }
  fondo.addEventListener('click', (e) => {
    if (e.target === fondo) cerrar();
  });
  document.addEventListener('keydown', alTeclado);
}
