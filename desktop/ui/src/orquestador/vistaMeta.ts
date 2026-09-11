/* PanelMeta global + estado de turno (extraído de main.ts [089A-16 F1b]).
 * El flag de turno y el último panel con envío viven aquí (M1: un turno a
 * la vez); el orquestador los consume vía los helpers devueltos. Sin
 * importes del orquestador. */

import { montarPanelMeta, type PanelMeta } from '../componentes/panelMeta';
import type { PanelChat } from '../componentes/panelChat';
import type { ComandoMetaVisible, EstadoMetaVisible } from '../dominio/tipos';

/** [109A-5 F3] Lo que el panel meta necesita de la sesión. Se agrupa en un
 * objeto para no llenar `VistaMetaDeps` de métodos de meta sueltos. */
export interface SesionMetaDeps {
  /** Borrador en memoria (solo cuando aún no hay conversación). */
  actualizarMeta: (valor: string | null) => Promise<unknown>;
  /** Ciclo de vida durable; devuelve el estado completo o `null` (borrador). */
  aplicarMeta: (comando: ComandoMetaVisible) => Promise<EstadoMetaVisible | null>;
  leerMeta: (conversacionId: string) => Promise<EstadoMetaVisible | null>;
  /** Id del último turno cerrado (respaldo de un logro). */
  ultimoTurnoId: () => string | null;
}

export interface VistaMetaDeps {
  usaReal: boolean;
  sesion: SesionMetaDeps;
  detenerReal: () => void;
  detenerMock: () => void;
  paneles: () => PanelChat[];
  avisar: (texto: string, meta: string, detalle: string) => void;
}

export interface VistaMeta {
  panelMeta: PanelMeta;
  hayTurnoGlobal: () => boolean;
  notificarTurnoInicio: () => void;
  notificarTurnoFin: (alTerminar: () => void) => void;
  registrarUltimoEnvio: (panel: PanelChat) => void;
  sincronizarPanelMeta: () => void;
}

