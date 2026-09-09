// ============================================================
// Fábrica de panel de chat (plan 039A-3, P5 — S3b).
// Encapsula UN `.chat` completo y duplicable: cabecera + mensajes
// + entrada (+ acciones por mensaje y por cabecera). Cada instancia
// cierra sobre SUS nodos y su estado local (conversaId, historial,
// último envío, inicio de turno), de modo que el orquestador
// (main.ts) pueda tener 1-2 paneles sin variables de módulo
// compartidas que los callbacks asíncronos capturarían mal.
//
// Modelo M1: un solo runtime/turno a la vez. El estado compartido
// (modelo/modo/razonamiento) y el panelMeta viven en el orquestador
// y se inyectan por `deps`; el panel que lanza un turno es el que
// recibe los eventos (el adaptador real es compartido).
// ============================================================

import type {
  Conversacion,
  ElementoSeleccionado,
  ModeloSeleccionado,
  ProveedorModelo,
  Workspace,
} from '../dominio/tipos';
import { montarCabeceraChat, type CabeceraChat } from './cabecera';
import {
  montarEntrada,
  type Entrada,
  type EstadoContexto,
  type ModoEjecucion,
} from './entrada';
import type { PanelMeta } from './panelMeta';
import type { Sidebar } from './sidebar';
import {
  crearAvisoSistema,
  crearHerramienta,
  crearMensajeAsistente,
  formatearResultadoHerramienta,
  crearMensajeUsuario,
  crearPieTurno,
} from './mensajes';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
} from './menu';
import { crearSimulacion } from '../simulacion/simulacion';
import {
  descripcionDeTool,
  iconoDeTool,
  type AccionRecuperada,
  type AdaptadorReal,
  type CargaConversacion,
  type MensajeGuardado,
  type OpcionesTurno,
  type ResultadoRestauracionTramo,
  type UsoTurno,
} from '../tauri/real';
import type { EstadoHerramienta, ResultadoHerramienta } from '../dominio/tipos';
import { el } from '../util/dom';
import { copiarAlPortapapeles } from '../util/portapapeles';

/** 'principal' | 'lateral' — coincide con el panel_id del backend. */
export type TipoPanel = 'principal' | 'lateral';

/**
 * Dependencias compartidas del runtime M1 que el orquestador posee y
 * entrega a cada panel. Lo que NO es por-panel vive aquí.
 */
export interface DepsPanel {
  adaptador: AdaptadorReal;
  simulacion: ReturnType<typeof crearSimulacion>;
  usaReal: boolean;
  usaMock: boolean;
  /** PanelMeta ÚNICO (global M1). El orquestador lo monta en el principal. */
  panelMeta: PanelMeta;
  /** Sidebar (dueña de la lista). El panel la usa desde el ⋯ de su cabecera. */
  sidebar: Sidebar;
  /** Lista actual de conversaciones (para el título/archivada del ⋯). */
  conversaciones(): Conversacion[];
  /** Estado compartido M1 (fuente: panel principal). */
  getModelo(): ModeloSeleccionado;
  getModo(): ModoEjecucion;
  getRazonamiento(): string;
  /** Workspaces disponibles y destino actual de las conversaciones nuevas. */
  getWorkspaces(): Workspace[];
  getWorkspaceSeleccionadoId(): string | null;
  /** Activa el destino seleccionado en la sesión antes de crear la conversación. */
  onWorkspaceCambiado(id: string | null): void;
  /** Prepara el workspace destino justo antes de enviar (create-on-write). */
  prepararWorkspaceSeleccionado(): Promise<void>;
  /** true si CUALQUIER panel tiene turno en curso (M1: 1 a la vez). */
  hayTurnoGlobal(): boolean;
  /** El panel que lanza avisa → el orquestador pone TODOS en 'corriendo'. */
  notificarTurnoInicio(): void;
  /** Al terminar → el orquestador pone todos en reposo y refresca la lista. */
  notificarTurnoFin(): void;
  /** Registra qué panel envió el último mensaje (para reanudar del panelMeta). */
  registrarUltimoEnvio(panel: PanelChat): void;
  /** Refresca la lista desde el backend (tras renombrar/auto-nombrar). */
  resincronizarSidebar(): Promise<void>;
  /** El panel cambió de conversación → la sidebar marca el panel enfocado. */
  onConversacionCambio(id: string | null): void;
}

