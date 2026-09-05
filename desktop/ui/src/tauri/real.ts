// ============================================================
// Adaptador real Tauri (plan 039A-1, Fases 4-5): la UI habla al núcleo
// vía comandos in-process y escucha el contrato AgenteEvento.
// Misma superficie que la simulación (montar/detener) para no tocar
// el layout: solo cambia la fuente de los eventos.
// ============================================================

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import {
  crearAvisoSistema,
  crearHerramientaViva,
  crearMensajeAsistenteVivo,
  crearMensajeUsuario,
  crearTarjetaAprobacion,
  type AsistenteVivo,
  type HerramientaViva,
} from '../componentes/mensajes';
import type { DecisionAprobacion, IconoNombre } from '../dominio/tipos';
import { el } from '../util/dom';

/**
 * Contrato AgenteEvento del núcleo (tag `tipo`, snake_case). Fiel a
 * `core/src/evento.rs` (`#[serde(tag = "tipo", rename_all = "snake_case")]`):
 * el backend envía `{ tipo: "token", texto }`, NO `{ evento: ... }`. Los
 * campos opcionales pueden no venir según el proveedor; el adaptador nunca
 * asume presentes los no obligatorios. [039A-1 04-09 H2-fix] El discriminante
 * era `evento` y el switch no caía nunca (respuesta invisible en vivo).
 */
export type AgenteEvento =
  | { tipo: 'token'; texto: string }
  | { tipo: 'tool_start'; tool: string; argumentos: unknown }
  | { tipo: 'tool_result'; tool: string; ok: boolean; resumen: string; diff?: string | null }
  | { tipo: 'peticion_aprobacion'; id: string; tool: string; argumentos: unknown; clasificacion: string }
  | { tipo: 'requiere_aprobacion'; tool: string; clasificacion: string }
  | { tipo: 'permiso_denegado'; tool: string; motivo: string }
  | { tipo: 'subagente_inicio'; perfil: string; instruccion: string }
  | { tipo: 'subagente_fin'; resumen: string; ok: boolean; parcial: boolean }
  | { tipo: 'plan_propuesto'; cambios: number; resumen: string }
  | {
      tipo: 'usage';
      tokens_prompt: number;
      tokens_complecion: number;
      ocupacion_pct?: number | null;
      provider?: string | null;
      modelo?: string | null;
    }
  | { tipo: 'contexto'; skills: number }
  | {
      tipo: 'contexto_detalle';
      max_ventana: number;
      reserva_salida: number;
      system_instrucciones: number;
      definiciones_tools: number;
      mensajes: number;
      resultados_tools: number;
      total_entrada: number;
      ocupacion_pct: number;
    }
  | { tipo: 'telemetria'; subagentes_parciales: number; herramientas: Array<{ tool: string; usos: number; fallos: number; duracion_ms_total: number }> }
  | { tipo: 'error'; mensaje: string; retryable: boolean }
  | { tipo: 'done'; turno_id: string };

export interface OpcionesTurno {
  proveedor: string;
  modelo: string;
  modo: string;
  /** [039A-1 04-09 H7] Nivel de razonamiento (low|medium|high). */
  razonamiento: string;
  /** [039A-3 P5] Panel destino del turno (`'principal'` default en el backend;
   * el panel lateral pasa `'lateral'`). En M1 solo hay un turno a la vez y
   * este panel es el que recibe los eventos (el adaptador es compartido). */
  panelId?: string;
}

export interface InfoConversacion {
  id: string;
  titulo: string;
  archivada: boolean;
  actualizada_en: string;
}

export interface InfoSesion {
  modelo: string;
  workspace: string;
  proveedores: Array<{ nombre: string; claves: number }>;
  conversacion: InfoConversacion;
  aviso?: string | null;
}

export interface MensajeGuardado {
  id: string;
  conversacion_id: string;
  rol: string;
  contenido: string;
  creado_en: string;
}

/** [039A-1 04-09 H6] Acción (tool) persistida de una conversación, para
 * repintar el bloque `.herramienta` al recargar (resumen/diff). */
export interface AccionRecuperada {
  tool: string;
  ok: boolean;
  resumen: string;
  argumentos_json: string | null;
  diff: string | null;
  turno_en: string;
}