export function montarVistaMeta(deps: VistaMetaDeps): VistaMeta {
  // Flag global: algún panel tiene turno en curso (M1: solo uno a la vez).
  let turnoGlobal = false;
  // Panel que envió el último mensaje (para el play/reanudar del panelMeta).
  let panelUltimoEnvio: PanelChat | null = null;

  function hayTurnoGlobal(): boolean {
    return turnoGlobal;
  }

  function notificarTurnoInicio(): void {
    turnoGlobal = true;
    deps.paneles().forEach((p) => p.setCorriendoGlobal(true));
  }

  function notificarTurnoFin(alTerminar: () => void): void {
    turnoGlobal = false;
    deps.paneles().forEach((p) => p.setCorriendoGlobal(false));
    alTerminar();
    /* [109A-5 F3] Después del primer turno ya existe un `turno_id` que puede
     * respaldar un logro: se recalcula aquí porque el pie acaba de cerrarse. */
    panelMeta.setHayTurno(deps.sesion.ultimoTurnoId() !== null);
  }

  function registrarUltimoEnvio(panel: PanelChat): void {
    panelUltimoEnvio = panel;
  }

  /** Conversación abierta del panel principal: el panel meta es global (M1)
   * pero la meta es POR conversación, así que la fila sin conversación es la
   * única que no puede tener reloj ni historial. */
  function conversacionActiva(): string | null {
    for (const panel of deps.paneles()) {
      if (panel.conversaId) return panel.conversaId;
    }
    return null;
  }

  /** Aplica un comando durable y repinta con la respuesta: el estado que
   * devuelve el backend es la misma fuente que el historial, así que el panel
   * no necesita una segunda consulta (ni puede quedar desincronizado). */
  async function aplicarMeta(comando: ComandoMetaVisible): Promise<void> {
    const id = conversacionActiva();
    if (id === null) return;
    const estado = await deps.sesion.aplicarMeta({ ...comando, conversacion_id: id });
    panelMeta.setEstadoMeta(estado);
  }

  /** Fuerza el patrón de error del panel: la meta no puede romper la UI. */
  function avisarFallo(que: string, error: unknown): void {
    deps.avisar(`no se pudo ${que}: ${String(error)}`, '', '');
  }

  /* Última conversación cuyo estado se leyó. Evita repetir el GET en cada
   * `sincronizarPanelMeta` (se llama al abrir/cerrar cada modal), pero no
   * bloquea el refresco tras un turno, que se pide explícito. */
  let conversacionLeida: string | null | undefined;

  /** Relee el estado durable de la conversación abierta. */
  async function refrescarEstadoMeta(forzar = false): Promise<void> {
    if (!deps.usaReal) return;
    const id = conversacionActiva();
    if (id === null) {
      conversacionLeida = null;
      panelMeta.setEstadoMeta(null);
      return;
    }
    if (!forzar && id === conversacionLeida) return;
    try {
      panelMeta.setEstadoMeta(await deps.sesion.leerMeta(id));
      conversacionLeida = id;
    } catch (e: unknown) {
      avisarFallo('leer la meta', e);
    }
  }

  // ---------- PanelMeta global (M1): lo monta el orquestador dentro de la
  // entrada del principal (el lateral no tiene panelMeta propio). ----------
  const panelMeta = montarPanelMeta({
    onMetaCambiada(meta) {
      if (!deps.usaReal) return;
      const texto = meta.trim();
      if (conversacionActiva() === null) {
        /* [109A-5 F3] Sin conversación no hay fila donde anclar el reloj ni el
         * historial: la meta sigue siendo un borrador en memoria, que el turno
         * lee como respaldo. No se simula un estado durable inexistente. */
        void deps.sesion
          .actualizarMeta(texto === '' ? null : texto)
          .catch((e: unknown) => avisarFallo('fijar la meta', e));
        return;
      }
      void aplicarMeta({
        accion: texto === '' ? 'limpiar' : 'fijar',
        meta: texto === '' ? null : texto,
      }).catch((e: unknown) => avisarFallo('fijar la meta', e));
    },
    onPausar() {
      if (!turnoGlobal) return;
      /* [109A-5 F3] Pausar el turno pausa también el reloj de la meta: para el
       * usuario es un solo acto, y sin esto el tiempo de persecución seguiría
       * corriendo mientras el agente está detenido. */
      if (deps.usaReal && panelMeta.metaActiva()) {
        void aplicarMeta({ accion: 'pausar' }).catch((e: unknown) =>
          avisarFallo('pausar la meta', e),
        );
      }
      if (deps.usaReal) deps.detenerReal();
      else deps.detenerMock();
      panelMeta.setEstado('pausado');
      deps.paneles().forEach((p) => p.setCorriendoGlobal(false));
      turnoGlobal = false;
    },
    onReanudar() {
      if (turnoGlobal) return;
      /* Solo si estaba pausada: `reanudar` sobre una meta que corre es un error
       * explícito del dominio, no un no-op que debamos provocar. */
      if (deps.usaReal && panelMeta.metaPausada()) {
        void aplicarMeta({ accion: 'reanudar' }).catch((e: unknown) =>
          avisarFallo('reanudar la meta', e),
        );
      }
      if (!panelUltimoEnvio) {
        deps.avisar('nada que reanudar: envía un mensaje primero', '', '');
        return;
      }
      panelUltimoEnvio.reanudarUltimo();
    },
    /** [109A-5 F3] Cierra la meta. El logro queda respaldado por el último
     * turno cerrado; el badge del pie lo pinta el evento `meta_lograda` que
     * emite el backend, no este camino. */
    onLograr() {
      if (!deps.usaReal) return;
      const turnoId = deps.sesion.ultimoTurnoId();
      if (turnoId === null) {
        deps.avisar(
          'no hay un turno que respalde el logro',
          '',
          'envía un mensaje antes de cerrar la meta',
        );
        return;
      }
      void aplicarMeta({ accion: 'lograr', turno_id: turnoId }).catch((e: unknown) =>
        avisarFallo('cerrar la meta', e),
      );
    },
  });

  function sincronizarPanelMeta(): void {
    /* [109A-4 F4] El modo global `meta` se retiró, así que la fila ya no depende
     * de él: vive con la conversación y muestra estado/tiempo/tokens del turno
     * M1. La meta que hay ahí se aplica al turno solo-lectura —`/meta <texto>`
     * la refleja y la persiste— y el backend la ignora fuera de esos turnos
     * (segunda barrera, no la única). */
    const hayConversacion = deps.paneles().some((p) => p.conversaId !== null);
    panelMeta.mostrar(hayConversacion);
    /* [109A-5 F3] La meta es por conversación: al cambiar de conversación hay
     * que releer su estado (activa, reloj, historial). `refrescarEstadoMeta`
     * evita el GET si la conversación no cambió. */
    panelMeta.setHayTurno(deps.sesion.ultimoTurnoId() !== null);
    void refrescarEstadoMeta();
  }

  return {
    panelMeta,
    hayTurnoGlobal,
    notificarTurnoInicio,
    notificarTurnoFin,
    registrarUltimoEnvio,
    sincronizarPanelMeta,
  };
}
