/* Núcleo del turno (montar/detener/cierre/sesión): el estado vivo del
 * adaptador —contenedor, asistente en curso, uso— más el ciclo abrir →
 * enviar → escuchar → cerrar (+ reenvío tras aprobar). La fachada de
 * sesión/conversaciones vive en `real.ts`; el render de eventos en
 * `aplicarEventos.ts`. */
import {
  crearAvisoSistema,
  crearMensajeAsistenteVivo,
  crearMensajePendiente,
  crearMensajeUsuario,
  type AsistenteVivo,
} from '../componentes/mensajes';
import { el } from '../util/dom';
import type { CambioArchivoPanel } from '../componentes/panelChatTipos';
import { aplicarEvento, finalizarRazonamiento, usoVacio, type EstadoTurno } from './aplicarEventos';
import type {
  HooksAdaptador,
  InfoSesion,
  OpcionesTurno,
  ResultadoTurno,
  Transporte,
  UsoTurno,
} from './realTipos';

export interface TurnoReal {
  montar(contenedor: HTMLElement | null, texto: string, opts: OpcionesTurno, fin: () => void): Promise<void>;
  detener(): void;
  /** Uso acumulado del turno en curso (tokens reales del núcleo). */
  usoUltimoTurno(): UsoTurno;
  /** [039A-3 P1] Cómo terminó el último turno (`ok`|`error`|`cancelado`). */
  resultadoUltimoTurno(): ResultadoTurno;
  /** [109A-5 F3] Id del último turno cerrado (`done`), o `null` si aún no
   * cerró ninguno. Lo usa `lograr`: el logro queda respaldado por el turno que
   * hizo el trabajo, y sin id el backend lo rechaza (`turno_requerido`) en vez
   * de inventarse una referencia. */
  ultimoTurnoId(): string | null;
  /** [129A-8] Escrituras con éxito del último turno, para su resumen. */
  cambiosUltimoTurno(): CambioArchivoPanel[];
  /** Abre la sesión si aún no existe (para listar/cargar al arrancar). */
  asegurarSesion(opts: OpcionesTurno): Promise<InfoSesion | null>;
  /** Marca sesión abierta tras elegir/fijar workspace o guardar proyecto. */
  avisarSesionAbierta(info: InfoSesion): void;
}

