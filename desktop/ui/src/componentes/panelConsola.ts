// Tab Consola: visor de ejecuciones `comando` en vivo (plan 209A-1 F3).
// Store por `id_ejecucion` + lista y visor. Solo pinta: los eventos llegan
// por `manejarEvento` desde el hook `onConsolaEvento` (un solo `TurnoReal`
// por adaptador, sin duplicados). Sin polling global ni PTY.
// [219A-5 F1] Excepción acotada al cero-polling 209A-1: el fondo no emite
// `consola_fin` (la tool ya volvió), así que sin refresco la vista se queda
// en `● corriendo` hasta reabrir la tab. Intervalo de 2 s SOLO mientras haya
// vivas (se arranca con actividad y se detiene solo al vaciarse): cada tick
// llama a `sincronizar`, que congela las ya terminadas y recarga las propias.
// Sin vivas no hay timer ni red.
// [219A-3] Visor interactivo: la lista es la sub-barra interna (ver +
// seleccionar); `sincronizar` trae vivas+recientes del backend (backfill al
// abrir la tab); las vivas aceptan stdin en la caja del visor.

import '../estilos/consola.css';
import { el } from '../util/dom';
import { crearBotonIcono, crearCabeceraPanel } from './chromePanel';
import type { EntradaConsola, LineaConsola, OpcionesPanelConsola, PanelConsola } from './consolaModelo';
import { acotarLineas, contarVivas } from './consolaModelo';
import { pintarListaConsola } from './consolaLista';
import { pintarVisorConsola } from './consolaVisor';
import type { NodosVisorConsola } from './consolaVisor';
import { manejarEventoConsola } from './consolaEventos';
import type { CtxEventoConsola } from './consolaEventos';
import { crearPropiaConsola, sincronizarConsolas } from './consolaRemoto';
import type { CtxRemotoConsola } from './consolaRemoto';

// Compatibilidad: los tipos viven en `consolaModelo.ts`; el orquestador
// (`panelDerecho`, `ganchos`) sigue importándolos desde aquí.
export type {
  EntradaConsola,
  EventoConsola,
  LineaConsola,
  OpcionesPanelConsola,
  PanelConsola,
} from './consolaModelo';
import type { EventoConsola } from './consolaModelo';

