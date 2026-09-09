/* Historial del panel de chat: mapa de textos de usuario (para edición),
 * cambios de archivo del historial (sincronizar Files) y pintado del
 * historial persistido intercalando herramientas + pie de turno. */
import type { EstadoHerramienta, ResultadoHerramienta } from '../dominio/tipos';
import {
  crearHerramienta,
  crearMensajeAsistente,
  crearMensajeUsuario,
  crearPieTurno,
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
  /** Añade el pie de turno (tokens/modelo reales) como último bloque. */
  anadirPieTurno(u: UsoTurno): void;
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
  function registrarCambioHistorial(tool: string, args: unknown, resumen: string, diff: string | null): void {
    if (tool !== 'file_write' && tool !== 'file_patch') return;
    const ruta = rutaDeArgs(args);
    if (!ruta) return;
    cambios.push({
      origen: 'tool',
      tool,
      ruta,
      titulo: descripcionDeTool(tool, args),
      resumen,
      diff,
    });
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
    for (const m of historial) {
      if (m.rol === 'user') {
        usuariosHistorial.set(m.id, m.contenido);
        mensajes.appendChild(crearMensajeUsuario(m.contenido, m.id, deps.abrirAccionesMensaje));
        (porTurno.get(idxUser) ?? []).forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
        idxUser++;
      } else if (m.rol === 'assistant') {
        mensajes.appendChild(crearMensajeAsistente(m.contenido));
      }
    }
    residuales.forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
    if (ultimo_uso) {
      mensajes.appendChild(
        crearPieTurno({
          tokensPrompt: ultimo_uso.tokens_prompt,
          tokensComplecion: ultimo_uso.tokens_complecion,
          modelo: ultimo_uso.provider ? `${ultimo_uso.provider}/${ultimo_uso.modelo}` : ultimo_uso.modelo || null,
          ocupacionPct: null,
          maxVentana: null,
          reservaSalida: null,
          alCopiar: deps.copiarUltimoTramo,
        }),
      );
    }
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  /** Añade el pie de turno (tokens/modelo reales) como último bloque. */
  function anadirPieTurno(u: UsoTurno): void {
    mensajes.appendChild(
      crearPieTurno({
        tokensPrompt: u.tokensPrompt,
        tokensComplecion: u.tokensComplecion,
        modelo: u.modelo,
        ocupacionPct: u.ocupacionPct,
        maxVentana: u.maxVentana,
        reservaSalida: u.reservaSalida,
        alCopiar: deps.copiarUltimoTramo,
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

  return {
    pintarHistorial,
    anadirPieTurno,
    limpiarHistorial,
    getTextoUsuario(id: string) {
      return usuariosHistorial.get(id);
    },
    listarCambios() {
      return [...cambios];
    },
  };
}