export interface CargaConversacion {
  id: string;
  titulo: string;
  mensajes: MensajeGuardado[];
  acciones: AccionRecuperada[];
  /** [039A-3 P1] Uso/modelo real del último turno (para repintar el pie). */
  ultimo_uso: {
    provider: string;
    modelo: string;
    tokens_prompt: number;
    tokens_complecion: number;
  } | null;
  /** [039A-3 P3] Archivos que tocó el último tramo rebobinado ("volver a
   * punto"), listos para la acción EXPLÍCITA "restaurar archivos de este
   * tramo". Vacío cuando la carga no viene de un rewind. */
  archivos_tramo?: string[];
}

/** [039A-3 P3] Resultado de la restauración explícita de un tramo. */
export interface RestauracionArchivo {
  ruta: string;
  /** "restaurado" | "cambio_externo" | "omitido" | "error" */
  estado: string;
  detalle?: string | null;
}

export interface ResultadoRestauracionTramo {
  archivos: string[];
  restaurados: RestauracionArchivo[];
  omitidos: RestauracionArchivo[];
}

export interface ProveedorInfo {
  id: string;
  modelos: string[];
  claves: number;
}

/** Totales del último turno (para el panel meta / cabecera, sin simular). */
export interface UsoTurno {
  tokensPrompt: number;
  tokensComplecion: number;
  ocupacionPct: number | null;
  /** [039A-3 P1] Modelo REAL que respondió (provider/modelo del último Usage
   * tras fallback); `null` si el proveedor no lo reportó. */
  modelo: string | null;
  /** [039A-3 P1] Ventana máxima de contexto (del `ContextoDetalle`). */
  maxVentana: number | null;
  /** [039A-3 P1] Reserva de salida declarada en el `ContextoDetalle`. */
  reservaSalida: number | null;
  /** [039A-3 P1] Tokens totales de entrada del último desglose de contexto. */
  totalEntrada: number | null;
}

/** Resultado de cierre de un turno, para que el llamador decida el pie. */
export type ResultadoTurno = 'ok' | 'error' | 'cancelado';

export interface HooksAdaptador {
  /** Se llama con cada `abrir_sesion`/`reconfigurar`/`elegir_workspace`. */
  onSesion?: (info: InfoSesion) => void;
  /** [039A-3 P6] Se llama al llegar `contexto_detalle`/`usage` con el estado
   * de contexto del turno (ocupacion_pct + max_ventana) para que la UI
   * actualice el indicador circular en vivo. Hook opcional: no rompe API. */
  onContexto?: (uso: UsoTurno) => void;
}

const RESPUESTA: Record<DecisionAprobacion, string> = {
  aprobar: 'aprobar',
  permitir: 'siempre',
  denegar: 'rechazar',
};

function iconoDeTool(tool: string): IconoNombre {
  if (tool.startsWith('file_')) return 'archivo';
  if (tool === 'web_search') return 'globo';
  if (tool.startsWith('comando')) return 'terminal';
  if (tool === 'task' || tool === 'todo') return 'flujo';
  return 'lupa';
}
export { iconoDeTool };

function compacto(valor: unknown, max = 500): string {
  const s = typeof valor === 'string' ? valor : JSON.stringify(valor);
  return s.length > max ? s.slice(0, max) + '…' : s;
}

export function esEntornoTauri(): boolean {
  return typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== 'undefined';
}