export function montarPanelConsola(opts: OpcionesPanelConsola): PanelConsola {
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

  function pintarContador(): void {
    const n = contarVivas(entradas);
    contador.textContent = n === 0 ? '' : `${n} viva${n === 1 ? '' : 's'}`;
  }

  /** Selección compartida por el click de la fila y `revelar`. */
  function elegir(id: string): void {
    activaId = id;
    repintar();
    // La entrada con historial en vivo completo no necesita backfill
    // (`cargarTranscript` la marca y sale); la rescatada por sincronizar
    // sí lo pide aquí (bajo demanda, una sola vez). La propia viva se
    // refresca (no emite chunks: ver `recargarSalida`).
    cargarTranscript(id);
    const sel = entradas.get(id);
    if (sel && sel.origen === 'usuario' && sel.estado === 'viva') recargarSalida(id);
  }

  const nodosVisor: NodosVisorConsola = { visor, vacio, visorComando, visorSalida, stdinFila, visorPie };

  // La sub-barra la pinta `pintarListaConsola` (ver `consolaLista.ts`).

  // El visor lo pinta `pintarVisorConsola` (ver `consolaVisor.ts`).

  function repintar(): void {
    pintarContador();
    pintarListaConsola({
      lista,
      orden,
      entradas,
      activaId,
      puedeMatar: opts.onMatar !== undefined,
      alElegir: elegir,
      alMatar: matarEntrada,
    });
    const activa = activaId ? entradas.get(activaId) ?? null : null;
    pintarVisorConsola(nodosVisor, activa, opts.onEscribir !== undefined);
    btnMatar.disabled = activa?.estado !== 'viva' || opts.onMatar === undefined;
  }

  /** [219A-5 F1] Refresco acotado con vivas (ver cabecera): arranca con
   * actividad y se detiene solo cuando no quedan vivas. Sin solape: un tick
   * no entra si el anterior sigue en vuelo. */
  let intervaloRefresco: number | null = null;
  let sincronizando = false;
  function detenerRefresco(): void {
    if (intervaloRefresco !== null) {
      window.clearInterval(intervaloRefresco);
      intervaloRefresco = null;
    }
  }
  function programarRefresco(): void {
    if (intervaloRefresco !== null || opts.onSincronizar === undefined) return;
    if (contarVivas(entradas) === 0) return;
    intervaloRefresco = window.setInterval(() => {
      if (contarVivas(entradas) === 0) {
        detenerRefresco();
        return;
      }
      if (sincronizando) return;
      sincronizando = true;
      void sincronizarConsolas(ctxRemoto).finally(() => {
        sincronizando = false;
      });
    }, 2000);
  }

  // El acotado vive en `acotarLineas` (ver `consolaModelo.ts`).

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
      // [219A-5 F2] Sin blankeo: un volcado vacío transitorio (reap en
      // curso en el backend) no borra lo ya pintado; solo sustituye si trae
      // líneas o la vista está vacía. Los pendientes siempre se fusionan.
      if (base.length > 0 || viva.lineas.length === 0) {
        viva.lineas = [...base, ...viva.pendientes];
      } else {
        viva.lineas.push(...viva.pendientes);
      }
      viva.pendientes = [];
      acotarLineas(viva, viva.lineas);
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
        acotarLineas(viva, viva.lineas);
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
      // [219A-5 F2] Sin blankeo (igual que `cargarTranscript`): un volcado
      // vacío no borra lo ya pintado.
      if (base.length > 0 || viva.lineas.length === 0) {
        viva.lineas = [...base, ...viva.pendientes];
      } else {
        viva.lineas.push(...viva.pendientes);
      }
      viva.pendientes = [];
      acotarLineas(viva, viva.lineas);
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

  // Las acciones contra el backend viven en `consolaRemoto.ts`; este
  // contexto las opera sobre el store de este panel.
  const ctxRemoto: CtxRemotoConsola = {
    entradas,
    orden,
    getActiva: () => activaId,
    setActiva: (id) => {
      activaId = id;
    },
    repintar: () => repintar(),
    cargarTranscript: (id) => cargarTranscript(id),
    recargarSalida: (id) => recargarSalida(id),
    programarRefresco: () => programarRefresco(),
    setNuevaHabilitada: (habilitada) => {
      btnNueva.disabled = !habilitada;
    },
    onCrear: opts.onCrear,
    onSincronizar: opts.onSincronizar,
    onError: opts.onError,
  };

  const ctxEventos: CtxEventoConsola = {
    entradas,
    orden,
    getActiva: () => activaId,
    setActiva: (id) => {
      activaId = id;
    },
    repintar: () => repintar(),
    programarRefresco: () => programarRefresco(),
  };

  // Los eventos del turno los aplica `manejarEventoConsola` (ver
  // `consolaEventos.ts`) sobre el store de este panel.
  function manejarEvento(ev: EventoConsola): void {
    manejarEventoConsola(ctxEventos, ev);
  }

  // [219A-4] [+ Nueva] en cabecera: abre la shell propia (ver
  // `crearPropia`). Sin `onCrear` el botón no hace nada (panel en vivo).
  btnNueva.addEventListener('click', () => {
    crearPropiaConsola(ctxRemoto);
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
  // [219A-5 F4] El backend dice si la mató (`false` = ya había terminado,
  // honesto, con aviso en vez del silencio de 209A-1). La × por fila usa
  // esta misma vía sin pasar por la activa.
  function matarEntrada(id: string): void {
    const e = entradas.get(id) ?? null;
    if (!e || e.estado !== 'viva' || opts.onMatar === undefined) return;
    const r = opts.onMatar(id);
    if (r !== undefined && typeof (r as Promise<boolean>).then === 'function') {
      (r as Promise<boolean>).then(
        (matada) => {
          opts.onInfo?.(
            matada ? 'consola matada: esperando el fin…' : 'la consola ya había terminado',
          );
        },
        (err: unknown) => {
          opts.onError?.('no se pudo matar la consola', String(err));
        },
      );
    }
  }
  btnMatar.addEventListener('click', () => {
    if (activaId) matarEntrada(activaId);
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
      elegir(id);
    },
    hayVivas(): boolean {
      return contarVivas(entradas) > 0;
    },
    sincronizar: () => sincronizarConsolas(ctxRemoto),
  };
}
