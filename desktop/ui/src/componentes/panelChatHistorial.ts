/* Historial del panel de chat: mapa de textos de usuario (para edición),
 * cambios de archivo del historial (sincronizar Files) y pintado del
 * historial persistido intercalando herramientas + pie de turno. */
import type { EstadoHerramienta, ResultadoHerramienta } from '../dominio/tipos';
import {
  crearHerramienta,
  crearMensajeAsistente,
  crearMensajeUsuario,
  crearPieTurno,
  crearRazonamientoCerrado,
  crearResumenTurno,
  formatearResultadoHerramienta,
} from './mensajes';
import type { CambioArchivoPanel } from './panelChatTipos';
import {
  descripcionDeTool,
  iconoDeTool,
  type AccionRecuperada,
  type MensajeGuardado,
  type UsoTurno,
} from '../tauri/real';

export interface HistorialDeps {
  mensajes: HTMLElement;
  /** Menú por mensaje (lo aporta el módulo de acciones del panel). */
  abrirAccionesMensaje(id: string, rect: DOMRect): void;
  /** Copia el último tramo (lo aporta el módulo de acciones). */
  copiarUltimoTramo(): void;
  /** Aviso del panel (lo aporta el módulo de acciones). */
  aviso(texto: string, meta: string, detalle: string): void;
  /** [129A-4 F4] Abre el visor del log de un turno. */
  verLogTurno(turnoId: string): void;
  /** [129A-8] Abre la tab Cambios en el archivo (enlace del resumen). */
  verEnCambios(ruta: string): void;
}

export interface HistorialChat {
  /** Pinta historial persistido intercalando las herramientas del turno. */
  pintarHistorial(
    historial: MensajeGuardado[],
    acciones?: AccionRecuperada[],
    ultimo_uso?: {
      provider: string;
      modelo: string;
      tokens_prompt: number;
      tokens_complecion: number;
    } | null,
  ): void;
  /** Añade el pie de turno (tokens/modelo reales) como último bloque.
   * [109A-5 F3] `turnoId` queda en el DOM (`data-turno`) para que el evento
   * `meta_lograda` pueda anclar el badge a este pie, que se crea al CERRAR el
   * turno y no cuando llega el logro. */
  anadirPieTurno(u: UsoTurno, turnoId?: string | null): void;
  /** [129A-8] Pinta el resumen de cambios tras el pie (sin cambios: nada). */
  anadirResumenTurno(cambios: CambioArchivoPanel[]): void;
  /** Vacía mensajes, mapa de usuario y cambios (sin tocar la entrada). */
  limpiarHistorial(): void;
  /** Texto original de un mensaje de usuario (para edición). */
  getTextoUsuario(id: string): string | undefined;
  /** Cambios de archivos acumulados (para sincronizar Files). */
  listarCambios(): CambioArchivoPanel[];
}