export function crearTurnoReal(hooks: HooksAdaptador, transporte: Transporte): TurnoReal {
  let escuchando = false;
  let sesionAbierta = false;
  let claveSesion = '';
  let mensajes: HTMLElement | null = null;
  let onFin: (() => void) | null = null;
  let asistente: AsistenteVivo | null = null;
  /** "pensando…" visible desde el envío hasta el primer evento (o el fin). */
  let pendiente: HTMLElement | null = null;
  let cerrado = false;
  let uso: UsoTurno = usoVacio();
  // [039A-3 P1] Cómo terminó el último turno (para que el pie distinga
  // un corte real de un cancelado/error, que no muestran tokens de fin).
  let ultimoResultado: ResultadoTurno = 'ok';
  const estado: EstadoTurno = {
    herramienta: null,
    rutaHerramienta: null,
    uso,
    cambiosResumen: [],
    tareas: null,
    ultimoTurnoId: null,
    razonamiento: null,
  };
  let ultimaOpcion: OpcionesTurno = { proveedor: '', modelo: '', modo: '', razonamiento: '' };

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
    estado.herramienta = null;
    return asistente;
  }

  function olvidarAsistente(): void {
    // [fix 12-09] El caret es un nodo real del DOM: hay que quitarlo al
    // cerrar el mensaje; si no, el cuadrito negro sigue titilando en todos
    // los mensajes ya terminados. `.remove()` sobre nodo huérfano es no-op.
    asistente?.cursor.remove();
    asistente = null;
  }

  function quitarPendiente(): void {
    pendiente?.remove();
    pendiente = null;
  }

  async function cerrar(ok: boolean, error?: string): Promise<void> {
    if (cerrado) return;
    cerrado = true;
    quitarPendiente();
    olvidarAsistente();
    ultimoResultado = ok ? 'ok' : 'error';
    if (!ok) aviso(`el turno falló: ${error ?? 'desconocido'}`, '', 'puedes reintentar');
    const fin = onFin;
    onFin = null;
    fin?.();
  }

  /**
   * Abre la sesión si no existe o reconfigura si cambió provider/modelo/modo/
   * razonamiento. Devuelve la info cuando hubo apertura o cambio (`null` = ya
   * estaba acorde, sin roundtrip). La conversación se conserva salvo apertura.
   */
  async function asegurarSesion(opts: OpcionesTurno): Promise<InfoSesion | null> {
    const clave = `${opts.proveedor}|${opts.modelo}|${opts.modo}|${opts.razonamiento}`;
    if (!sesionAbierta) {
      const info = await transporte.abrirSesion(opts);
      sesionAbierta = true;
      claveSesion = clave;
      if (info.aviso) aviso(info.aviso, 'persistencia', '');
      hooks.onSesion?.(info);
      return info;
    }
    if (clave !== claveSesion) {
      // Cambio de modelo/modo/razonamiento: reconfigura SIN perder la
      // conversación (abrir_sesion crearía una conversación nueva vacía).
      const info = await transporte.reconfigurarSesion(opts);
      claveSesion = clave;
      hooks.onSesion?.(info);
      return info;
    }
    return null;
  }

  function avisarSesionAbierta(info: InfoSesion): void {
    sesionAbierta = true;
    hooks.onSesion?.(info);
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
    quitarPendiente();
    estado.herramienta = null;
    estado.razonamiento = null;
    estado.rutaHerramienta = null;
    estado.cambiosResumen = [];
    ultimoResultado = 'ok';
    uso = usoVacio();
    estado.uso = uso;
    ultimaOpcion = opts;
    mensajes?.appendChild(crearMensajeUsuario(texto));
    // El turno puede tardar (proveedor, SSE, herramientas) antes del primer
    // evento: "pensando…" inmediato para no dejar el hueco vacío. Lo releva
    // el primer evento (`quitarPendiente` en `aplicarEvento`) o el cierre.
    pendiente = crearMensajePendiente();
    mensajes?.appendChild(pendiente);
    // [039A-1 04-09 H2] El mensaje del usuario debe verse al enviar: baja el
    // scroll al final (puede que el contenedor no estuviera al fondo).
    if (mensajes) mensajes.scrollTop = mensajes.scrollHeight;
    try {
      // La sesión (sid/cookie) debe existir ANTES de suscribirse: en modo
      // web el SSE cuelga de `/api/v1/session/{sid}/events` y suscribirse
      // con el sid aún vacío fijaba una fuente muerta (sin realtime y sin
      // fin de turno). En Tauri el orden es indiferente (solo suscribe).
      await asegurarSesion(opts);
      if (!escuchando) {
        await transporte.escucharTurno(
          (ev) => aplicarEvento(ev, estado, {
            hooks,
            transporte,
            mensajes: () => mensajes,
            aviso,
            bajarScroll,
            asistenteVivo,
            olvidarAsistente,
            quitarPendiente,
          }),
          (ok, error) => void cerrar(ok, error),
        );
        escuchando = true;
      }
      await transporte.enviarTurno(texto, opts.panelId ?? null, opts.soloLectura === true);
    } catch (e: unknown) {
      await cerrar(false, String(e));
    }
  }

  function detener(): void {
    // El backend aborta, marca el turno `cancelado` y emite `turno-fin`
    // (ok:false). El cierre local es optimista; el `turno-fin` tardío se
    // ignora por el flag `cerrado`. [039A-3 P5] Cancela el turno del panel
    // que lanzó `montar` (el adaptador guarda la última `OpcionesTurno`).
    transporte.detenerTurno(ultimaOpcion.panelId ?? null);
    if (!cerrado) {
      cerrado = true;
      quitarPendiente();
      ultimoResultado = 'cancelado';
      // [129A-2] Sin `done` del backend no hay cierre del summary: se fija
      // con lo acumulado para no dejar el spinner colgado.
      finalizarRazonamiento(estado);
      // [fix 12-09] El mensaje vivo queda sin `done`: apaga su caret.
      olvidarAsistente();
      const fin = onFin;
      onFin = null;
      const n = el('div', 'msg-asis');
      n.textContent = '[turno cancelado por el usuario]';
      mensajes?.appendChild(n);
      fin?.();
    }
  }

  return {
    montar,
    detener,
    usoUltimoTurno(): UsoTurno {
      return { ...uso };
    },
    resultadoUltimoTurno(): ResultadoTurno {
      return ultimoResultado;
    },
    ultimoTurnoId(): string | null {
      return estado.ultimoTurnoId;
    },
    /** [129A-8] Escrituras con éxito del último turno (copia: el resumen
     * las lee al cerrar, cuando el estado ya puede estar reseteándose). */
    cambiosUltimoTurno(): CambioArchivoPanel[] {
      return [...estado.cambiosResumen];
    },
    asegurarSesion,
    avisarSesionAbierta,
  };
}
