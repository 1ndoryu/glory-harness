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

import { montarCabeceraChat, type CabeceraChat } from './cabecera';
import {
  montarEntrada,
  type Entrada,
  type EstadoContexto,
  type ModoEjecucion,
} from './entrada';
import type {
  ElementoSeleccionado,
  ModeloSeleccionado,
  Workspace,
} from '../dominio/tipos';
import { crearAcciones } from './panelChatAcciones';
import { crearCarga } from './panelChatCarga';
import { crearComandosArea } from './comandosArea';
import { crearEjecutorComandos } from './panelChatComandos';
import { crearHistorial } from './panelChatHistorial';
import { crearTurno } from './panelChatTurno';
import type { PanelChat, PanelChatOpciones } from './panelChatTipos';
export type {
  CambioArchivoPanel,
  DepsPanel,
  PanelChat,
  PanelChatOpciones,
  TipoPanel,
} from './panelChatTipos';
import { el } from '../util/dom';

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
      // El área activa cambia con el destino elegido: el catálogo de comandos
      // `/` del área se recarga (el backend resuelve la carpeta, no el front).
      void comandosArea.refrescar();
    },
    onEnviar(texto, editandoId) {
      void turno.enviar(texto, editandoId);
    },
    onDetener() {
      turno.detener();
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



  /** Sincronizador visual: lo invoca el orquestador en TODOS los paneles
   * cuando un turno empieza/termina (M1). No lanza ni corta turnos. */
  function setCorriendoGlobal(v: boolean): void {
    corriendo = v;
    entrada.setCorriendo(v);
  }

  // Sub-módulos del panel (acciones, historial, carga, turno). Los callbacks
  // que se pasan entre ellos solo se ejecutan a runtime, cuando las cuatro
  // instancias ya existen, así que el orden de creación no impone ciclo.
  const historial = crearHistorial({
    mensajes,
    abrirAccionesMensaje: (id, rect) => acciones.abrirAccionesMensaje(id, rect),
    copiarUltimoTramo: () => acciones.copiarUltimoTramo(),
    aviso: (texto, meta, detalle) => acciones.avisoChat(texto, meta, detalle),
  });
  const acciones = crearAcciones({
    mensajes,
    entrada,
    hayTurnoGlobal: () => d.hayTurnoGlobal(),
    getTextoUsuario: (id) => historial.getTextoUsuario(id),
    getVolverA: () => (id) => carga.volverA(id),
    limpiarHistorial: () => historial.limpiarHistorial(),
  });
  const carga = crearCarga({
    d,
    tipo,
    mensajes,
    entrada,
    cabecera,
    fijarConversaId: (id) => {
      conversaId = id;
    },
    limpiarChat: () => acciones.limpiarChat(),
    pintarHistorial: (h, a, u) => historial.pintarHistorial(h, a, u),
    aviso: (texto, meta, detalle) => acciones.avisoChat(texto, meta, detalle),
  });
  // [109A-4 F2] Comandos `/`: catálogo del área activa (vivo y recargable) y
  // ejecutor. El backend resuelve el área; el front no envía rutas.
  const comandosArea = crearComandosArea({
    listar: () => d.adaptador.sesion.comandos.listar(),
    avisar: (texto) => acciones.avisoChat(texto, 'comandos del área', ''),
  });
  comandosArea.alCambiar(() => entrada.setComandosProyecto(comandosArea.lista()));
  void comandosArea.refrescar();
  const comandos = crearEjecutorComandos({
    aviso: (texto, meta, detalle) => acciones.avisoChat(texto, meta, detalle),
    comandosProyecto: () => comandosArea.lista(),
    expandirComando: (nombre, argumentos) =>
      d.adaptador.sesion.comandos.expandir(nombre, argumentos),
    limpiarPanel: () => carga.ponerBorrador(),
    contexto: () => entrada.getContexto(),
    proveedores: () => opts.proveedores ?? [],
    modeloActual: () => d.getModelo(),
    cambiarModelo: (modelo) => opts.onModeloCambiado?.(modelo),
    compactar: (instruccion) => d.adaptador.sesion.compactar(tipo, instruccion),
  });

  const turno = crearTurno({
    d,
    tipo,
    mensajes,
    entrada,
    cabecera,
    getPanel: () => panel,
    getCorriendo: () => corriendo,
    getConversaId: () => conversaId,
    fijarConversaId: (id) => {
      conversaId = id;
    },
    aplicarCarga: (c) => carga.aplicarCarga(c),
    anadirPieTurno: (u) => historial.anadirPieTurno(u),
    aviso: (texto, meta, detalle) => acciones.avisoChat(texto, meta, detalle),
    comandos,
  });

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
      await carga.cargarConversacion(id);
    },
    async nuevaConversacion() {
      await carga.nuevaConversacion();
    },
    reanudarUltimo() {
      turno.reanudarUltimo();
    },
    setCorriendoGlobal(v: boolean) {
      setCorriendoGlobal(v);
    },
    getInicioTurno() {
      return turno.getInicioTurno();
    },
    enfocarEntrada() {
      entrada.enfocar();
    },
    ponerTitulo(texto: string) {
      cabecera.ponerTitulo(texto);
    },
    limpiar() {
      acciones.limpiarChat();
    },
    ponerBorrador() {
      carga.ponerBorrador();
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
      acciones.avisoChat(texto, meta, detalle);
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
      return historial.listarCambios();
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