/** Un chat duplicable montado por `montarPanelChat`. */
export interface PanelChat {
  raiz: HTMLElement;
  /** Nodo `.mensajes` de este panel (para pintar bloques del mock). */
  mensajes: HTMLElement;
  tipo: TipoPanel;
  idPrefijo: string;
  /** Conversación que este panel muestra (null = sin conversación). */
  conversaId: string | null;
  getCorriendo(): boolean;
  /** Marca el panel como ENFOCADO (el orquestador pinta la sidebar). */
  activar(): void;
  /** Marca el panel como NO enfocado (lo usa el orquestador al activar otro). */
  desenfocar(): void;
  /** Carga una conversación en ESTE panel (backend real o mock). */
  cargarConversacion(id: string): Promise<void>;
  /** Crea una conversación nueva en ESTE panel (solo real). */
  nuevaConversacion(): Promise<void>;
  /** Reenvía el último mensaje de ESTE panel (play del panelMeta global). */
  reanudarUltimo(): void;
  /** El orquestador marca el estado de turno (M1: afecta a todos). */
  setCorriendoGlobal(corriendo: boolean): void;
  /** Inicio (ms) del turno en curso de este panel (o null). */
  getInicioTurno(): number | null;
  /** Enfoca el textarea de este panel. */
  enfocarEntrada(): void;
  /** Pone el título de la cabecera de este panel. */
  ponerTitulo(texto: string): void;
  /** Vacía los mensajes y cancela una edición pendiente. */
  limpiar(): void;
  /** [069A-7] Pone el panel en BORRADOR local: `conversaId=null`, chat vacío
   * y título "Nueva conversación", SIN tocar el backend (create-on-write: la
   * fila solo se crea al enviar el primer mensaje). */
  ponerBorrador(): void;
  /** Recalcula la altura del textarea (llamar tras montar al DOM). */
  medir(): void;
  /** Entra en modo renombrar inline en la cabecera de ESTE panel. */
  empezarRenombrarCabecera(
    valor: string,
    onGuardar: (nuevo: string) => void,
    onCancelar?: () => void,
  ): void;
  /** [039A-3 P6b] Sincroniza los controles de la entrada de ESTE panel. Todos
   * los paneles usan la variante completa y comparten el estado M1 del
   * orquestador (modelo/modo/razonamiento), así que los tres setter existen
   * en principal y lateral. */
  setModelo(modelo: ModeloSeleccionado): void;
  setModo(modo: ModoEjecucion): void;
  setRazonamiento(valor: string): void;
  /** [039A-3 P4/P6b] Refleja lista visible/oculta en el botón de la cabecera
   * (solo el principal lo tiene). */
  setSidebarAbierta(abierta: boolean): void;
  /** [089A-2] Refleja panel derecho visible/oculto en el botón de la
   * cabecera (solo el principal lo tiene). */
  setPanelDerechoAbierto(abierto: boolean): void;
  /** Repinta un aviso en este panel (mock/próximamente, sin backend). */
  avisoLocal(texto: string, meta: string, detalle: string): void;
  /** [039A-3 P6] Actualiza el indicador circular de contexto de la entrada. */
  setContexto(estado: EstadoContexto): void;
  /** Actualiza el selector de área de trabajo de la conversación nueva. */
  setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null): void;
  /** Oculta/muestra el selector de workspace según si la conversación es nueva. */
  setConversaId(id: string | null): void;
  /** [seleccionar] Muestra un elemento del navegador como badge pendiente en
   * la entrada de ESTE panel (se antepone al próximo mensaje enviado). */
  adjuntarElemento(elem: ElementoSeleccionado): void;
  /** Cambios de archivos de la conversación cargada para sincronizar Files. */
  listarCambios(): CambioArchivoPanel[];
}

/** Cambio de archivo de la conversación (ruta + diff ya formateado). */
export interface CambioArchivoPanel {
  origen: 'tool';
  tool: 'file_write' | 'file_patch';
  ruta: string;
  titulo: string;
  resumen: string;
  diff: string | null;
}

