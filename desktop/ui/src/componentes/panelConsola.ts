// Tab Consola: visor de ejecuciones `comando` en vivo (plan 209A-1 F3).
// Store por `id_ejecucion` + lista y visor. Solo pinta: los eventos llegan
// por `manejarEvento` desde el hook `onConsolaEvento` (un solo `TurnoReal`
// por adaptador, sin duplicados). Sin polling, sin timers, sin PTY.
// [219A-3] Visor interactivo: la lista es la sub-barra interna (ver +
// seleccionar); `sincronizar` trae vivas+recientes del backend (backfill al
// abrir la tab); las vivas aceptan stdin en la caja del visor.

import '../estilos/consola.css';
import { el } from '../util/dom';
import { crearBotonIcono, crearCabeceraPanel } from './chromePanel';
import type { AgenteEvento, InfoConsolaLista, NuevaConsola, TranscriptConsola } from '../tauri/realTipos';

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
  /** [219A-4] Dueño: `agente` = la abrió el modelo con `comando`;
   * `usuario` = shell propia abierta con [+ Nueva] (sin jaula). */
  origen: 'agente' | 'usuario';
  /** [219A-3] Backfill ya cargado (o innecesario: historial en vivo
   * completo). Con `cargando`, los chunks van a `pendientes` y se fusionan
   * al resolver (sin duplicar lo que ya pinta la vista). */
  transcript: boolean;
  cargando: boolean;
  pendientes: LineaConsola[];
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

