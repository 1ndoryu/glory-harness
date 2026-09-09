/* Render de eventos del turno (`aplicar`): un `AgenteEvento` → DOM.
 * Estado mutable explícito (`EstadoTurno`) + dependencias inyectadas
 * (`EventosDeps`); sin closure sobre el adaptador. */
import {
  crearHerramientaViva,
  crearTarjetaAprobacion,
  formatearResultadoHerramienta,
  type AsistenteVivo,
  type HerramientaViva,
} from '../componentes/mensajes';
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
            })
            .catch((e: unknown) => t.ponerEstado(`no se pudo responder: ${String(e)}`));
        },
      });
      d.mensajes()?.appendChild(tarjeta.raiz);
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
      d.aviso(`error: ${ev.mensaje}`, ev.retryable ? 'reintentable' : '', '');
      break;
    case 'done':
      d.olvidarAsistente();
      st.herramienta = null;
      break;
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
  };
}