export interface PanelChatOpciones {
  tipo: TipoPanel;
  /** Prefijo de ids DOM (coincide con `tipo`; p. ej. 'principal'). */
  idPrefijo: string;
  /** Catálogo de proveedores de la entrada completa (principal y lateral). */
  proveedores?: ProveedorModelo[];
  deps: DepsPanel;
  /** Se invoca al pulsar el botón ⋯ de la cabecera de ESTE panel. */
  onAcciones(rect: DOMRect): void;
  /** Panel lateral: se invoca al pulsar el × de cierre. */
  onCerrar?: () => void;
  /** Panel principal: se invoca al pulsar el botón de MOSTRAR la lista
   * (solo aparece si la lista está oculta por ancho; no hay ocultación manual). */
  onToggleSidebar?: () => void;
  /** [089A-2] Panel principal: muestra/oculta el panel derecho. */
  onTogglePanelDerecho?: () => void;
  /** El usuario cambió el modelo/modo/razonamiento en la barra de ESTE panel
   * (todos los paneles son completos; el orquestador propaga M1 al resto). */
  onModeloCambiado?: (modelo: ModeloSeleccionado) => void;
  onModoCambiado?: (modo: ModoEjecucion) => void;
  onRazonamientoCambiado?: (razonamiento: string) => void;
  /** El usuario eligió un área de trabajo distinta para la conversación nueva. */
  onWorkspaceCambiado?: (workspaceId: string | null) => void;
}

