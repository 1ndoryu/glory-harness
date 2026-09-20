// Tab Consola: visor de ejecuciones `comando` en vivo (plan 209A-1 F3).
// Store por `id_ejecucion` + lista y visor. Solo pinta: los eventos llegan
// por `manejarEvento` desde el hook `onConsolaEvento` (un solo `TurnoReal`
// por adaptador, sin duplicados). Sin polling, sin timers, sin PTY.

import '../estilos/consola.css';
import { icono } from './iconos';
import { el } from '../util/dom';
import type { AgenteEvento } from '../tauri/realTipos';

/** Subconjunto del contrato que alimenta la tab (fiel a `evento.rs`). */
export type EventoConsola = Extract<
  AgenteEvento,
  { tipo: 'consola_inicio' | 'consola_chunk' | 'consola_fin' }
>;

type EstadoConsola = 'viva' | 'fin' | 'matada';

interface LineaConsola {
  flujo: 'stdout' | 'stderr';
  texto: string;
}

interface EntradaConsola {
  id: string;
  comando: string;
  conversacionId: string;
  lineas: LineaConsola[];
  /** Líneas caídas por el tope de vista (el backend acota a 128 KB). */
  descartadas: number;
  estado: EstadoConsola;
  codigo: number | null;
  truncada: boolean;
  duracionMs: number | null;
}

/** Líneas por entrada en la vista (el transcript completo vive en el
 * backend; la vista no es archivo: pasado el tope se descartan las más
 * antiguas y se cuenta cuántas). */
const MAX_LINEAS_VISTA = 1000;

function idCorto(id: string): string {
  return id.length > 8 ? id.slice(0, 8) : id;
}

function estadoTexto(e: EntradaConsola): string {
  if (e.estado === 'viva') return '● corriendo';
  if (e.estado === 'matada') return '✕ matada';
  return e.codigo === 0 ? `✓ fin (${e.codigo})` : `✓ fin (${e.codigo ?? '?'})`;
}

export interface PanelConsola {
  raiz: HTMLElement;
  manejarEvento(ev: EventoConsola): void;
  /** Selecciona la entrada (sin abrir la tab: eso lo hace el orquestador). */
  revelar(id: string): void;
  hayVivas(): boolean;
}

