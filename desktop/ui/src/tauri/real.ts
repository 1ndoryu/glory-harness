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
 * Contrato AgenteEvento del núcleo (tag `evento`, snake_case). Fiel a
 * `core/src/evento.rs`: los campos opcionales pueden no venir según el
 * proveedor; el adaptador nunca asume presentes los no obligatorios.
 */
export type AgenteEvento =
  | { evento: 'token'; texto: string }
  | { evento: 'tool_start'; tool: string; argumentos: unknown }
  | { evento: 'tool_result'; tool: string; ok: boolean; resumen: string; diff?: string | null }
  | { evento: 'peticion_aprobacion'; id: string; tool: string; argumentos: unknown; clasificacion: string }
  | { evento: 'requiere_aprobacion'; tool: string; clasificacion: string }
  | { evento: 'permiso_denegado'; tool: string; motivo: string }
  | { evento: 'subagente_inicio'; perfil: string }
  | { evento: 'subagente_fin'; ok: boolean }
  | { evento: 'plan_propuesto'; cambios: number }
  | {
      evento: 'usage';
      tokens_prompt: number;
      tokens_complecion: number;
      ocupacion_pct?: number | null;
      provider?: string | null;
      modelo?: string | null;
    }
  | { evento: 'contexto'; skills: number }
  | {
      evento: 'contexto_detalle';
      max_ventana: number;
      reserva_salida: number;
      system_instrucciones: number;
      definiciones_tools: number;
      mensajes: number;
      resultados_tools: number;
      total_entrada: number;
      ocupacion_pct: number;
    }
  | { evento: 'telemetria'; subagentes_parciales: number; herramientas: Array<{ tool: string }> }
  | { evento: 'error'; mensaje: string; retryable: boolean }
  | { evento: 'done'; turno_id: string };

export interface OpcionesTurno {
  proveedor: string;
  modelo: string;
  modo: string;
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

export interface CargaConversacion {
  id: string;
  titulo: string;
  mensajes: MensajeGuardado[];
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
}

export interface HooksAdaptador {
  /** Se llama con cada `abrir_sesion`/`reconfigurar`/`elegir_workspace`. */
  onSesion?: (info: InfoSesion) => void;
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
  let uso: UsoTurno = { tokensPrompt: 0, tokensComplecion: 0, ocupacionPct: null };

  function aviso(texto: string, meta: string, detalle: string): void {
    mensajes?.appendChild(crearAvisoSistema(texto, meta, detalle));
  }

  function asistenteVivo(): AsistenteVivo {
    if (!asistente) {
      asistente = crearMensajeAsistenteVivo('');
      mensajes?.appendChild(asistente.raiz);
    }
    herramienta = null;
    return asistente;
  }

  function aplicar(ev: AgenteEvento): void {
    switch (ev.evento) {
      case 'token': {
        const a = asistenteVivo();
        a.nodo.insertBefore(document.createTextNode(ev.texto), a.cursor);
        break;
      }
      case 'tool_start': {
        herramienta = crearHerramientaViva(iconoDeTool(ev.tool), ev.tool);
        herramienta.ejecutando();
        mensajes?.appendChild(herramienta.raiz);
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
        break;
      case 'contexto_detalle':
        uso.ocupacionPct = ev.ocupacion_pct;
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

  let ultimaOpcion: OpcionesTurno = { proveedor: '', modelo: '', modo: '' };

  /**
   * Abre la sesión si no existe o reconfigura si cambió provider/modelo/modo.
   * Devuelve la info cuando hubo apertura o cambio (`null` = ya estaba
   * acorde, sin roundtrip). La conversación se conserva salvo apertura.
   */
  async function asegurarSesion(opts: OpcionesTurno): Promise<InfoSesion | null> {
    const clave = `${opts.proveedor}|${opts.modelo}|${opts.modo}`;
    if (!sesionAbierta) {
      const info = await invoke<InfoSesion>('abrir_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        dir: null,
        modo: opts.modo || null,
      });
      sesionAbierta = true;
      claveSesion = clave;
      if (info.aviso) aviso(info.aviso, 'persistencia', '');
      hooks.onSesion?.(info);
      return info;
    }
    if (clave !== claveSesion) {
      // Cambio de modelo/modo: reconfigura SIN perder la conversación
      // (abrir_sesion crearía una conversación nueva vacía).
      const info = await invoke<InfoSesion>('reconfigurar_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        modo: opts.modo || null,
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
    uso = { tokensPrompt: 0, tokensComplecion: 0, ocupacionPct: null };
    ultimaOpcion = opts;
    ultimoMensaje = texto;
    mensajes?.appendChild(crearMensajeUsuario(texto));
    try {
      if (!escuchando) {
        await listen<AgenteEvento>('agente-evento', (e) => aplicar(e.payload));
        await listen<{ ok: boolean; error?: string }>('turno-fin', (e) =>
          void cerrar(e.payload.ok, e.payload.error),
        );
        escuchando = true;
      }
      await asegurarSesion(opts);
      await invoke('enviar_turno', { mensaje: texto });
    } catch (e: unknown) {
      await cerrar(false, String(e));
    }
  }

  function detener(): void {
    // El backend aborta, marca el turno `cancelado` y emite `turno-fin`
    // (ok:false). El cierre local es optimista; el `turno-fin` tardío se
    // ignora por el flag `cerrado`.
    void invoke('cancelar_turno').catch(() => {});
    if (!cerrado) {
      cerrado = true;
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

  return {
    montar,
    detener,
    usoUltimoTurno,
    /** Abre la sesión si aún no existe (para listar/cargar al arrancar). */
    asegurarSesion,
    sesion: {
      async nueva(titulo?: string): Promise<InfoConversacion> {
        const conv = await invoke<InfoConversacion>('conversacion_nueva', { titulo: titulo ?? null });
        return conv;
      },
      async listar(): Promise<InfoConversacion[]> {
        return invoke<InfoConversacion[]>('listar_conversaciones');
      },
      async cargar(id: string): Promise<CargaConversacion> {
        return invoke<CargaConversacion>('cargar_conversacion', { id });
      },
      async renombrar(id: string, titulo: string): Promise<boolean> {
        return invoke<boolean>('renombrar_conversacion', { id, titulo });
      },
      async archivar(id: string, archivada: boolean): Promise<boolean> {
        return invoke<boolean>('archivar_conversacion', { id, archivada });
      },
      /** Si era la actual, el backend crea una nueva y la devuelve. */
      async eliminar(id: string): Promise<InfoConversacion> {
        return invoke<InfoConversacion>('eliminar_conversacion', { id });
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
