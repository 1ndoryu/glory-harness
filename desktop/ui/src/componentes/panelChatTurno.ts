/* Turno del panel de chat (runtime M1): opciones del turno con el estado
 * compartido, envío (edición via rewind + create-on-write), detención y
 * reenvío del último mensaje (play del panelMeta global). */
import type { CabeceraChat } from './cabecera';
import type { Entrada } from './entrada';
import { crearAvisoSistema } from './mensajes';
import type { DepsPanel, PanelChat, TipoPanel } from './panelChatTipos';
import type { EjecutorComandos } from './panelChatComandos';
import type {
  CargaConversacion,
  OpcionesTurno,
  UsoTurno,
} from '../tauri/real';

export interface TurnoDeps {
  d: DepsPanel;
  tipo: TipoPanel;
  mensajes: HTMLElement;
  entrada: Entrada;
  cabecera: CabeceraChat;
  /** Panel público (para `registrarUltimoEnvio`; getter perezoso). */
  getPanel(): PanelChat;
  /** true si ESTE panel está corriendo (lo aporta el panel). */
  getCorriendo(): boolean;
  getConversaId(): string | null;
  fijarConversaId(id: string | null): void;
  aplicarCarga(carga: CargaConversacion): void;
  anadirPieTurno(u: UsoTurno): void;
  aviso(texto: string, meta: string, detalle: string): void;
  /** [109A-4] Resuelve un `/comando` antes de montar el turno. */
  comandos: EjecutorComandos;
}

export interface TurnoChat {
  enviar(texto: string, editandoId?: string | null): Promise<void>;
  detener(): void;
  /** Inicio (ms) del turno en curso de este panel (o null). */
  getInicioTurno(): number | null;
  /** Play del panelMeta global: reenvía el último mensaje de ESTE panel. */
  reanudarUltimo(): void;
}