export function crearAdaptadorReal(hooks: HooksAdaptador = {}) {
  let escuchando = false;
  let sesionAbierta = false;
  let claveSesion = '';
  let mensajes: HTMLElement | null = null;
  let onFin: (() => void) | null = null;
  let asistente: AsistenteVivo | null = null;
  let herramienta: HerramientaViva | null = null;
  let ultimoMensaje = '';
  let huboPeticiones = false;
  let cerrado = false;
  let uso: UsoTurno = {
    tokensPrompt: 0,
    tokensComplecion: 0,
    ocupacionPct: null,
    modelo: null,
    maxVentana: null,
    reservaSalida: null,
    totalEntrada: null,
  };
  // [039A-3 P1] Cómo terminó el último turno (para que el pie distinga
  // un corte real de un cancelado/error, que no muestran tokens de fin).
  let ultimoResultado: ResultadoTurno = 'ok';

  function aviso(texto: string, meta: string, detalle: string): void {
    mensajes?.appendChild(crearAvisoSistema(texto, meta, detalle));
    bajarScroll();
  }

  /** [039A-1 04-09 H2] Baja el scroll del chat al fondo (contenedor real). */
  function bajarScroll(): void {
    if (mensajes) mensajes.scrollTop = mensajes.scrollHeight;
  }

  function asistenteVivo(): AsistenteVivo {
    if (!asistente) {
      asistente = crearMensajeAsistenteVivo('');
      mensajes?.appendChild(asistente.raiz);
      bajarScroll();
    }
    herramienta = null;
    return asistente;
  }

  function aplicar(ev: AgenteEvento): void {
    switch (ev.tipo) {
      case 'token': {
        const a = asistenteVivo();
        a.nodo.insertBefore(document.createTextNode(ev.texto), a.cursor);
        // La respuesta fluye bajo el mensaje del usuario: mantener visible.
        bajarScroll();
        break;
      }
      case 'tool_start': {
        herramienta = crearHerramientaViva(iconoDeTool(ev.tool), ev.tool);
        herramienta.ejecutando();
        mensajes?.appendChild(herramienta.raiz);
        bajarScroll();
        asistente = null;
        break;
      }
      case 'tool_result': {
        const detalle = ev.diff ? `${ev.resumen}\n${ev.diff.slice(0, 600)}` : ev.resumen;
        if (herramienta) {
          if (ev.ok) herramienta.completada('ok', { tipo: 'texto', texto: detalle });
          else herramienta.errored('falló', { tipo: 'texto', texto: detalle });
          herramienta = null;
        } else {
          aviso(`${ev.tool} → ${ev.ok ? 'ok' : 'falló'}`, '', detalle);
        }
        break;
      }
      case 'peticion_aprobacion': {
        huboPeticiones = true;
        const tarjeta = crearTarjetaAprobacion({
          titulo: `${ev.tool} (clase: ${ev.clasificacion})`,
          argsTexto: compacto(ev.argumentos),
          onDecidir: (decision, t) => {
            void invoke('responder_aprobacion', { id: ev.id, respuesta: RESPUESTA[decision] })
              .then(() => {
                t.ponerEstado(
                  decision === 'denegar' ? 'denegada por el usuario' : 'aprobada · se ejecuta al reenviar',
                );
                if (decision === 'denegar') t.marcarDenegada();
                t.quitarAcciones();
              })
              .catch((e: unknown) => t.ponerEstado(`no se pudo responder: ${String(e)}`));
          },
        });
        mensajes?.appendChild(tarjeta.raiz);
        break;
      }
      case 'requiere_aprobacion':
        aviso(`${ev.tool} requiere aprobación (tarjeta arriba)`, ev.clasificacion, '');
        break;
      case 'permiso_denegado':
        aviso(`${ev.tool} denegada (${ev.motivo})`, 'permiso', 'el modelo cambia de plan');
        break;
      case 'subagente_inicio':
        aviso(`└ subagente [${ev.perfil}]…`, '', '');
        break;
      case 'subagente_fin':
        aviso(`└ subagente: ${ev.ok ? 'fin' : 'sin resumen'}`, '', '');
        break;
      case 'plan_propuesto':
        aviso(`propuesta del modo plan: ${ev.cambios} cambios pendientes`, 'plan', '');
        break;
      case 'telemetria': {
        const n = Array.isArray(ev.herramientas) ? ev.herramientas.length : 0;
        const parciales = typeof ev.subagentes_parciales === 'number' ? ev.subagentes_parciales : 0;
        aviso(`telemetría: ${n} tools${parciales ? ` · ${parciales} subagentes parciales` : ''}`, '', '');
        break;
      }
      case 'usage':
        uso.tokensPrompt += typeof ev.tokens_prompt === 'number' ? ev.tokens_prompt : 0;
        uso.tokensComplecion += typeof ev.tokens_complecion === 'number' ? ev.tokens_complecion : 0;
        if (typeof ev.ocupacion_pct === 'number') uso.ocupacionPct = ev.ocupacion_pct;
        // [039A-3 P1] El Usage lleva el provider/modelo REAL (tras fallback):
        // se conserva el último que respondió de verdad.
        if (ev.provider && ev.modelo) uso.modelo = `${ev.provider}/${ev.modelo}`;
        // [039A-3 P6] Refresca el indicador en vivo con el % del último uso.
        hooks.onContexto?.({ ...uso });
        break;
      case 'contexto_detalle':
        uso.ocupacionPct = ev.ocupacion_pct;
        uso.maxVentana = ev.max_ventana;
        uso.reservaSalida = ev.reserva_salida;
        uso.totalEntrada = ev.total_entrada;
        // [039A-3 P6] El `ContextoDetalle` es la fuente única del % y la
        // ventana: notifica a la UI para repintar el indicador circular.
        hooks.onContexto?.({ ...uso });
        break;
      case 'contexto':
        break;
      case 'error':
        aviso(`error: ${ev.mensaje}`, ev.retryable ? 'reintentable' : '', '');
        break;
      case 'done':
        asistente = null;
        herramienta = null;
        break;
    }
  }

  async function cerrar(ok: boolean, error?: string): Promise<void> {
    if (cerrado) return;
    cerrado = true;
    ultimoResultado = ok ? 'ok' : 'error';
    if (!ok) aviso(`el turno falló: ${error ?? 'desconocido'}`, '', 'puedes reintentar');
    const fin = onFin;
    onFin = null;
    fin?.();
    // Reenvío tras aprobar (paridad REPL): si hubo peticiones y ya no quedan
    // pendientes, el agente ejecuta lo aprobado sin repetir el mensaje.
    if (ok && huboPeticiones && ultimoMensaje) {
      try {
        const pendientes = await invoke<unknown[]>('pendientes_aprobacion');
        if (pendientes.length === 0) {
          const reintento = ultimoMensaje;
          aviso('aprobaciones resueltas: se reenvía el mensaje', '', '');
          await montar(mensajes, reintento, ultimaOpcion, () => {});
        }
      } catch (e: unknown) {
        aviso(`no se pudo verificar aprobaciones: ${String(e)}`, '', '');
      }
    }
  }

  let ultimaOpcion: OpcionesTurno = { proveedor: '', modelo: '', modo: '', razonamiento: '' };

  /**
   * Abre la sesión si no existe o reconfigura si cambió provider/modelo/modo/
   * razonamiento. Devuelve la info cuando hubo apertura o cambio (`null` = ya
   * estaba acorde, sin roundtrip). La conversación se conserva salvo apertura.
   */
  async function asegurarSesion(opts: OpcionesTurno): Promise<InfoSesion | null> {
    const clave = `${opts.proveedor}|${opts.modelo}|${opts.modo}|${opts.razonamiento}`;
    if (!sesionAbierta) {
      const info = await invoke<InfoSesion>('abrir_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        dir: null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      });
      sesionAbierta = true;
      claveSesion = clave;
      if (info.aviso) aviso(info.aviso, 'persistencia', '');
      hooks.onSesion?.(info);
      return info;
    }
    if (clave !== claveSesion) {
      // Cambio de modelo/modo/razonamiento: reconfigura SIN perder la
      // conversación (abrir_sesion crearía una conversación nueva vacía).
      const info = await invoke<InfoSesion>('reconfigurar_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      });
      claveSesion = clave;
      hooks.onSesion?.(info);
      return info;
    }
    return null;
  }

  async function montar(
    contenedor: HTMLElement | null,
    texto: string,
    opts: OpcionesTurno,
    fin: () => void,
  ): Promise<void> {
    mensajes = contenedor;
    onFin = fin;
    cerrado = false;
    asistente = null;
    herramienta = null;
    huboPeticiones = false;
    ultimoResultado = 'ok';
    uso = {
      tokensPrompt: 0,
      tokensComplecion: 0,
      ocupacionPct: null,
      modelo: null,
      maxVentana: null,
      reservaSalida: null,
      totalEntrada: null,
    };
    ultimaOpcion = opts;
    ultimoMensaje = texto;
    mensajes?.appendChild(crearMensajeUsuario(texto));
    // [039A-1 04-09 H2] El mensaje del usuario debe verse al enviar: baja el
    // scroll al final (puede que el contenedor no estuviera al fondo).
    if (mensajes) mensajes.scrollTop = mensajes.scrollHeight;
    try {
      if (!escuchando) {
        await listen<AgenteEvento>('agente-evento', (e) => aplicar(e.payload));
        await listen<{ ok: boolean; error?: string }>('turno-fin', (e) =>
          void cerrar(e.payload.ok, e.payload.error),
        );
        escuchando = true;
      }
      await asegurarSesion(opts);
      await invoke('enviar_turno', { mensaje: texto, panel_id: opts.panelId ?? null });
    } catch (e: unknown) {
      await cerrar(false, String(e));
    }
  }

  function detener(): void {
    // El backend aborta, marca el turno `cancelado` y emite `turno-fin`
    // (ok:false). El cierre local es optimista; el `turno-fin` tardío se
    // ignora por el flag `cerrado`. [039A-3 P5] Cancela el turno del panel
    // que lanzó `montar` (el adaptador guarda la última `OpcionesTurno`).
    void invoke('cancelar_turno', { panel_id: ultimaOpcion.panelId ?? null }).catch(() => {});
    if (!cerrado) {
      cerrado = true;
      ultimoResultado = 'cancelado';
      const fin = onFin;
      onFin = null;
      const n = el('div', 'msg-asis');
      n.textContent = '[turno cancelado por el usuario]';
      mensajes?.appendChild(n);
      fin?.();
    }
  }

  /** Uso acumulado del turno en curso (tokens reales del núcleo). */
  function usoUltimoTurno(): UsoTurno {
    return { ...uso };
  }

  /** [039A-3 P1] Cómo terminó el último turno (`ok`|`error`|`cancelado`). */
  function resultadoUltimoTurno(): ResultadoTurno {
    return ultimoResultado;
  }

  return {
    montar,
    detener,
    usoUltimoTurno,
    resultadoUltimoTurno,
    /** Abre la sesión si aún no existe (para listar/cargar al arrancar). */
    asegurarSesion,
    sesion: {
      /** [039A-3 P5] Crea una conversación en el panel dado (default
       * 'principal'). Devuelve la conversación nueva. */
      async nueva(titulo?: string, panelId?: string): Promise<InfoConversacion> {
        const conv = await invoke<InfoConversacion>('conversacion_nueva', {
          titulo: titulo ?? null,
          panel_id: panelId ?? null,
        });
        return conv;
      },
      async listar(): Promise<InfoConversacion[]> {
        return invoke<InfoConversacion[]>('listar_conversaciones');
      },
      /** [039A-3 P5] Carga una conversación en el panel dado. */
      async cargar(id: string, panelId?: string): Promise<CargaConversacion> {
        return invoke<CargaConversacion>('cargar_conversacion', {
          id,
          panel_id: panelId ?? null,
        });
      },
      async renombrar(id: string, titulo: string): Promise<boolean> {
        return invoke<boolean>('renombrar_conversacion', { id, titulo });
      },
      async archivar(id: string, archivada: boolean): Promise<boolean> {
        return invoke<boolean>('archivar_conversacion', { id, archivada });
      },
      /** [039A-3 P5] Si era la actual del panel, el backend crea una nueva en
       * ese panel y la devuelve. */
      async eliminar(id: string, panelId?: string): Promise<InfoConversacion> {
        return invoke<InfoConversacion>('eliminar_conversacion', {
          id,
          panel_id: panelId ?? null,
        });
      },
      /** [039A-3 P2] Borra el hilo posterior a un mensaje de usuario y
       * devuelve la conversación recién recortada. `editar=false` conserva el
       * mensaje objetivo ("volver a este punto"); `editar=true` lo borra
       * también (se reescribe al reenviar desde el modo edición).
       * [039A-3 P5] Opera sobre el panel dado. */
      async rewind(
        hastaMensajeId: string,
        editar: boolean,
        panelId?: string,
      ): Promise<CargaConversacion> {
        return invoke<CargaConversacion>('rewind_conversacion', {
          hastaMensajeId,
          editar,
          panel_id: panelId ?? null,
        });
      },
      /** [039A-3 P3] Restaura los archivos del último tramo rebobinado (acción
       * EXPLÍCITA tras "volver a punto"). Falla si no hay tramo pendiente.
       * [039A-3 P5] Opera sobre el tramo del panel dado. */
      async restaurarTramo(panelId?: string): Promise<ResultadoRestauracionTramo> {
        return invoke<ResultadoRestauracionTramo>('restaurar_archivos_tramo', {
          panel_id: panelId ?? null,
        });
      },
      async proveedores(): Promise<ProveedorInfo[]> {
        return invoke<ProveedorInfo[]>('proveedores_disponibles');
      },
      async configLeer(clave: string): Promise<string | null> {
        return invoke<string | null>('config_leer', { clave });
      },
      async configGuardar(clave: string, valor: string): Promise<void> {
        await invoke('config_guardar', { clave, valor });
      },
      /**
       * Diálogo nativo de carpeta. Reabre la sesión (conversación nueva).
       * Si el usuario cancela, devuelve la sesión actual sin cambios.
       */
      async elegirWorkspace(): Promise<InfoSesion> {
        const info = await invoke<InfoSesion>('elegir_workspace');
        sesionAbierta = true;
        hooks.onSesion?.(info);
        return info;
      },
      async actualizarMeta(meta: string | null): Promise<string | null> {
        return invoke<string | null>('actualizar_meta', { meta });
      },
    },
  };
}

export type AdaptadorReal = ReturnType<typeof crearAdaptadorReal>;
