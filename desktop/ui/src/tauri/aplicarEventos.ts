/* Render de eventos del turno (`aplicar`): un `AgenteEvento` → DOM.
 * Estado mutable explícito (`EstadoTurno`) + dependencias inyectadas
 * (`EventosDeps`); sin closure sobre el adaptador. */
import {
  crearHerramientaViva,
  crearRazonamientoCerrado,
  crearRazonamientoVivo,
  crearTarjetaAprobacion,
  formatearResultadoHerramienta,
  pintarLogroEnPie,
  tokensCortos,
  type AsistenteVivo,
  type HerramientaViva,
  type RazonamientoVivo,
} from '../componentes/mensajes';
import { crearTareasViva, type TareasViva } from '../componentes/tareasMeta';
import { notificarAprobacion } from '../componentes/notificacionSistema';
import type { DecisionAprobacion } from '../dominio/tipos';
import { compacto, descripcionDeTool, iconoDeTool, rutaDeArgumentos } from './descripcionHerramientas';
import type { AgenteEvento, HooksAdaptador, Transporte, UsoTurno } from './realTipos';

const RESPUESTA: Record<DecisionAprobacion, string> = {
  aprobar: 'aprobar',
  permitir: 'siempre',
  denegar: 'rechazar',
};

/** Parte mutable del turno que `aplicarEvento` lee/escribe. */
export interface EstadoTurno {
  herramienta: HerramientaViva | null;
  rutaHerramienta: string | null;
  uso: UsoTurno;
  huboPeticiones: boolean;
  /** [109A-5 F2] Bloque del plan visible. Sobrevive al fin del turno a
   * propósito: el plan es de la CONVERSACIÓN, así que el mismo bloque se
   * actualiza en los turnos siguientes (resume) en vez de duplicarse. */
  tareas: TareasViva | null;
  /** [109A-5 F3] Id del último turno cerrado (llega en `done`). El evento
   * `meta_lograda` puede llegar DESPUÉS del cierre, así que hace falta el id
   * para localizar el pie de ese turno y anclar ahí el badge. */
  ultimoTurnoId: string | null;
  /** [129A-2] Bloque de pensamiento en curso (hay `razonamiento_delta` sin su
   * `razonamiento` de cierre todavía). Sobrevive entre eventos del mismo
   * turno; se cierra en `razonamiento`/`done`/`error` o al detener. */
  razonamiento: {
    bloque: RazonamientoVivo;
    inicioMs: number;
    chars: number;
  } | null;
}

/** Lo que el render necesita del adaptador (DOM + hooks + transporte). */
export interface EventosDeps {
  hooks: HooksAdaptador;
  transporte: Transporte;
  /** Getter: el contenedor cambia en cada `montar` (paneles), no se captura. */
  mensajes(): HTMLElement | null;
  aviso(texto: string, meta: string, detalle: string): void;
  bajarScroll(): void;
  asistenteVivo(): AsistenteVivo;
  olvidarAsistente(): void;
}