/** [219A-4] Etiqueta de dueño para la fila y el pie del visor. */
function duenoTexto(e: EntradaConsola): string {
  return e.origen === 'usuario' ? 'mía' : 'agente';
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

export function montarPanelConsola(opts: {
  onError?: (texto: string, detalle?: string) => void;
  /** [209A-1 F4-resto] El usuario pulsó × sobre la consola ACTIVA (solo
   * habilitado si sigue viva): el orquestador la mata en el backend. El
   * `consola_fin` posterior la marca como terminada en la vista. */
  onMatar?: (idEjecucion: string) => void;
  /** [219A-3] Puentes al backend (los cablea el orquestador a la sesión):
   * lista para la sub-barra, transcript por entrada, stdin de una viva.
   * Ausentes = panel solo-en-vivo (como hasta 209A-1).
   * [219A-4] `onCrear` abre una consola PROPIA (shell del SO, sin jaula). */
  onSincronizar?: () => Promise<InfoConsolaLista[]>;
  onLeerSalida?: (idEjecucion: string) => Promise<TranscriptConsola>;
  onEscribir?: (idEjecucion: string, texto: string) => Promise<number>;
  onCrear?: () => Promise<NuevaConsola>;
}): PanelConsola {
  const entradas = new Map<string, EntradaConsola>();
  const orden: string[] = [];
  let activaId: string | null = null;

  const raiz = el('div', 'panel-consola');
  raiz.setAttribute('aria-label', 'consolas de comandos');

  // (219A-1) Cabecera y botones con los constructores únicos: título en
  // `--sm` y botones icono 28×28 sin borde.
  const contador = el('span', 'consola-contador');
  // [219A-4] [+ Nueva]: abre una consola PROPIA (shell del SO, sin jaula).
  // Primera acción de la cabecera (es la entrada del operador a la tab).
  const btnNueva = crearBotonIcono({
    icono: 'mas',
    etiqueta: 'Abrir consola propia (shell, sin jaula)',
  });
  btnNueva.setAttribute('aria-label', 'Abrir consola propia');
  const btnLimpiar = crearBotonIcono({
    icono: 'x',
    etiqueta: 'Quitar las terminadas (las vivas siguen corriendo)',
  });
  btnLimpiar.setAttribute('aria-label', 'Quitar consolas terminadas');
  const btnCopiar = crearBotonIcono({
    icono: 'copiar',
    etiqueta: 'Copiar el transcript de la consola activa',
  });
  btnCopiar.setAttribute('aria-label', 'Copiar transcript');
  // [209A-1 F4-resto] × por entrada viva: mata la ACTIVA en el backend
  // (las terminadas se quitan con limpiar; no hay nada que matar).
  // El click se cablea más abajo (mismo sitio que antes).
  const btnMatar = crearBotonIcono({
    icono: 'x-circulo',
    etiqueta: 'Matar la consola activa (solo si sigue corriendo)',
    deshabilitado: true,
  });
  btnMatar.setAttribute('aria-label', 'Matar la consola activa');
  const cabecera = crearCabeceraPanel({
    claseRaiz: 'consola-cabecera',
    titulo: 'Consola',
    medio: [contador],
    acciones: [btnNueva, btnLimpiar, btnCopiar, btnMatar],
  }).raiz;

  const lista = el('div', 'consola-lista');
  lista.setAttribute('role', 'list');
  const visor = el('div', 'consola-visor');
  const visorComando = el('div', 'consola-comando');
  const visorSalida = el('div', 'consola-salida');
  // [219A-3] Caja de stdin: solo visible con activa viva (ver `pintarVisor`).
  // Enter envía lo tecleado + `\n` al stdin de esa consola (crudo, sin eco
  // local: el eco lo pone el propio programa si lee; el transcript del
  // backend solo captura stdout/stderr).
  const stdinFila = el('div', 'consola-stdin');
  const stdinPrompt = el('span', 'consola-prompt');
  stdinPrompt.textContent = '> ';
  const stdinInput = el('input', 'consola-stdin-entrada');
  stdinInput.type = 'text';
  stdinInput.placeholder = 'escribir al stdin (Enter para enviar)';
  stdinInput.setAttribute('aria-label', 'Escribir al stdin de la consola activa');
  stdinFila.append(stdinPrompt, stdinInput);
  const visorPie = el('div', 'consola-pie');
  visor.append(visorComando, visorSalida, stdinFila, visorPie);
  // [219A-3] Marco listo en vez de texto de vacío: la tab siempre ofrece su
  // sub-barra (arriba) + este visor; las consolas del agente aparecen aquí.
  // [219A-4] Con + se abre una consola propia (shell) para trabajar el operador.
  const vacio = el('div', 'consola-vacio');
  vacio.textContent = 'Consola lista: cada comando que abra el agente aparece arriba para verlo e interactuar con él; con + abres una consola propia (shell).';
  const nota = el('div', 'consola-nota');
  // [219A-3 límites honestos] Las vivas aceptan stdin en la caja de abajo;
  // sigue sin haber PTY: los programas que detectan no-TTY cambian formato.
  nota.textContent = 'Las vivas aceptan stdin abajo; sin PTY: los programas que detectan no-TTY cambian formato (sin color ni barras).';
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
      fila.addEventListener('click', () => {
        activaId = id;
        pintarLista();
        pintarVisor();
        // La entrada con historial en vivo completo no necesita backfill
        // (`cargarTranscript` la marca y sale); la rescatada por sincronizar
        // sí lo pide aquí (bajo demanda, una sola vez). La propia viva se
        // refresca (no emite chunks: ver `recargarSalida`).
        cargarTranscript(id);
        const sel = entradas.get(id);
        if (sel && sel.origen === 'usuario' && sel.estado === 'viva') recargarSalida(id);
      });
      lista.appendChild(fila);
    }
  }

  function pintarVisor(): void {
    const e = activaId ? entradas.get(activaId) ?? null : null;
    const hay = e !== null;
    // `hidden` + regla CSS (sin estilo inline: regla cssInlineScript).
    visor.hidden = !hay;
    vacio.hidden = hay;
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
    // [219A-4] El pie abre con el dueño (el visor mezcla mías + agente).
    const partes = [duenoTexto(e), estadoTexto(e)];
    if (e.duracionMs !== null) partes.push(`${(e.duracionMs / 1000).toFixed(1)} s`);
    if (e.truncada) partes.push('salida truncada por el backend');
    if (e.descartadas > 0) partes.push(`${e.descartadas} líneas antiguas fuera de vista`);
    if (e.cargando) partes.push('cargando historial…');
    visorPie.textContent = partes.join(' · ');
    // `hidden` + regla CSS (sin estilo inline: regla cssInlineScript).
    stdinFila.hidden = e.estado !== 'viva' || opts.onEscribir === undefined;
  }

  function repintar(): void {
    pintarContador();
    pintarLista();
    pintarVisor();
    const activa = activaId ? entradas.get(activaId) ?? null : null;
    btnMatar.disabled = activa?.estado !== 'viva' || opts.onMatar === undefined;
  }

  /** Acota un depósito de líneas al tope de vista (cuenta las caídas). */
  function acotar(e: EntradaConsola, deposito: LineaConsola[]): void {
    if (deposito.length > MAX_LINEAS_VISTA) {
      const sobran = deposito.length - MAX_LINEAS_VISTA;
      deposito.splice(0, sobran);
      e.descartadas += sobran;
    }
  }

  /** [219A-3] Backfill de UNA entrada (una sola vez): si ya trae historial
   * en vivo se marca y sale; si no, vuelca el transcript del backend y
   * fusiona lo que llegó en vivo durante la carga (sin duplicar). Un fin
   * rescatado congela la entrada si seguía viva en la vista. */
  function cargarTranscript(id: string): void {
    const e = entradas.get(id);
    if (!e || e.transcript || e.cargando || opts.onLeerSalida === undefined) return;
    if (e.lineas.length > 0) {
      e.transcript = true;
      return;
    }
    e.cargando = true;
    repintar();
    void opts.onLeerSalida(id).then((t) => {
      const viva = entradas.get(id);
      if (!viva) return;
      const base: LineaConsola[] = t.lineas.map((l) => ({ flujo: l.flujo, texto: l.linea }));
      viva.lineas = [...base, ...viva.pendientes];
      viva.pendientes = [];
      acotar(viva, viva.lineas);
      viva.transcript = true;
      viva.cargando = false;
      if (!t.viva && viva.estado === 'viva') {
        viva.estado = 'fin';
        viva.codigo = t.codigo_salida;
      }
      repintar();
    }).catch((err: unknown) => {
      const viva = entradas.get(id);
      if (viva) {
        viva.lineas.push(...viva.pendientes);
        viva.pendientes = [];
        acotar(viva, viva.lineas);
        viva.cargando = false;
      }
      opts.onError?.('no se pudo leer la consola', String(err));
      repintar();
    });
  }

  /** [219A-4] Refresco de una PROPIA viva: las propias no emiten chunks
   * (ningún turno las emite), así que el transcript se relee del backend al
   * seleccionar, al abrir la tab y tras cada escritura (ida y vuelta sin
   * polling ni timers). Solo propias: el vivo del agente no se toca. */
  function recargarSalida(id: string): void {
    const e = entradas.get(id);
    if (!e || e.origen !== 'usuario' || e.cargando || opts.onLeerSalida === undefined) return;
    e.cargando = true;
    repintar();
    void opts.onLeerSalida(id).then((t) => {
      const viva = entradas.get(id);
      if (!viva) return;
      const base: LineaConsola[] = t.lineas.map((l) => ({ flujo: l.flujo, texto: l.linea }));
      viva.lineas = [...base, ...viva.pendientes];
      viva.pendientes = [];
      acotar(viva, viva.lineas);
      viva.transcript = true;
      viva.cargando = false;
      viva.origen = t.origen;
      if (!t.viva && viva.estado === 'viva') {
        viva.estado = 'fin';
        viva.codigo = t.codigo_salida;
      }
      repintar();
    }).catch((err: unknown) => {
      const viva = entradas.get(id);
      if (viva) viva.cargando = false;
      opts.onError?.('no se pudo refrescar la consola', String(err));
      repintar();
    });
  }

  /** [219A-4] [+ Nueva]: abre la shell propia, la ancla como activa y carga
   * su transcript (vacío al nacer). El backend responde el id + etiqueta. */
  function crearPropia(): void {
    if (opts.onCrear === undefined) return;
    btnNueva.disabled = true;
    void opts.onCrear().then((n) => {
      if (!entradas.has(n.id_ejecucion)) orden.push(n.id_ejecucion);
      entradas.set(n.id_ejecucion, {
        id: n.id_ejecucion,
        comando: n.comando,
        conversacionId: '',
        lineas: [],
        descartadas: 0,
        estado: 'viva',
        codigo: null,
        truncada: false,
        duracionMs: null,
        origen: 'usuario',
        transcript: false,
        cargando: false,
        pendientes: [],
      });
      activaId = n.id_ejecucion;
      repintar();
      cargarTranscript(n.id_ejecucion);
    }).catch((err: unknown) => {
      opts.onError?.('no se pudo abrir la consola propia', String(err));
    }).finally(() => {
      btnNueva.disabled = false;
    });
  }

  /** [219A-3] Backfill de la sub-barra: crea las que faltan (vivas y
   * recientes) y congela las que el backend ya dio por terminadas. No borra:
   * una archivada que el runner ya olvidó sigue visible hasta limpiar. No
   * roba la selección: solo ancla la última si no había ninguna. */
  async function sincronizar(): Promise<void> {
    if (opts.onSincronizar === undefined) return;
    let remotas: InfoConsolaLista[];
    try {
      remotas = await opts.onSincronizar();
    } catch (err: unknown) {
      opts.onError?.('no se pudieron listar las consolas', String(err));
      return;
    }
    for (const c of remotas) {
      const e = entradas.get(c.id_ejecucion);
      if (!e) {
        entradas.set(c.id_ejecucion, {
          id: c.id_ejecucion,
          comando: c.comando,
          conversacionId: '',
          lineas: [],
          descartadas: 0,
          estado: c.viva ? 'viva' : 'fin',
          codigo: c.codigo_salida,
          truncada: false,
          duracionMs: null,
          // [219A-4] El backend es la fuente del dueño.
          origen: c.origen,
          transcript: false,
          cargando: false,
          pendientes: [],
        });
        orden.push(c.id_ejecucion);
      } else if (!c.viva && e.estado === 'viva') {
        e.estado = 'fin';
        e.codigo = c.codigo_salida ?? e.codigo;
      } else if (e.comando.startsWith('(comando desconocido')) {
        // Entrada rescatada por un chunk sin inicio: el backend sí sabe el
        // comando verbatim (el historial en vivo ya pintado no se toca).
        e.comando = c.comando;
      }
      const actual = entradas.get(c.id_ejecucion);
      if (actual) actual.origen = c.origen;
    }
    if (!activaId && orden.length > 0) {
      activaId = orden[orden.length - 1];
    }
    repintar();
    if (activaId) cargarTranscript(activaId);
    // [219A-4] Las propias no emiten chunks (no hay turno que las emita):
    // se refrescan aquí (al abrir la tab) en vez de con polling.
    for (const c of remotas) {
      if (c.viva && c.origen === 'usuario') recargarSalida(c.id_ejecucion);
    }
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
        // Lo que nace de un evento del turno lo abrió el agente.
        origen: 'agente',
        // Nace del vivo: el historial en vivo es el completo (sin backfill).
        transcript: true,
        cargando: false,
        pendientes: [],
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
          // Un chunk del turno solo lo emite una consola del agente.
          origen: 'agente',
          // Nace de un chunk en vivo: lo que llegue es el historial (el
          // comando real lo trae `sincronizar` si el backend la retiene).
          transcript: true,
          cargando: false,
          pendientes: [],
        };
        entradas.set(ev.id_ejecucion, creada);
        orden.push(ev.id_ejecucion);
        if (!activaId) activaId = ev.id_ejecucion;
        return creada;
      })();
      const linea: LineaConsola = { flujo: ev.flujo, texto: ev.linea };
      // En plena carga de backfill, el vivo espera en `pendientes` y se
      // fusiona al resolver (sin duplicar lo ya volcado).
      if (entrada.cargando) {
        entrada.pendientes.push(linea);
        acotar(entrada, entrada.pendientes);
      } else {
        entrada.lineas.push(linea);
        acotar(entrada, entrada.lineas);
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

  // [219A-4] [+ Nueva] en cabecera: abre la shell propia (ver
  // `crearPropia`). Sin `onCrear` el botón no hace nada (panel en vivo).
  btnNueva.addEventListener('click', () => {
    crearPropia();
  });

  btnLimpiar.addEventListener('click', () => {    for (const id of [...orden]) {
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

  // [209A-1 F4-resto] La vista NO retira la entrada al matar: el backend
  // emite `consola_fin` y `manejarEvento` la congela como terminada.
  btnMatar.addEventListener('click', () => {
    const id = activaId;
    const e = id ? entradas.get(id) ?? null : null;
    if (!id || !e || e.estado !== 'viva' || opts.onMatar === undefined) return;
    opts.onMatar(id);
  });

  // [219A-3] Enter en la caja de stdin: envía lo tecleado + `\n` a la
  // consola ACTIVA (solo viva). En éxito se limpia; en error se conserva el
  // texto y se avisa (la consola pudo terminar entre medias).
  stdinInput.addEventListener('keydown', (ev) => {
    if (ev.key !== 'Enter') return;
    const id = activaId;
    const e = id ? entradas.get(id) ?? null : null;
    if (!id || !e || e.estado !== 'viva' || opts.onEscribir === undefined) return;
    const texto = stdinInput.value;
    if (texto === '') return;
    ev.preventDefault();
    void opts.onEscribir(id, `${texto}\n`).then(() => {
      if (stdinInput.value === texto) stdinInput.value = '';
      // [219A-4] La propia no emite chunks: tras escribir se relee el
      // transcript para ver la respuesta (ida y vuelta sin polling).
      const e2 = entradas.get(id);
      if (e2 && e2.origen === 'usuario' && e2.estado === 'viva') recargarSalida(id);
    }).catch((err: unknown) => {
      opts.onError?.('no se pudo escribir a la consola', String(err));
    });
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
      repintar();
      cargarTranscript(id);
      const sel = entradas.get(id);
      if (sel && sel.origen === 'usuario' && sel.estado === 'viva') recargarSalida(id);
    },
    hayVivas(): boolean {
      return vivas() > 0;
    },
    sincronizar,
  };
}
