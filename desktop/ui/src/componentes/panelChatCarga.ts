/* Carga del panel de chat: aplicar la carga del backend (rewind o inicial),
 * restauración de archivos del tramo y gestión de conversación
 * (borrador local create-on-write, nueva, cargar). */
import type { CabeceraChat } from './cabecera';
import type { Entrada } from './entrada';
import { crearAvisoSistema } from './mensajes';
import type { AccionesChat } from './panelChatAcciones';
import type { HistorialChat } from './panelChatHistorial';
import type { DepsPanel, TipoPanel } from './panelChatTipos';
import type {
  CargaConversacion,
  ResultadoRestauracionTramo,
} from '../tauri/real';

export interface CargaDeps {
  d: DepsPanel;
  tipo: TipoPanel;
  mensajes: HTMLElement;
  entrada: Entrada;
  cabecera: CabeceraChat;
  fijarConversaId(id: string | null): void;
  limpiarChat: AccionesChat['limpiarChat'];
  pintarHistorial: HistorialChat['pintarHistorial'];
  aviso(texto: string, meta: string, detalle: string): void;
}

export interface CargaChat {
  /** Aplica la carga devuelta por el backend (rewind o carga inicial). */
  aplicarCarga(carga: CargaConversacion): void;
  /** "Volver a este punto": rewind con editar=false + repintar. */
  volverA(id: string): Promise<void>;
  /** [069A-7] Borrador local: limpia el panel y lo deja "sin conversación". */
  ponerBorrador(): void;
  /** [069A-7] "Nueva conversación" = borrador local (sin fila persistida). */
  nuevaConversacion(): Promise<void>;
  /** Carga una conversación en ESTE panel. En mock solo cambia el título. */
  cargarConversacion(id: string): Promise<void>;
}

export function crearCarga(deps: CargaDeps): CargaChat {
  const { d, tipo, mensajes, entrada, cabecera } = deps;

  /** Aplica la carga devuelta por el backend (rewind o carga inicial). */
  function aplicarCarga(carga: CargaConversacion): void {
    deps.fijarConversaId(carga.id);
    entrada.setConversaId(carga.id);
    deps.limpiarChat();
    deps.pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
    cabecera.ponerTitulo(carga.titulo);
    d.onConversacionCambio(carga.id);
  }

  async function restaurarArchivosTramo(archivosEsperados: string[]): Promise<void> {
    try {
      const r: ResultadoRestauracionTramo = await d.adaptador.sesion.restaurarTramo(tipo);
      if (r.restaurados.length === 0 && r.omitidos.length === 0) {
        deps.aviso('no había archivos que restaurar', 'restaurar', 'el tramo ya no conserva respaldos');
        return;
      }
      const lineas = (a: { ruta: string; estado: string; detalle?: string | null }[]): string =>
        a
          .slice(0, 5)
          .map((x) => {
            const det = x.detalle ? ` — ${x.detalle}` : '';
            return `${x.estado}: ${x.ruta}${det}`;
          })
          .join('\n') + (a.length > 5 ? `\n… y ${a.length - 5} más` : '');
      const partes: string[] = [];
      if (r.restaurados.length > 0) partes.push(`restaurados (${r.restaurados.length}):\n${lineas(r.restaurados)}`);
      if (r.omitidos.length > 0) partes.push(`omitidos (${r.omitidos.length}):\n${lineas(r.omitidos)}`);
      deps.aviso(
        'restauración de archivos del tramo',
        r.omitidos.length > 0 ? 'con cambios externos omitidos' : `${r.restaurados.length} restaurados`,
        partes.join('\n\n'),
      );
    } catch (e: unknown) {
      const conocido = archivosEsperados.length > 0 ? `\narchivos esperados:\n${archivosEsperados.join('\n')}` : '';
      deps.aviso(`no se pudo restaurar: ${String(e)}`, '', conocido);
    }
  }

  /** "Volver a este punto": rewind con editar=false + repintar. */
  async function volverA(id: string): Promise<void> {
    if (d.hayTurnoGlobal()) {
      deps.aviso('termina el turno antes de volver a un punto', '', '');
      return;
    }
    if (!d.usaReal) {
      deps.aviso('volver a un punto requiere la app Tauri', '', '');
      return;
    }
    try {
      const carga = await d.adaptador.sesion.rewind(id, false, tipo);
      aplicarCarga(carga);
      const archivos = carga.archivos_tramo;
      if (archivos && archivos.length > 0) {
        const detalle =
          archivos.slice(0, 3).join('\n') + (archivos.length > 3 ? `\n… y ${archivos.length - 3} más` : '');
        mensajes.appendChild(
          crearAvisoSistema(
            'volviste a un punto anterior que tocó archivos',
            `${archivos.length} archivo${archivos.length === 1 ? '' : 's'}`,
            detalle,
            { texto: 'Restaurar archivos', onClick: () => void restaurarArchivosTramo(archivos) },
          ),
        );
        mensajes.scrollTop = mensajes.scrollHeight;
      }
    } catch (e: unknown) {
      deps.aviso(`no se pudo volver a ese punto: ${String(e)}`, '', '');
    }
  }

  /** [069A-7] Borrador local: limpia el panel y lo deja "sin conversación".
   * NO llama al backend (create-on-write): la fila se creará al enviar el
   * primer mensaje, no al abrir/recargar/pulsar "Nueva conversación". */
  function ponerBorrador(): void {
    deps.fijarConversaId(null);
    entrada.setConversaId(null);
    deps.limpiarChat();
    cabecera.ponerTitulo('Nueva conversación');
    d.onConversacionCambio(null);
  }

  /** [069A-7] "Nueva conversación" = borrador local (sin fila persistida).
   * La creación real ocurre en `enviar()` al escribir el primer mensaje. */
  async function nuevaConversacion(): Promise<void> {
    if (d.hayTurnoGlobal()) {
      deps.aviso('termina el turno antes de abrir otra conversación', '', '');
      return;
    }
    if (!d.usaReal) {
      deps.aviso('nueva conversación requiere backend (Tauri o modo web)', '', '');
      return;
    }
    ponerBorrador();
  }

  /** Carga una conversación en ESTE panel. En mock solo cambia el título. */
  async function cargarConversacion(id: string): Promise<void> {
    deps.fijarConversaId(id);
    entrada.setConversaId(id);
    if (!d.usaReal) {
      const conv = d.conversaciones().find((c) => c.id === id);
      if (conv) cabecera.ponerTitulo(conv.titulo);
      d.onConversacionCambio(id);
      return;
    }
    if (d.hayTurnoGlobal()) {
      deps.aviso('termina el turno antes de cambiar de conversación', '', '');
      return;
    }
    try {
      const carga = await d.adaptador.sesion.cargar(id, tipo);
      deps.fijarConversaId(carga.id);
      entrada.setConversaId(carga.id);
      deps.limpiarChat();
      deps.pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
      cabecera.ponerTitulo(carga.titulo);
      d.onConversacionCambio(carga.id);
    } catch (e: unknown) {
      deps.aviso(`no se pudo cargar la conversación: ${String(e)}`, '', '');
    }
  }

  return { aplicarCarga, volverA, ponerBorrador, nuevaConversacion, cargarConversacion };
}