export function aplicarEvento(ev: AgenteEvento, st: EstadoTurno, d: EventosDeps): void {
  switch (ev.tipo) {
    case 'token': {
      const a = d.asistenteVivo();
      a.nodo.insertBefore(document.createTextNode(ev.texto), a.cursor);
      // La respuesta fluye bajo el mensaje del usuario: mantener visible.
      d.bajarScroll();
      break;
    }
    case 'razonamiento': {
      /* [129A-1] El pensamiento llega completo al final de la llamada: sin
       * deltas previos se pinta como summary cerrado. [129A-2] Con deltas
       * previos hay un bloque vivo abierto: se cierra con el total real. */
      if (st.razonamiento) {
        finalizarRazonamiento(st, ev.texto);
      } else if (ev.texto.trim()) {
        d.mensajes()?.appendChild(crearRazonamientoCerrado(ev.texto, 'razonamiento'));
        d.bajarScroll();
      }
      break;
    }
    case 'razonamiento_delta': {
      /* [129A-2] Pensamiento en vivo: al primer delta se abre el summary con
       * spinner; cada delta anexa texto y refresca el contador estimado. */
      if (!st.razonamiento) {
        const bloque = crearRazonamientoVivo();
        d.mensajes()?.appendChild(bloque.raiz);
        st.razonamiento = { bloque, inicioMs: Date.now(), chars: 0 };
      }
      const r = st.razonamiento;
      if (ev.texto) {
        r.bloque.nodoResultado.appendChild(document.createTextNode(ev.texto));
        r.chars += ev.texto.length;
        r.bloque.fijarProgreso(`~${tokensCortos(r.chars / 4)} tok`);
      }
      d.bajarScroll();
      break;
    }
    case 'tool_start': {
      st.herramienta = crearHerramientaViva(iconoDeTool(ev.tool), descripcionDeTool(ev.tool, ev.argumentos));
      st.herramienta.ejecutando();
      d.mensajes()?.appendChild(st.herramienta.raiz);
      d.bajarScroll();
      d.olvidarAsistente();
      // [089A-2] Recuerda la ruta si es escritura/parche de archivo.
      st.rutaHerramienta =
        ev.tool === 'file_write' || ev.tool === 'file_patch'
          ? rutaDeArgumentos(ev.argumentos)
          : null;
      break;
    }
    case 'tool_result': {
      const dif = ev.diff ?? null;
      if (st.herramienta) {
        const detalle = formatearResultadoHerramienta(ev.resumen, dif);
        if (ev.ok) st.herramienta.completada('ok', { tipo: 'html', html: detalle });
        else st.herramienta.errored('falló', { tipo: 'html', html: detalle });
        st.herramienta = null;
      } else {
        // Sin bloque de herramienta: aviso en texto plano (crearAvisoSistema usa textContent).
        d.aviso(`${ev.tool} → ${ev.ok ? 'ok' : 'falló'}`, '', dif ? `${ev.resumen}\n${dif}` : ev.resumen);
      }
      // [089A-2] Notifica el cambio al visor (solo escrituras con ruta y diff).
      if (
        (ev.tool === 'file_write' || ev.tool === 'file_patch') &&
        st.rutaHerramienta &&
        dif
      ) {
        d.hooks.onCambioArchivo?.({
          origen: 'tool',
          tool: ev.tool,
          ruta: st.rutaHerramienta,
          titulo: descripcionDeTool(ev.tool, undefined),
          resumen: ev.resumen,
          diff: dif,
        });
      }
      st.rutaHerramienta = null;
      break;
    }
    case 'peticion_aprobacion': {
      st.huboPeticiones = true;
      const tarjeta = crearTarjetaAprobacion({
        titulo: `${ev.tool} (clase: ${ev.clasificacion})`,
        argsTexto: compacto(ev.argumentos),
        onDecidir: (decision, t) => {
          void d.transporte
            .responderAprobacion(ev.id, RESPUESTA[decision])
            .then(() => {
              t.ponerEstado(
                decision === 'denegar' ? 'denegada por el usuario' : 'aprobada · se ejecuta al reenviar',
              );
              if (decision === 'denegar') t.marcarDenegada();
              t.quitarAcciones();
              // [fix 12-09] La tarjeta decidida vuelve al flujo como registro;
              // el slot fijo queda libre para las peticiones pendientes.
              d.mensajes()?.appendChild(tarjeta.raiz);
              d.bajarScroll();
              // [fix 12-09] Si el turno ya cerró (decisión tardía), el cierre
              // no pudo reenviar (aún había pendientes): lo intenta ahora el
              // orquestador, que reenvía el último mensaje como turno nuevo.
              if (d.transporte.requiereReenvioTrasAprobar()) d.hooks.onAprobacionResuelta?.();
            })
            .catch((e: unknown) => t.ponerEstado(`no se pudo responder: ${String(e)}`));
        },
      });
      // [fix 12-09] La tarjeta pendiente vive fija encima de la caja (slot
      // `.aprobaciones-fijas`, como la caja de meta), no perdida en el flujo.
      const flujo = d.mensajes();
      const slot = flujo?.closest('.chat')?.querySelector('.aprobaciones-fijas');
      (slot ?? flujo)?.appendChild(tarjeta.raiz);
      d.bajarScroll();
      // [fix 12-09] Toast del sistema: el usuario puede estar en otra ventana
      // de Windows y la tarjeta fija no se ve. Nunca bloquea el turno.
      void notificarAprobacion(ev.tool, `clase ${ev.clasificacion}: ${compacto(ev.argumentos)}`);
      break;
    }
    case 'requiere_aprobacion':
      d.aviso(`${ev.tool} requiere aprobación (tarjeta arriba)`, ev.clasificacion, '');
      break;
    case 'permiso_denegado':
      d.aviso(`${ev.tool} denegada (${ev.motivo})`, 'permiso', 'el modelo cambia de plan');
      break;
    case 'subagente_inicio':
      d.aviso(`└ subagente [${ev.perfil}]…`, '', '');
      break;
    case 'subagente_fin':
      d.aviso(`└ subagente: ${ev.ok ? 'fin' : 'sin resumen'}`, '', '');
      break;
    case 'plan_propuesto':
      d.aviso(`propuesta del modo plan: ${ev.cambios} cambios pendientes`, 'plan', '');
      break;
    case 'tareas_actualizadas': {
      /* [109A-5 F2] Un solo bloque por conversación: si el nodo ya no está
       * montado (cambió el contenedor de mensajes) se crea uno nuevo; si sigue
       * vivo, `appendChild` lo MUEVE al final del transcript, junto al último
       * mensaje, para que el plan vigente no quede perdido arriba mientras el
       * modelo lo actualiza. */
      if (!st.tareas || !st.tareas.montado()) {
        st.tareas = crearTareasViva();
      }
      st.tareas.actualizar(ev.items);
      d.mensajes()?.appendChild(st.tareas.raiz);
      d.bajarScroll();
      break;
    }
    case 'usage':
      st.uso.tokensPrompt += typeof ev.tokens_prompt === 'number' ? ev.tokens_prompt : 0;
      st.uso.tokensComplecion += typeof ev.tokens_complecion === 'number' ? ev.tokens_complecion : 0;
      if (typeof ev.ocupacion_pct === 'number') st.uso.ocupacionPct = ev.ocupacion_pct;
      // [039A-3 P1] El Usage lleva el provider/modelo REAL (tras fallback):
      // se conserva el último que respondió de verdad.
      if (ev.provider && ev.modelo) st.uso.modelo = `${ev.provider}/${ev.modelo}`;
      // [039A-3 P6] Refresca el indicador en vivo con el % del último uso.
      d.hooks.onContexto?.({ ...st.uso });
      break;
    case 'contexto_detalle':
      st.uso.ocupacionPct = ev.ocupacion_pct;
      st.uso.maxVentana = ev.max_ventana;
      st.uso.reservaSalida = ev.reserva_salida;
      st.uso.totalEntrada = ev.total_entrada;
      // [039A-3 P6] El `ContextoDetalle` es la fuente única del % y la
      // ventana: notifica a la UI para repintar el indicador circular.
      d.hooks.onContexto?.({ ...st.uso });
      break;
    case 'contexto':
      break;
    case 'error':
      finalizarRazonamiento(st);
      d.aviso(`error: ${ev.mensaje}`, ev.retryable ? 'reintentable' : '', '');
      break;
    case 'done':
      finalizarRazonamiento(st);
      d.olvidarAsistente();
      st.herramienta = null;
      st.ultimoTurnoId = ev.turno_id;
      break;
    case 'meta_lograda': {
      /* [109A-5 F3] El logro se ancla al pie del turno que lo respalda
       * (`turno_id`), no al final del transcript: con varios turnos abiertos,
       * un badge suelto no diría QUÉ turno cerró la meta. Se busca el pie por
       * su `data-turno` en vez de por selector CSS para no depender del
       * escapado del id. */
      const contenedor = d.mensajes();
      const pie = Array.from(contenedor?.querySelectorAll<HTMLElement>('.pie-turno') ?? []).find(
        (nodo) => nodo.dataset.turno === ev.turno_id,
      );
      if (pie) {
        pintarLogroEnPie(pie, ev);
        d.bajarScroll();
        break;
      }
      /* Sin pie donde anclarlo (ese turno terminó en error/cancelado, que no
       * pinta pie, o el logro es de otra conversación): el aviso evita que el
       * cierre de la meta desaparezca en silencio. El historial durable sigue
       * en el panel meta. */
      d.aviso(`meta lograda: ${ev.meta}`, 'sin pie de turno donde anclarla', '');
      break;
    }
    case 'meta_pausada_por_bloqueo': {
      /* [109A-5 F4] El reloj se detuvo solo: nadie pulsó pausar, así que sin
       * este aviso la meta parecería colgada. El motivo va en el texto porque
       * es lo único que el usuario necesita para desatascarla. El panel meta
       * se relee al cerrar el turno (`VistaMeta.notificarTurnoFin`), así que
       * aquí solo se cuenta QUÉ pasó. */
      d.aviso(
        `meta en pausa: ${ev.motivo}`,
        `${ev.turnos} turnos bloqueado sin avanzar`,
        'reanúdala cuando lo desbloquees',
      );
      break;
    }
    case 'tool_navegador':
      d.hooks.onToolNavegador?.(ev);
      break;
  }
}

/** Uso vacío inicial (se resetea en cada `montar`). */
export function usoVacio(): UsoTurno {
  return {
    tokensPrompt: 0,
    tokensComplecion: 0,
    ocupacionPct: null,
    modelo: null,
    maxVentana: null,
    reservaSalida: null,
    totalEntrada: null,
    velocidadTokS: null,
  };
}

/**
 * [129A-2] Cierra el bloque de pensamiento en curso (si lo hay): fija el
 * texto completo cuando se conoce (cura deltas perdidos por un canal lleno)
 * y el meta con total estimado + segundos reales. Sin bloque, no-op. También
 * la usa `detener` (corte del usuario sin `done` del backend).
 */
export function finalizarRazonamiento(st: EstadoTurno, texto?: string): void {
  const r = st.razonamiento;
  if (!r) return;
  st.razonamiento = null;
  const final = texto && texto.trim() ? texto : r.bloque.nodoResultado.textContent ?? '';
  r.bloque.nodoResultado.textContent = final;
  const seg = Math.max(0, (Date.now() - r.inicioMs) / 1000);
  r.bloque.terminar(`~${tokensCortos(final.length / 4)} tok · ${seg.toFixed(1)} s`);
}