export function montarPanelConsola(opts: {
  onError?: (texto: string, detalle?: string) => void;
}): PanelConsola {
  const entradas = new Map<string, EntradaConsola>();
  const orden: string[] = [];
  let activaId: string | null = null;

  const raiz = el('div', 'panel-consola');
  raiz.setAttribute('aria-label', 'consolas de comandos');

  const cabecera = el('div', 'consola-cabecera');
  const titulo = el('span', 'consola-titulo');
  titulo.textContent = 'Consola';
  const contador = el('span', 'consola-contador');
  const btnLimpiar = el('button', 'consola-accion') as HTMLButtonElement;
  btnLimpiar.type = 'button';
  btnLimpiar.title = 'Quitar las terminadas (las vivas siguen corriendo)';
  btnLimpiar.setAttribute('aria-label', 'Quitar consolas terminadas');
  btnLimpiar.appendChild(icono('x'));
  const btnCopiar = el('button', 'consola-accion') as HTMLButtonElement;
  btnCopiar.type = 'button';
  btnCopiar.title = 'Copiar el transcript de la consola activa';
  btnCopiar.setAttribute('aria-label', 'Copiar transcript');
  btnCopiar.appendChild(icono('copiar'));
  cabecera.append(titulo, contador, btnLimpiar, btnCopiar);

  const lista = el('div', 'consola-lista');
  lista.setAttribute('role', 'list');
  const visor = el('div', 'consola-visor');
  const visorComando = el('div', 'consola-comando');
  const visorSalida = el('div', 'consola-salida');
  const visorPie = el('div', 'consola-pie');
  visor.append(visorComando, visorSalida, visorPie);
  const vacio = el('div', 'consola-vacio');
  vacio.textContent = 'Sin ejecuciones todavía: cada `comando` del agente abre aquí su consola en vivo.';
  const nota = el('div', 'consola-nota');
  // [209A-1 límites honestos] Sin stdin ni PTY: visible donde se mira.
  nota.textContent = 'Sin stdin ni PTY: los programas que detectan no-TTY cambian formato (sin color ni barras).';
  raiz.append(cabecera, lista, visor, vacio, nota);

  function vivas(): number {
    let n = 0;
    for (const e of entradas.values()) if (e.estado === 'viva') n += 1;
    return n;
  }

  function pintarContador(): void {
    const n = vivas();
    contador.textContent = n === 0 ? '' : `${n} viva${n === 1 ? '' : 's'}`;
  }

  function pintarLista(): void {
    lista.textContent = '';
    for (const id of orden) {
      const e = entradas.get(id);
      if (!e) continue;
      const fila = el('button', 'consola-fila') as HTMLButtonElement;
      fila.type = 'button';
      fila.setAttribute('role', 'listitem');
      if (id === activaId) fila.classList.add('activa');
      const marca = el('span', `consola-marca consola-${e.estado}`);
      marca.textContent = e.estado === 'viva' ? '●' : e.estado === 'matada' ? '✕' : '✓';
      const cmd = el('span', 'consola-fila-comando');
      cmd.textContent = e.comando;
      cmd.title = e.comando;
      const meta = el('span', 'consola-fila-meta');
      const kb = Math.round(e.comando.length / 1024);
      meta.textContent = `${idCorto(id)} · ${estadoTexto(e)} · ${e.lineas.length} líneas${kb > 0 ? ` · ${kb} KB` : ''}`;
      fila.append(marca, cmd, meta);
      fila.addEventListener('click', () => {
        activaId = id;
        pintarLista();
        pintarVisor();
      });
      lista.appendChild(fila);
    }
  }

  function pintarVisor(): void {
    const e = activaId ? entradas.get(activaId) ?? null : null;
    const hay = e !== null;
    visor.style.display = hay ? '' : 'none';
    vacio.style.display = hay ? 'none' : '';
    if (!e) return;
    visorComando.textContent = '';
    visorComando.title = e.comando;
    const prompt = el('span', 'consola-prompt');
    prompt.textContent = '$ ';
    const cmdTexto = el('span', 'consola-comando-texto');
    // textContent: el comando verbatim, sin interpretar (puede traer `$`, `<`).
    cmdTexto.textContent = e.comando;
    visorComando.append(prompt, cmdTexto);
    // Seguir el vivo solo si ya estaba al fondo (no robar el scroll al releer).
    const alFondo = visorSalida.scrollHeight - visorSalida.scrollTop - visorSalida.clientHeight < 40;
    visorSalida.textContent = '';
    for (const l of e.lineas) {
      const div = el('div', 'consola-linea');
      if (l.flujo === 'stderr') div.classList.add('error');
      // textContent: la salida puede traer HTML/ANSI sin escapar.
      div.textContent = l.texto;
      visorSalida.appendChild(div);
    }
    if (alFondo) visorSalida.scrollTop = visorSalida.scrollHeight;
    const partes = [estadoTexto(e)];
    if (e.duracionMs !== null) partes.push(`${(e.duracionMs / 1000).toFixed(1)} s`);
    if (e.truncada) partes.push('salida truncada por el backend');
    if (e.descartadas > 0) partes.push(`${e.descartadas} líneas antiguas fuera de vista`);
    visorPie.textContent = partes.join(' · ');
  }

  function repintar(): void {
    pintarContador();
    pintarLista();
    pintarVisor();
  }

  function manejarEvento(ev: EventoConsola): void {
    if (ev.tipo === 'consola_inicio') {
      if (!entradas.has(ev.id_ejecucion)) {
        orden.push(ev.id_ejecucion);
      }
      entradas.set(ev.id_ejecucion, {
        id: ev.id_ejecucion,
        comando: ev.comando,
        conversacionId: ev.conversacion_id,
        lineas: [],
        descartadas: 0,
        estado: 'viva',
        codigo: null,
        truncada: false,
        duracionMs: null,
      });
      // La última en arrancar toma el visor (el usuario puede cambiarla).
      activaId = ev.id_ejecucion;
    } else if (ev.tipo === 'consola_chunk') {
      const e = entradas.get(ev.id_ejecucion);
      // Chunk sin inicio (p. ej. tab abierta a mitad de turno): se crea la
      // entrada sin comando antes que perder salida en silencio.
      const entrada = e ?? (() => {
        const creada: EntradaConsola = {
          id: ev.id_ejecucion,
          comando: '(comando desconocido: la consola arrancó antes de abrir la tab)',
          conversacionId: '',
          lineas: [],
          descartadas: 0,
          estado: 'viva',
          codigo: null,
          truncada: false,
          duracionMs: null,
        };
        entradas.set(ev.id_ejecucion, creada);
        orden.push(ev.id_ejecucion);
        if (!activaId) activaId = ev.id_ejecucion;
        return creada;
      })();
      entrada.lineas.push({ flujo: ev.flujo, texto: ev.linea });
      if (entrada.lineas.length > MAX_LINEAS_VISTA) {
        const sobran = entrada.lineas.length - MAX_LINEAS_VISTA;
        entrada.lineas.splice(0, sobran);
        entrada.descartadas += sobran;
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
    repintar();
  }

  btnLimpiar.addEventListener('click', () => {
    for (const id of [...orden]) {
      if (entradas.get(id)?.estado !== 'viva') {
        entradas.delete(id);
        orden.splice(orden.indexOf(id), 1);
      }
    }
    if (activaId && !entradas.has(activaId)) {
      activaId = orden.length > 0 ? orden[orden.length - 1] : null;
    }
    repintar();
  });

  btnCopiar.addEventListener('click', () => {
    const e = activaId ? entradas.get(activaId) ?? null : null;
    if (!e) {
      opts.onError?.('nada que copiar', 'abre una consola primero');
      return;
    }
    const texto = `$ ${e.comando}\n${e.lineas.map((l) => l.texto).join('\n')}`;
    void navigator.clipboard.writeText(texto).catch((err: unknown) => {
      opts.onError?.('no se pudo copiar', String(err));
    });
  });

  repintar();
  return {
    raiz,
    manejarEvento,
    revelar(id: string): void {
      if (!entradas.has(id)) return;
      activaId = id;
      pintarLista();
      pintarVisor();
    },
    hayVivas(): boolean {
      return vivas() > 0;
    },
  };
}