export function crearTurno(deps: TurnoDeps): TurnoChat {
  const { d, tipo, mensajes, entrada, cabecera } = deps;

  let ultimoTextoEnviado = '';
  let inicioTurno: number | null = null;

  /** Opciones del turno con el modelo/modo/razonamiento compartidos (M1). */
  function opcionesTurno(): OpcionesTurno {
    let proveedor = d.getModelo().proveedor;
    const modelo = d.getModelo().modelo;
    if (proveedor === 'commandcode' && /^(meta|stealth)\//.test(modelo)) {
      proveedor = 'glory';
    }
    return {
      proveedor,
      modelo,
      modo: d.getModo(),
      razonamiento: d.getRazonamiento(),
      panelId: tipo === 'lateral' ? 'lateral' : undefined,
    };
  }

  /** Empuja la meta editable (única, del panelMeta global) antes del turno. */
  async function empujarMeta(): Promise<void> {
    const meta = d.panelMeta.getMeta().trim() ? d.panelMeta.getMeta().trim() : null;
    try {
      await d.adaptador.sesion.actualizarMeta(meta);
    } catch (e: unknown) {
      deps.aviso(`no se pudo fijar la meta: ${String(e)}`, '', 'el turno sigue sin meta');
    }
  }

  async function enviar(texto: string, editandoId?: string | null): Promise<void> {
    // M1: un solo turno a la vez. Si OTRO panel corre, este no puede enviar.
    if (d.hayTurnoGlobal()) {
      deps.aviso('termina el turno en curso antes de enviar', '', '');
      return;
    }
    // [109A-4] Un `/comando` nunca cae al agente por accidente: se resuelve
    // ANTES de montar el turno. Solo los que devuelven `prompt` (comandos del
    // área y `/revisar`,`/iniciar`) siguen el curso normal como texto.
    const comando = await deps.comandos.resolver(texto);
    if (comando.tipo === 'consumido') return;
    if (comando.tipo === 'error') {
      deps.aviso(comando.mensaje, 'comando no ejecutado', 'prueba /ayuda');
      return;
    }
    /* [109A-4 F4] `/meta <texto>` marca el turno como solo-lectura: viaja en
     * las opciones (el backend fuerza el modo meta de ESE turno y lo revierte
     * al terminar), así que el reenvío tras aprobar conserva la política. */
    let soloLectura = false;
    if (comando.tipo === 'prompt') {
      texto = comando.texto;
      soloLectura = comando.soloLectura === true;
    }
    ultimoTextoEnviado = texto;
    inicioTurno = Date.now();
    d.registrarUltimoEnvio(deps.getPanel());
    // El orquestador marca TODOS los paneles 'corriendo' + el flag global M1.
    d.notificarTurnoInicio();
    if (d.usaReal) d.panelMeta.setEstado('corriendo');

    const alTerminar = () => {
      inicioTurno = null;
      if (d.usaReal) {
        d.panelMeta.setEstado('inactivo');
        const u = d.adaptador.usoUltimoTurno();
        d.panelMeta.setTokens(u.tokensPrompt + u.tokensComplecion);
        if (d.adaptador.resultadoUltimoTurno() === 'ok') {
          deps.anadirPieTurno(u);
        }
        void (async () => {
          await d.resincronizarSidebar();
          if (deps.getConversaId()) {
            try {
              const lista2 = await d.adaptador.sesion.listar();
              const actual = lista2.find((c) => c.id === deps.getConversaId());
              if (actual) cabecera.ponerTitulo(actual.titulo);
            } catch {
              /* el refresco del título es cosmético */
            }
          }
        })();
      } else if (d.usaMock) {
        deps.anadirPieTurno({
          tokensPrompt: 1240,
          tokensComplecion: 385,
          ocupacionPct: 7,
          maxVentana: 150000,
          reservaSalida: 20000,
          modelo: 'glory/gpt-4.1',
          totalEntrada: 9100,
        });
        // [039A-3 P6] Espejo del pie mock: el % y la ventana del pie también
        // se reflejan en el indicador circular (verificación visual sin
        // backend; en real el ContextoDetalle/usage alimenta setContexto).
        entrada.setContexto({ pct: 7, maxVentana: 150000, reservaSalida: 20000, totalEntrada: 9100 });
      }
      d.notificarTurnoFin();
    };

    // Edición: rewind(editar=true) + reenvío con el texto corregido.
    if (d.usaReal && editandoId) {
      try {
        const carga = await d.adaptador.sesion.rewind(editandoId, true, tipo);
        deps.aplicarCarga(carga);
      } catch (e: unknown) {
        deps.aviso(`no se pudo editar el mensaje: ${String(e)}`, '', '');
        inicioTurno = null;
        d.notificarTurnoFin();
        if (d.usaReal) d.panelMeta.setEstado('inactivo');
        return;
      }
    }

    // [069A-7] Create-on-write: si el panel está en BORRADOR (sin conversación),
    // el primer mensaje crea la fila AHORA (la ancla el backend como actual del
    // panel/sesión y la auto-nombra desde el texto en `preparar_turno` [H5]).
    // Abrir, recargar o pulsar "Nueva conversación" NUNCA crean la fila.
    if (d.usaReal && deps.getConversaId() === null) {
      try {
        const conv = await d.adaptador.sesion.nueva(undefined, tipo);
        deps.fijarConversaId(conv.id);
        entrada.setConversaId(conv.id);
        cabecera.ponerTitulo(conv.titulo);
        d.onConversacionCambio(conv.id);
      } catch (e: unknown) {
        deps.aviso(`no se pudo crear la conversación: ${String(e)}`, '', '');
        inicioTurno = null;
        d.notificarTurnoFin();
        if (d.usaReal) d.panelMeta.setEstado('inactivo');
        return;
      }
    }

    if (d.usaReal) {
      void (async () => {
        // [109A-4 F4] La meta del turno la aporta el comando: `/meta <texto>`
        // fuerza solo lectura en ESE turno (el backend aplica el modo y lo
        // revierte al terminar). El texto se refleja en la fila y se persiste
        // como meta de la conversación, que es la que el backend antepone en
        // los siguientes turnos solo-lectura; fuera de ellos no se antepone.
        if (soloLectura) {
          d.panelMeta.setMeta(texto);
          await empujarMeta();
        }
        await d.adaptador.montar(
          mensajes,
          texto,
          { ...opcionesTurno(), soloLectura },
          alTerminar,
        );
      })();
    } else if (d.usaMock) {
      d.simulacion.montar(mensajes, texto, d.getModo() === 'autonomo', alTerminar);
    } else {
      mensajes.appendChild(
        crearAvisoSistema('Sin backend: abre desde la app Tauri o sirve la UI con `glory-harness web` (?api=<url>&token=...)', 'sin backend', 'con VITE_MOCK=1 se usa la simulación'),
      );
      inicioTurno = null;
      d.notificarTurnoFin();
    }
  }

  function detener(): void {
    if (d.usaReal) {
      d.adaptador.detener();
      d.panelMeta.setEstado('pausado');
    } else {
      d.simulacion.detener();
    }
    // `detener()` en real.ts/simulacion llama a `onFin` (alTerminar) → el
    // orquestador pone todos en reposo vía notificarTurnoFin.
  }

  /** Play del panelMeta global: reenvía el último mensaje de ESTE panel. */
  function reanudarUltimo(): void {
    if (deps.getCorriendo()) return;
    if (!ultimoTextoEnviado) {
      deps.aviso('nada que reanudar: envía un mensaje primero', '', '');
      return;
    }
    void enviar(ultimoTextoEnviado);
  }

  return {
    enviar,
    detener,
    getInicioTurno() {
      return inicioTurno;
    },
    reanudarUltimo,
  };
}