export function crearHistorial(deps: HistorialDeps): HistorialChat {
  const { mensajes } = deps;

  let usuariosHistorial = new Map<string, string>();

  function argumentosPersistidos(json: string | null): unknown {
    if (!json) return undefined;
    try {
      return JSON.parse(json) as unknown;
    } catch {
      return undefined;
    }
  }

  /** Render de una acción recuperada → bloque `.herramienta` estático. */
  function bloqueDesdeAccion(accion: AccionRecuperada): HTMLElement {
    const meta = accion.ok ? 'ok' : 'falló';
    const cuerpo = formatearResultadoHerramienta(accion.resumen, accion.diff);
    const resultado: ResultadoHerramienta = { tipo: 'html', html: cuerpo };
    const estado: EstadoHerramienta = accion.ok
      ? { estado: 'completada', meta, resultado }
      : { estado: 'error', meta, resultado };
    // [089A-12] Conserva cambios para sincronizar el pane Files al abrirlo.
    registrarCambioHistorial(accion.tool, argumentosPersistidos(accion.argumentos_json), accion.resumen, accion.diff);
    return crearHerramienta({
      icono: iconoDeTool(accion.tool),
      titulo: descripcionDeTool(accion.tool, argumentosPersistidos(accion.argumentos_json)),
      estado,
    });
  }

  /** [089A-2] Cambios del historial (file_write/file_patch con ruta). */
  const cambios: CambioArchivoPanel[] = [];
  /** Entrada de resumen desde una acción (null = no es escritura con ruta). */
  function entradaDesdeAccion(
    tool: string,
    args: unknown,
    resumen: string,
    diff: string | null,
  ): CambioArchivoPanel | null {
    if (tool !== 'file_write' && tool !== 'file_patch') return null;
    const ruta = rutaDeArgs(args);
    if (!ruta) return null;
    return {
      origen: 'tool',
      tool,
      ruta,
      titulo: descripcionDeTool(tool, args),
      resumen,
      diff,
    };
  }
  function registrarCambioHistorial(tool: string, args: unknown, resumen: string, diff: string | null): void {
    const entrada = entradaDesdeAccion(tool, args, resumen, diff);
    if (entrada) cambios.push(entrada);
  }
  /** Extrae la ruta de los argumentos de una tool de archivo. */
  function rutaDeArgs(args: unknown): string | null {
    if (typeof args !== 'object' || args === null) return null;
    const o = args as Record<string, unknown>;
    for (const clave of ['ruta', 'path', 'archivo']) {
      const v = o[clave];
      if (typeof v === 'string' && v.trim() !== '') return v;
    }
    return null;
  }

  /** Pinta historial persistido intercalando las herramientas del turno. */
  function pintarHistorial(
    historial: MensajeGuardado[],
    acciones: AccionRecuperada[] = [],
    ultimo_uso?: { provider: string; modelo: string; tokens_prompt: number; tokens_complecion: number } | null,
  ): void {
    const users = historial.filter((m) => m.rol === 'user');
    const en = (s: string): number => Date.parse(s) || 0;
    const porTurno = new Map<number, AccionRecuperada[]>();
    const residuales: AccionRecuperada[] = [];
    acciones.forEach((a) => {
      const t = en(a.turno_en);
      let indice = -1;
      for (let i = 0; i < users.length; i++) {
        if (en(users[i].creado_en) <= t) indice = i;
        else break;
      }
      if (indice < 0) {
        residuales.push(a);
        return;
      }
      const lista = porTurno.get(indice) ?? [];
      lista.push(a);
      porTurno.set(indice, lista);
    });

    let idxUser = 0;
    // [fix 12-09] El backend solo expone el uso del ÚLTIMO turno
    // (`ultimo_uso`): cada respuesta del asistente conserva su propio pie;
    // las anteriores muestran su hora (`creado_en`) y la última además los
    // tokens reales. Sin esto la recarga dejaba un único pie al final.
    let ultimoIdxAsistente = -1;
    historial.forEach((mm, ii) => {
      if (mm.rol === 'assistant') ultimoIdxAsistente = ii;
    });
    // [129A-8 F2] El resumen va tras el pie del turno que hizo los cambios:
    // cada respuesta pertenece al último usuario previo; si un turno trae
    // varias respuestas, el resumen cierra la ÚLTIMA (tras su pie).
    const turnoDe: number[] = historial.map(() => -1);
    let turnoVisto = -1;
    historial.forEach((m, ii) => {
      if (m.rol === 'user') turnoVisto++;
      turnoDe[ii] = turnoVisto;
    });
    const ultimoAsistenteDe = new Map<number, number>();
    historial.forEach((m, ii) => {
      if (m.rol === 'assistant' && turnoDe[ii] >= 0) ultimoAsistenteDe.set(turnoDe[ii], ii);
    });
    /** Resumen desde acciones persistidas (sin tocar `cambios` de Files). */
    function resumenDesdeAcciones(accionesTurno: AccionRecuperada[]): CambioArchivoPanel[] {
      const entradas: CambioArchivoPanel[] = [];
      for (const a of accionesTurno) {
        const entrada = entradaDesdeAccion(
          a.tool,
          argumentosPersistidos(a.argumentos_json),
          a.resumen,
          a.diff ?? null,
        );
        if (entrada) entradas.push(entrada);
      }
      return entradas;
    }
    function pintarResumen(entradas: CambioArchivoPanel[]): void {
      const bloque = crearResumenTurno(entradas, (ruta) => deps.verEnCambios(ruta));
      if (bloque) mensajes.appendChild(bloque);
    }
    historial.forEach((m, ii) => {
      if (m.rol === 'user') {
        usuariosHistorial.set(m.id, m.contenido);
        mensajes.appendChild(crearMensajeUsuario(m.contenido, m.id, deps.abrirAccionesMensaje));
        (porTurno.get(idxUser) ?? []).forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
        idxUser++;
      } else if (m.rol === 'assistant') {
        mensajes.appendChild(crearMensajeAsistente(m.contenido));
        const ms = Date.parse(m.creado_en);
        const hora = Number.isFinite(ms) ? ms : null;
        if (ii === ultimoIdxAsistente && ultimo_uso) {
          mensajes.appendChild(
            crearPieTurno({
              tokensPrompt: ultimo_uso.tokens_prompt,
              tokensComplecion: ultimo_uso.tokens_complecion,
              modelo: ultimo_uso.provider
                ? `${ultimo_uso.provider}/${ultimo_uso.modelo}`
                : ultimo_uso.modelo || null,
              ocupacionPct: null,
              maxVentana: null,
              reservaSalida: null,
              // [129A-2] Recarga: la velocidad no se persiste, se omite la parte.
              velocidadTokS: null,
              creadoEnMs: hora,
              alCopiar: deps.copiarUltimoTramo,
            }),
          );
        } else {
          mensajes.appendChild(
            crearPieTurno({
              tokensPrompt: 0,
              tokensComplecion: 0,
              modelo: null,
              ocupacionPct: null,
              maxVentana: null,
              reservaSalida: null,
              velocidadTokS: null,
              creadoEnMs: hora,
              alCopiar: deps.copiarUltimoTramo,
            }),
          );
        }
        // [129A-8 F2] El historial viejo también muestra su resumen, tras el
        // pie de la última respuesta del turno (vacío = sin bloque).
        if (ultimoAsistenteDe.get(turnoDe[ii]) === ii && turnoDe[ii] >= 0) {
          pintarResumen(resumenDesdeAcciones(porTurno.get(turnoDe[ii]) ?? []));
        }
      } else if (m.rol === 'reasoning' && m.contenido.trim() !== '') {
        /* [129A-1] Pensamiento persistido: mismo summary cerrado que en vivo
         * (`aplicarEventos`), intercalado por `creado_en` entre el usuario y
         * la respuesta gracias al `rowid` del ORDER BY. */
        mensajes.appendChild(crearRazonamientoCerrado(m.contenido, 'razonamiento'));
      }
    });
    residuales.forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
    // [129A-8 F2] Acciones previas al primer usuario: resumen al final, donde
    // se pintan (sin pie de turno al que anclarse).
    if (residuales.length > 0) pintarResumen(resumenDesdeAcciones(residuales));
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  /** Añade el pie de turno (tokens/modelo reales) como último bloque. */
  function anadirPieTurno(u: UsoTurno, turnoId?: string | null): void {
    mensajes.appendChild(
      crearPieTurno({
        tokensPrompt: u.tokensPrompt,
        tokensComplecion: u.tokensComplecion,
        modelo: u.modelo,
        ocupacionPct: u.ocupacionPct,
        maxVentana: u.maxVentana,
        reservaSalida: u.reservaSalida,
        turnoId: turnoId ?? null,
        // [129A-2] La velocidad viaja en el `UsoTurno` (la mide el turno en
        // vivo con su propio reloj; en recarga es `null`).
        velocidadTokS: u.velocidadTokS ?? null,
        // [fix 12-09] El pie se crea al cerrar el turno: hora visible + fecha
        // exacta en hover (vale para el camino real y el mock).
        creadoEnMs: Date.now(),
        alCopiar: deps.copiarUltimoTramo,
        // [129A-4 F4] "Ver log": solo con id real (sin id no hay log que
        // pedir; crearPieTurno oculta el botón cuando falta).
        alVerLog: turnoId ? () => deps.verLogTurno(turnoId) : undefined,
      }),
    );
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  function limpiarHistorial(): void {
    mensajes.replaceChildren();
    usuariosHistorial = new Map<string, string>();
    // [089A-2] Los cambios acumulados pertenecen a la conversación anterior.
    cambios.length = 0;
  }

  /** [129A-8 F1] Resumen en vivo tras el pie (sin cambios: sin bloque). */
  function anadirResumenTurno(cambiosTurno: CambioArchivoPanel[]): void {
    const bloque = crearResumenTurno(cambiosTurno, (ruta) => deps.verEnCambios(ruta));
    if (!bloque) return;
    mensajes.appendChild(bloque);
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  return {
    pintarHistorial,
    anadirPieTurno,
    anadirResumenTurno,
    limpiarHistorial,
    getTextoUsuario(id: string) {
      return usuariosHistorial.get(id);
    },
    listarCambios() {
      return [...cambios];
    },
  };
}
