// ============================================================
// Adaptador real Tauri (plan 039A-1, Fase 4): la UI habla al núcleo
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

/** Contrato AgenteEvento del núcleo (tag `evento`, snake_case). */
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
  | { evento: 'usage'; ocupacion_pct?: number | null }
  | { evento: 'contexto_detalle'; ocupacion_pct: number }
  | { evento: 'telemetria'; motivo_cierre: string; herramientas: unknown[] }
  | { evento: 'error'; mensaje: string }
  | { evento: 'contexto' }
  | { evento: 'done' };

export interface OpcionesTurno {
  proveedor: string;
  modelo: string;
  modo: string;
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

export function crearAdaptadorReal() {
  let escuchando = false;
  let claveSesion = '';
  let mensajes: HTMLElement | null = null;
  let onFin: (() => void) | null = null;
  let asistente: AsistenteVivo | null = null;
  let herramienta: HerramientaViva | null = null;
  let ultimoMensaje = '';
  let huboPeticiones = false;
  let cerrado = false;

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
      case 'telemetria':
        aviso(`telemetría: ${ev.motivo_cierre} · ${ev.herramientas.length} tools`, '', '');
        break;
      case 'usage':
        if (typeof ev.ocupacion_pct === 'number') {
          aviso(`contexto al ${ev.ocupacion_pct.toFixed(1)}%`, 'uso', '');
        }
        break;
      case 'contexto_detalle':
        aviso(`contexto al ${ev.ocupacion_pct.toFixed(1)}%`, 'uso', '');
        break;
      case 'error':
        aviso(`error: ${ev.mensaje}`, 'reintentable', '');
        break;
      case 'contexto':
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
      const clave = `${opts.proveedor}|${opts.modelo}|${opts.modo}`;
      if (clave !== claveSesion) {
        await invoke('abrir_sesion', {
          provider: opts.proveedor || null,
          modelo: opts.modelo || null,
          dir: null,
          modo: opts.modo || null,
        });
        claveSesion = clave;
      }
      await invoke('enviar_turno', { mensaje: texto });
    } catch (e: unknown) {
      await cerrar(false, String(e));
    }
  }

  function detener(): void {
    // El abort del backend no emite turno-fin: se cierra en local.
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

  return { montar, detener };
}