export function montarPanelChat(opts: PanelChatOpciones): PanelChat {
  const d = opts.deps;
  const { tipo, idPrefijo } = opts;

  const chat = el('section');
  chat.className = 'chat';
  if (tipo === 'lateral') chat.dataset.panel = 'lateral';

  // ---------- cabecera + mensajes + entrada ----------
  const cabecera: CabeceraChat = montarCabeceraChat({
    idPrefijo,
    titulo: 'Sin conversación',
    lateral: tipo === 'lateral',
    onAcciones(rect) {
      opts.onAcciones(rect);
    },
    onToggleSidebar: tipo === 'principal' ? () => opts.onToggleSidebar?.() : undefined,
    onTogglePanelDerecho:
      tipo === 'principal' ? () => opts.onTogglePanelDerecho?.() : undefined,
    onCerrar: tipo === 'lateral' ? () => opts.onCerrar?.() : undefined,
  });

  const mensajes = el('div');
  mensajes.className = 'mensajes';

  const entrada: Entrada = montarEntrada({
    idPrefijo,
    // [039A-3 P6b] Ambos paneles usan la variante 'completa': el lateral
    // también permite elegir modelo/razonamiento/modo (compartidos M1 con el
    // principal; el orquestador propaga el cambio a los dos selectores).
    variante: 'completa',
    proveedores: opts.proveedores ?? [],
    modeloActual: d.getModelo(),
    modo: d.getModo(),
    razonamiento: d.getRazonamiento(),
    workspaces: d.getWorkspaces(),
    workspaceSeleccionadoId: d.getWorkspaceSeleccionadoId(),
    onWorkspaceCambiado(workspaceId) {
      opts.onWorkspaceCambiado?.(workspaceId);
    },
    onEnviar(texto, editandoId) {
      void enviar(texto, editandoId);
    },
    onDetener() {
      detener();
    },
    onModeloCambiado(modelo) {
      opts.onModeloCambiado?.(modelo);
    },
    onModoCambiado(modo) {
      opts.onModoCambiado?.(modo);
    },
    onRazonamientoCambiado(raz) {
      opts.onRazonamientoCambiado?.(raz);
    },
  });

  chat.appendChild(cabecera.raiz);
  chat.appendChild(mensajes);
  chat.appendChild(entrada.raiz);

  // [089A-2] La entrada flota por encima del scroll: la reserva inferior de
  // .mensajes sigue a la altura real de la entrada (el textarea crece).
  try {
    const reserva = new ResizeObserver(() => {
      const alto = entrada.raiz.getBoundingClientRect().height;
      if (alto > 0) mensajes.style.paddingBottom = `${Math.ceil(alto) + 24}px`;
    });
    reserva.observe(entrada.raiz);
  } catch {
    // Sin ResizeObserver (navegador antiguo): vale el valor CSS inicial.
  }

  // ---------- estado local del panel ----------
  let conversaId: string | null = null;
  let usuariosHistorial = new Map<string, string>();
  let ultimoTextoEnviado = '';
  let inicioTurno: number | null = null;
  let corriendo = false;
  let enfocado = false;

  function pintarEnfocado(): void {
    chat.classList.toggle('enfocado', enfocado);
  }
  // Clic en cualquier parte del chat lo enfoca (la sidebar marca su conversación).
  chat.addEventListener('pointerdown', () => {
    if (!enfocado) {
      enfocado = true;
      pintarEnfocado();
      d.onConversacionCambio(conversaId);
    }
  });

  // ---------- helpers de aviso / copia ----------
  function avisoChat(texto: string, meta: string, detalle: string): void {
    mensajes.appendChild(crearAvisoSistema(texto, meta, detalle));
    mensajes.scrollTop = mensajes.scrollHeight;
  }

  function limpiarChat(): void {
    mensajes.replaceChildren();
    usuariosHistorial = new Map<string, string>();
    // [089A-2] Los cambios acumulados pertenecen a la conversación anterior.
    cambios.length = 0;
    // Se descarta una edición pendiente (su mensaje ya no está visible).
    entrada.cancelarEnEdicion();
  }

  function tramoParaCopiar(): string | null {
    const hijos = Array.from(mensajes.children);
    const ultimoIndice = (pred: (n: Element) => boolean): number => {
      for (let i = hijos.length - 1; i >= 0; i--) {
        if (pred(hijos[i])) return i;
      }
      return -1;
    };
    const ultimoUser = ultimoIndice((n) => n.classList.contains('msg-user'));
    if (ultimoUser < 0) return null;
    const ultimoAsis = ultimoIndice((n) => n.classList.contains('msg-asis'));
    const fin = ultimoAsis >= ultimoUser ? ultimoAsis : ultimoUser;
    const texto = hijos
      .slice(ultimoUser, fin + 1)
      .map((n) => {
        if (n.classList.contains('pie-turno')) return '';
        return (n.textContent ?? '').trim();
      })
      .filter((s) => s.length > 0)
      .join('\n\n');
    return texto || null;
  }

  function copiarUltimoTramo(): void {
    const texto = tramoParaCopiar();
    if (!texto) {
      avisoChat('no hay mensajes que copiar', '', '');
      return;
    }
    void copiarAlPortapapeles(texto)
      .then(() => avisoChat('tramo copiado al portapapeles', 'copiar', ''))
      .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
  }

  function tramoDesdeId(id: string): string | null {
    const nodo = mensajes.querySelector<HTMLElement>(`.msg-user[data-id="${CSS.escape(id)}"]`);
    if (!nodo) return null;
    const hijos = Array.from(mensajes.children);
    const i = hijos.indexOf(nodo);
    if (i < 0) return null;
    const partes: string[] = [];
    for (let j = i; j < hijos.length; j++) {
      const n = hijos[j];
      if (j > i && n.classList.contains('msg-user')) break;
      if (n.classList.contains('pie-turno')) continue;
      const t = (n.textContent ?? '').trim();
      if (t) partes.push(t);
    }
    return partes.length ? partes.join('\n\n') : null;
  }

  function copiarTramoMensaje(id: string): void {
    const texto = tramoDesdeId(id);
    if (!texto) {
      avisoChat('no hay texto que copiar', '', '');
      return;
    }
    void copiarAlPortapapeles(texto)
      .then(() => avisoChat('mensaje copiado al portapapeles', 'copiar', ''))
      .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
  }

  // ---------- acciones por mensaje (editar / volver / copiar) ----------
  function empezarEdicion(id: string): void {
    if (d.hayTurnoGlobal()) {
      avisoChat('termina el turno antes de editar un mensaje', '', '');
      return;
    }
    const texto = usuariosHistorial.get(id);
    if (texto === undefined) {
      avisoChat('el mensaje ya no está en esta conversación', '', '');
      return;
    }
    entrada.ponerEnEdicion(id, texto);
  }

  function abrirAccionesMensaje(id: string, rect: DOMRect): void {
    abrirMenuContextual({
      rect,
      construir(m) {
        m.appendChild(
          crearItemMenu({
            texto: 'Editar',
            onClick() {
              cerrarMenuActual();
              empezarEdicion(id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Volver a este punto',
            onClick() {
              cerrarMenuActual();
              void volverA(id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Copiar',
            onClick() {
              cerrarMenuActual();
              copiarTramoMensaje(id);
            },
          }),
        );
      },
    });
  }

  /** Aplica la carga devuelta por el backend (rewind o carga inicial). */
  function aplicarCarga(carga: CargaConversacion): void {
    conversaId = carga.id;
    entrada.setConversaId(conversaId);
    limpiarChat();
    pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
    cabecera.ponerTitulo(carga.titulo);
    d.onConversacionCambio(carga.id);
  }

  async function restaurarArchivosTramo(archivosEsperados: string[]): Promise<void> {
    try {
      const r: ResultadoRestauracionTramo = await d.adaptador.sesion.restaurarTramo(tipo);
      if (r.restaurados.length === 0 && r.omitidos.length === 0) {
        avisoChat('no había archivos que restaurar', 'restaurar', 'el tramo ya no conserva respaldos');
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
      avisoChat(
        'restauración de archivos del tramo',
        r.omitidos.length > 0 ? 'con cambios externos omitidos' : `${r.restaurados.length} restaurados`,
        partes.join('\n\n'),
      );
    } catch (e: unknown) {
      const conocido = archivosEsperados.length > 0 ? `\narchivos esperados:\n${archivosEsperados.join('\n')}` : '';
      avisoChat(`no se pudo restaurar: ${String(e)}`, '', conocido);
    }
  }

  /** "Volver a este punto": rewind con editar=false + repintar. */
  async function volverA(id: string): Promise<void> {
    if (d.hayTurnoGlobal()) {
      avisoChat('termina el turno antes de volver a un punto', '', '');
      return;
    }
    if (!d.usaReal) {
      avisoChat('volver a un punto requiere la app Tauri', '', '');
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
      avisoChat(`no se pudo volver a ese punto: ${String(e)}`, '', '');
    }
  }

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
        mensajes.appendChild(crearMensajeUsuario(m.contenido, m.id, abrirAccionesMensaje));
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
          alCopiar: copiarUltimoTramo,
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
        alCopiar: copiarUltimoTramo,
      }),
    );
    mensajes.scrollTop = mensajes.scrollHeight;
  }

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
      avisoChat(`no se pudo fijar la meta: ${String(e)}`, '', 'el turno sigue sin meta');
    }
  }

  async function enviar(texto: string, editandoId?: string | null): Promise<void> {
    // M1: un solo turno a la vez. Si OTRO panel corre, este no puede enviar.
    if (d.hayTurnoGlobal()) {
      avisoChat('termina el turno en curso antes de enviar', '', '');
      return;
    }
    ultimoTextoEnviado = texto;
    inicioTurno = Date.now();
    d.registrarUltimoEnvio(panel);
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
          anadirPieTurno(u);
        }
        void (async () => {
          await d.resincronizarSidebar();
          if (conversaId) {
            try {
              const lista2 = await d.adaptador.sesion.listar();
              const actual = lista2.find((c) => c.id === conversaId);
              if (actual) cabecera.ponerTitulo(actual.titulo);
            } catch {
              /* el refresco del título es cosmético */
            }
          }
        })();
      } else if (d.usaMock) {
        anadirPieTurno({
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
        aplicarCarga(carga);
      } catch (e: unknown) {
        avisoChat(`no se pudo editar el mensaje: ${String(e)}`, '', '');
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
    if (d.usaReal && conversaId === null) {
      try {
        const conv = await d.adaptador.sesion.nueva(undefined, tipo);
        conversaId = conv.id;
        entrada.setConversaId(conversaId);
        cabecera.ponerTitulo(conv.titulo);
        d.onConversacionCambio(conv.id);
      } catch (e: unknown) {
        avisoChat(`no se pudo crear la conversación: ${String(e)}`, '', '');
        inicioTurno = null;
        d.notificarTurnoFin();
        if (d.usaReal) d.panelMeta.setEstado('inactivo');
        return;
      }
    }

    if (d.usaReal) {
      void (async () => {
        if (d.getModo() === 'meta') await empujarMeta();
        await d.adaptador.montar(mensajes, texto, opcionesTurno(), alTerminar);
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

  /** Sincronizador visual: lo invoca el orquestador en TODOS los paneles
   * cuando un turno empieza/termina (M1). No lanza ni corta turnos. */
  function setCorriendoGlobal(v: boolean): void {
    corriendo = v;
    entrada.setCorriendo(v);
  }

  /** [069A-7] Borrador local: limpia el panel y lo deja "sin conversación".
   * NO llama al backend (create-on-write): la fila se creará al enviar el
   * primer mensaje, no al abrir/recargar/pulsar "Nueva conversación". */
  function ponerBorrador(): void {
    conversaId = null;
    entrada.setConversaId(null);
    limpiarChat();
    cabecera.ponerTitulo('Nueva conversación');
    d.onConversacionCambio(null);
  }

  /** [069A-7] "Nueva conversación" = borrador local (sin fila persistida).
   * La creación real ocurre en `enviar()` al escribir el primer mensaje. */
  async function nuevaConversacion(): Promise<void> {
    if (d.hayTurnoGlobal()) {
      avisoChat('termina el turno antes de abrir otra conversación', '', '');
      return;
    }
    if (!d.usaReal) {
      avisoChat('nueva conversación requiere backend (Tauri o modo web)', '', '');
      return;
    }
    ponerBorrador();
  }

  /** Carga una conversación en ESTE panel. En mock solo cambia el título. */
  async function cargarConversacion(id: string): Promise<void> {
    conversaId = id;
    entrada.setConversaId(id);
    if (!d.usaReal) {
      const conv = d.conversaciones().find((c) => c.id === id);
      if (conv) cabecera.ponerTitulo(conv.titulo);
      d.onConversacionCambio(id);
      return;
    }
    if (d.hayTurnoGlobal()) {
      avisoChat('termina el turno antes de cambiar de conversación', '', '');
      return;
    }
    try {
      const carga = await d.adaptador.sesion.cargar(id, tipo);
      conversaId = carga.id;
      entrada.setConversaId(conversaId);
      limpiarChat();
      pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
      cabecera.ponerTitulo(carga.titulo);
      d.onConversacionCambio(carga.id);
    } catch (e: unknown) {
      avisoChat(`no se pudo cargar la conversación: ${String(e)}`, '', '');
    }
  }

  /** Play del panelMeta global: reenvía el último mensaje de ESTE panel. */
  function reanudarUltimo(): void {
    if (corriendo) return;
    if (!ultimoTextoEnviado) {
      avisoChat('nada que reanudar: envía un mensaje primero', '', '');
      return;
    }
    void enviar(ultimoTextoEnviado);
  }

  const panel: PanelChat = {
    raiz: chat,
    mensajes,
    tipo,
    idPrefijo,
    conversaId: null,
    getCorriendo: () => corriendo,
    activar() {
      if (!enfocado) {
        enfocado = true;
        pintarEnfocado();
      }
    },
    desenfocar() {
      if (enfocado) {
        enfocado = false;
        pintarEnfocado();
      }
    },
    async cargarConversacion(id) {
      await cargarConversacion(id);
    },
    async nuevaConversacion() {
      await nuevaConversacion();
    },
    reanudarUltimo() {
      reanudarUltimo();
    },
    setCorriendoGlobal(v: boolean) {
      setCorriendoGlobal(v);
    },
    getInicioTurno: () => inicioTurno,
    enfocarEntrada() {
      entrada.enfocar();
    },
    ponerTitulo(texto: string) {
      cabecera.ponerTitulo(texto);
    },
    limpiar() {
      limpiarChat();
    },
    ponerBorrador() {
      ponerBorrador();
    },
    medir() {
      entrada.medir();
    },
    empezarRenombrarCabecera(valor, onGuardar, onCancelar) {
      cabecera.empezarRenombrar(valor, onGuardar, onCancelar);
    },
    setModelo(modelo: ModeloSeleccionado) {
      entrada.setModelo(modelo);
    },
    setModo(modo: ModoEjecucion) {
      entrada.setModo(modo);
    },
    setRazonamiento(valor: string) {
      entrada.setRazonamientoValor(valor);
    },
    setSidebarAbierta(abierta: boolean) {
      cabecera.setSidebarAbierta(abierta);
    },
    setPanelDerechoAbierto(abierto: boolean) {
      cabecera.setPanelDerechoAbierto(abierto);
    },
    avisoLocal(texto: string, meta: string, detalle: string) {
      avisoChat(texto, meta, detalle);
    },
    setContexto(estado: EstadoContexto) {
      entrada.setContexto(estado);
    },
    setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null) {
      entrada.setWorkspaces(workspaces, seleccionadoId);
    },
    setConversaId(id: string | null) {
      entrada.setConversaId(id);
    },
    adjuntarElemento(elem: ElementoSeleccionado) {
      entrada.adjuntarElemento(elem);
    },
    listarCambios() {
      return [...cambios];
    },
  };

  // El campo `conversaId` del objeto público debe reflejar el estado local.
  Object.defineProperty(panel, 'conversaId', {
    get() {
      return conversaId;
    },
  });

  return panel;
}
