// Punto de entrada del front de glory-harness desktop (plan 039A-3).
// ORQUESTADOR DELGADO: posee los elementos de layout NO duplicables
// (sidebar + grip + #paneles + modal + panelMeta global M1) y delega
// cada chat (cabecera+mensajes+entrada) en la fábrica `montarPanelChat`
// (componentes/panelChat.ts). El estado compartido del runtime M1
// (modelo/modo/razonamiento + turno global) vive aquí y se inyecta por
// `deps`; el panel que lanza un turno es el destino (payloads sin
// panel_id, ver backend 045f7e1).

import './estilos/index.css';

import { CONVERSACIONES } from './datos/conversaciones';
import { MODELO_INICIAL, PROVEEDORES } from './dominio/catalogoModelos';

import { montarVistaModal } from './orquestador/vistaModal';
import { ejecutarArranque } from './orquestador/arranque';
import { crearGanchos } from './orquestador/ganchos';
import { crearSesionVista } from './orquestador/sesionVista';
import { crearGestorPaneles } from './orquestador/paneles';
import { montarVistaBarra } from './orquestador/vistaBarra';
import { montarVistaMeta } from './orquestador/vistaMeta';
import './estilos/modalProyecto.css';
import type { PanelChat } from './componentes/panelChat';
import { montarNavegadorVista } from './orquestador/navegadorVista';
import { montarPanelDerechoTodo } from './orquestador/panelDerecho';

import { crearSimulacion } from './simulacion/simulacion';
import { crearAdaptadorReal, esEntornoTauri } from './tauri/real';
import { crearAdaptadorApi } from './adaptadores/api';
import type { HooksAdaptador } from './tauri/real';
import { cuerpo as cuerpoDocumento, el, porId } from './util/dom';
import { alRedimensionar } from './plataforma/ventana';
import {
  abrirAccionesPanel,
  abrirChatLateralVacio,
  abrirEnLateral,
  type LateralesDeps,
} from './orquestador/laterales';
import { crearPanel, type CrearPanelDeps } from './orquestador/crearPanel';
import { montarBarraLateral } from './orquestador/barraLateral';
import {
  CLAVE_TEMA_OSCURO,
  RAZONAMIENTO_ETIQUETA,
  aplicarTemaOscuro,
  opcionesArranque,
  resolverEntorno,
} from './orquestador/entorno';
import { type PersistenciaDeps } from './orquestador/persistencia';

const raizApp = porId('app');
if (!raizApp) throw new Error('falta #app');

// ---------- Layout raíz ----------
// `index.html` ya aporta el único #app. Reutilizarlo evita anidar otro
// contenedor con `height: 100vh`, que en modo web hacía crecer el documento.
const app = raizApp;
const cuerpo = el('div');
cuerpo.id = 'cuerpo';
// #paneles es el contenedor flex de los chats duplicables (1-2).
const paneles = el('div');
paneles.id = 'paneles';

// ---------- Paneles: registro y activación ----------
/* [109A-6] `panelActivo`/`activarPanel`/`avisoGlobal` viven en
 * `orquestador/paneles`. Se crean AQUÍ, antes de la barra, porque la barra
 * recibe `avisar` en su montaje: con una declaración de función bastaba el
 * hoisting, con el gestor hay que inicializarlo antes de usarlo. La sidebar y
 * el panel derecho aún no existen y llegan como cierres perezosos de runtime. */
const panelesRegistrados: PanelChat[] = [];
const gestorPaneles = crearGestorPaneles({
  paneles: panelesRegistrados,
  getSidebar: () => sidebar,
  getPanelDerecho: () => panelDerecho,
});
const { panelActivo, activarPanel, avisoGlobal } = gestorPaneles;

// [089A-3] Barra superior global estilo Synara (toggles + arrastre +
// botonera caption). Vive en `orquestador/vistaBarra`; los toggles son
// cierres de runtime (sidebar y panel derecho se crean más abajo).
const barra = montarVistaBarra({
  alternarSidebar: () => alternarSidebar(),
  alternarPanelDerecho: () => alternarPanelDerecho(),
  avisar: avisoGlobal,
  // [089A-5] Historial de la app (cierres de runtime sobre la barra lateral,
  // creada más abajo; mismo patrón que `alternarSidebar`).
  onAtras: () => barraLateral.irAtrasHistorial(),
  onAdelante: () => barraLateral.irAdelanteHistorial(),
});

// ---------- Estado compartido M1 (runtime único) ----------
/* El estado vista (modelo/modo/razonamiento) vive en `orquestador/vistaModal`
 * y el turno global en `orquestador/vistaMeta`; aquí no quedan lets. */

const ent = resolverEntorno(esEntornoTauri());
const USA_TAURI = ent.usaTauri;
const BASE_API = ent.baseApi;
const USA_REAL = ent.usaReal;
const USA_MOCK = ent.usaMock;
const MODO_TEXTO = ent.modoTexto;

const simulacion = crearSimulacion();

// ---------- Sesión/proyectos (conversaciones + workspaces) ----------
// Vive en `orquestador/sesionVista`. `adaptador`, `barraLateral` y
// `todoVistaModal` llegan como cierres de runtime (se crean más abajo).
const sesionVista = crearSesionVista({
  usaReal: USA_REAL,
  conversacionesIniciales: USA_REAL ? [] : CONVERSACIONES.map((c) => ({ ...c })),
  paneles: () => panelesRegistrados,
  getSidebar: () => barraLateral.sidebar,
  getEstadoVista: () => todoVistaModal.estado,
  setModeloEnModal: (m) => todoVistaModal.modal.setModelo(m),
  listarConversaciones: () => adaptador.sesion.listar(),
  listarWorkspaces: () => adaptador.sesion.workspaces.listar(),
});

// ---------- PanelMeta + turno global (M1) ----------
// Vive en `orquestador/vistaMeta`. `adaptador`/`todoVistaModal` llegan como
// cierres de runtime (se crean más abajo), fuera de la TDZ.
const vistaMeta = montarVistaMeta({
  usaReal: USA_REAL,
  sesion: {
    actualizarMeta: (valor) => adaptador.sesion.actualizarMeta(valor),
    aplicarMeta: (comando) => adaptador.sesion.metaAplicar(comando),
    leerMeta: (id) => adaptador.sesion.metaLeer(id),
    ultimoTurnoId: () => adaptador.ultimoTurnoId(),
  },
  detenerReal: () => adaptador.detener(),
  detenerMock: () => simulacion.detener(),
  paneles: () => panelesRegistrados,
  avisar: avisoGlobal,
});
const panelMeta = vistaMeta.panelMeta;

// ---------- Backend real: adaptador (compartido). ----------
// Los ganchos viven en `orquestador/ganchos`; las piezas creadas más abajo
// (modal, navegador, files) llegan como cierres perezosos de runtime.
const hooksAdaptador: HooksAdaptador = crearGanchos({
  usaReal: USA_REAL,
  sincronizarModeloDesdeSesion: sesionVista.sincronizarModeloDesdeSesion,
  asignarWorkspace: (ws) => todoVistaModal.modal.asignarValor('workspace', ws),
  refrescarProyectos: sesionVista.refrescarProyectos,
  resincronizarSidebar: sesionVista.resincronizarSidebar,
  paneles: () => panelesRegistrados,
  getNavegador: () => todoNavegador.navegador,
  avisar: avisoGlobal,
  registrarCambioArchivo: (cambio) => files.registrarCambio(cambio),
});
// Tauri → IPC in-process; web (`?api=`/`gh_api`/mismo origen) → HTTP/SSE.
// `adaptador` se usa en cierres de runtime; en modo ni-ni nunca se monta.
const adaptador = USA_TAURI
  ? crearAdaptadorReal(hooksAdaptador)
  : crearAdaptadorApi(BASE_API ?? '', hooksAdaptador);

// ---------- Gestión de paneles (1 principal + 0..N laterales) ----------
// [089A-2] Cada lateral vive en su propia tab `chat:<conversaId>` del panel
// derecho (multi-chat estilo Paseo): ya no hay límite de 2 paneles.

// Persistencia de preferencias: se resuelve contra el adaptador una vez
// creado (las llamadas son todas en runtime, tras el montaje).
const depsPersistencia: PersistenciaDeps = {
  usaReal: USA_REAL,
  usaTauri: USA_TAURI,
  guardarConfig: (clave, valor) => adaptador.sesion.configGuardar(clave, valor),
  leerConfig: (clave) => adaptador.sesion.configLeer(clave),
  avisar: (texto) => avisoGlobal(texto, '', ''),
};

// ---------- Sidebar ----------
/* La sidebar vive en `orquestador/barraLateral` (montaje, grip,
 * auto-ocultado, renombrado). Los cierres sobre modal, modalProyecto,
 * navegador, principal y laterales son de runtime (eventos de UI). */
const barraLateral = montarBarraLateral({
  cuerpo,
  barra,
  adaptador,
  usaReal: USA_REAL,
  usaMock: USA_MOCK,
  persistencia: depsPersistencia,
  conversacionesIniciales: sesionVista.getConversaciones(),
  proyectosIniciales: sesionVista.getProyectos(),
  proyectoActivoInicial: sesionVista.getProyectoActivo(),
  getConversaciones: sesionVista.getConversaciones,
  setConversaciones: sesionVista.setConversaciones,
  getTurnoGlobal: () => vistaMeta.hayTurnoGlobal(),
  getProyectoRutaActiva: () => sesionVista.getProyectoActivo()?.ruta ?? null,
  paneles: panelesRegistrados,
  panelActivo,
  activarPanel,
  avisar: avisoGlobal,
  resincronizarSidebar: sesionVista.resincronizarSidebar,
  abrirEnLateral: (id) => abrirEnLateral(depsLaterales, id),
  abrirConfig: () => todoVistaModal.modal.abrir(),
  abrirModalProyecto: () => todoVistaModal.modalProyecto.abrir(),
  alternarNavegador: () => {
    if (todoNavegador.estaAbierto()) todoNavegador.cerrarNavegador();
    else todoNavegador.abrirNavegador();
  },
  getPrincipal: () => principal,
});
const sidebar = barraLateral.sidebar;
const grip = barraLateral.grip;
const alternarSidebar = barraLateral.alternarSidebar;

// Escucha resize para el auto-ocultado de la lista por ancho mínimo.
alRedimensionar(() => barraLateral.pintarSidebar());

// ---------- Modal (config + nuevo proyecto + estado vista) ----------
/* Vive en `orquestador/vistaModal`. Se crea aquí (antes de la fábrica de
 * paneles) porque `depsCrearPanel` comparte su `estado`; los usos previos
 * (`sincronizarModeloDesdeSesion`, `onSesion`, sidebar) son cierres de
 * runtime, fuera de la TDZ. */
const todoVistaModal = montarVistaModal({
  modeloInicial: MODELO_INICIAL,
  modoInicial: 'predeterminado',
  razonamientoInicial: 'medium',
  proveedores: PROVEEDORES,
  claveTemaOscuro: CLAVE_TEMA_OSCURO,
  etiquetasRazonamiento: RAZONAMIENTO_ETIQUETA,
  paneles: panelesRegistrados,
  usaReal: USA_REAL,
  usaTauri: USA_TAURI,
  sincronizarPanelMeta: vistaMeta.sincronizarPanelMeta,
  aplicarTemaOscuro,
  configGuardar: (id, valor) => adaptador.sesion.configGuardar(id, valor),
  configGuardarModelo: async (nuevo) => {
    await adaptador.sesion.configGuardar('proveedor', nuevo.proveedor);
    await adaptador.sesion.configGuardar('modelo', nuevo.modelo);
  },
  guardarProyecto: async (nombre, ruta) => {
    if (!USA_REAL) return;
    await adaptador.sesion.workspaces.guardarProyecto(nombre, ruta);
  },
  // [109A-3] Memorias del proyecto activo: el ámbito lo resuelve el backend
  // (área activa de la sesión). En modo web el transporte rechaza con su
  // motivo, que el panel muestra tal cual; el aviso lo añade el modal.
  memoria: adaptador.sesion.memorias,
  hayTurno: () => vistaMeta.hayTurnoGlobal(),
  avisar: avisoGlobal,
  ponerBorradorPrincipal: () => principal.ponerBorrador(),
  activarPrincipal: () => activarPanel(principal),
});

/* Deps de la fábrica de paneles. `abrirAcciones` cierra sobre `depsLaterales`
 * y `getPrincipal` sobre `principal` (ambos declarados más abajo): solo se
 * invocan desde eventos de UI posteriores al arranque, fuera de la TDZ. */
const depsCrearPanel: CrearPanelDeps = {
  paneles: panelesRegistrados,
  adaptador,
  simulacion,
  usaReal: USA_REAL,
  usaMock: USA_MOCK,
  panelMeta,
  sidebar,
  modal: todoVistaModal.modal,
  proveedores: PROVEEDORES,
  getConversaciones: sesionVista.getConversaciones,
  getModelo: () => todoVistaModal.estado.modelo,
  setModelo: (m) => {
    todoVistaModal.estado.modelo = m;
  },
  getModo: () => todoVistaModal.estado.modo,
  setModo: (m) => {
    todoVistaModal.estado.modo = m;
  },
  getRazonamiento: () => todoVistaModal.estado.razonamiento,
  setRazonamiento: (r) => {
    todoVistaModal.estado.razonamiento = r;
  },
  getProyectos: sesionVista.getProyectos,
  getProyectoActivoId: sesionVista.getProyectoActivoId,
  getPrincipal: () => principal,
  hayTurnoGlobal: () => vistaMeta.hayTurnoGlobal(),
  notificarTurnoInicio: () => vistaMeta.notificarTurnoInicio(),
  notificarTurnoFin: () =>
    vistaMeta.notificarTurnoFin(() => {
      if (USA_REAL) void sesionVista.resincronizarSidebar();
    }),
  registrarUltimoEnvio: (panel) => vistaMeta.registrarUltimoEnvio(panel),
  panelActivo,
  activarPanel,
  resincronizarSidebar: sesionVista.resincronizarSidebar,
  sincronizarPanelMeta: vistaMeta.sincronizarPanelMeta,
  avisar: avisoGlobal,
  alternarSidebar,
  // `todoPanelDerecho` se crea más abajo (tras el navegador): cierre de runtime.
  alternarPanelDerecho: () => todoPanelDerecho.alternarPanelDerecho(),
  abrirAcciones: (panel, rect) => abrirAccionesPanel(depsLaterales, panel, rect),
};

// ---------- Panel principal ----------
const principal = crearPanel(depsCrearPanel, 'principal', 'principal');

/* El navegador vive en `orquestador/navegadorVista`. Las piezas del panel
 * derecho llegan como cierres perezosos (ese módulo se crea justo debajo)
 * y solo se invocan en runtime, fuera de la TDZ. */
const todoNavegador = montarNavegadorVista({
  usaTauri: USA_TAURI,
  panelActivo,
  activarPanel,
  avisar: avisoGlobal,
  asegurarPanelDerecho: () => todoPanelDerecho.asegurarPanelDerecho(),
  cerrarPanelDerechoSiVacio: () => todoPanelDerecho.cerrarPanelDerechoSiVacio(),
  abrirTabNavegador: (raiz, onCerrar) => {
    todoPanelDerecho.panelDerecho.abrirTab('navegador', 'Navegador', raiz, onCerrar);
  },
  cerrarTabNavegador: () => {
    todoPanelDerecho.panelDerecho.cerrarTab('navegador');
  },
});
/* El panel derecho vive en `orquestador/panelDerecho` (Files, Git, tabs,
 * grip de ancho, visibilidad). `abrirNavegador`/`abrirChatLateral` son
 * cierres de runtime sobre piezas declaradas más abajo. */
const todoPanelDerecho = montarPanelDerechoTodo({
  cuerpo,
  app,
  barra,
  adaptador,
  usaTauri: USA_TAURI,
  persistencia: depsPersistencia,
  paneles: panelesRegistrados,
  panelActivo,
  getConversaciones: sesionVista.getConversaciones,
  onCambioWorkspace: sesionVista.onCambioWorkspace,
  navegadorRaiz: todoNavegador.navegador.raiz,
  mostrarNavegador: (v) => todoNavegador.navegador.mostrar(v),
  estaNavegadorAbierto: todoNavegador.estaAbierto,
  abrirNavegador: todoNavegador.abrirNavegador,
  abrirChatLateral: () => abrirChatLateralVacio(depsLaterales),
  abrirChatLateralPorId: (id) => abrirEnLateral(depsLaterales, id),
});
const panelDerecho = todoPanelDerecho.panelDerecho;
const files = todoPanelDerecho.files;
const toastGlobal = todoPanelDerecho.toastGlobal;
const asegurarPanelDerecho = todoPanelDerecho.asegurarPanelDerecho;
const cerrarPanelDerechoSiVacio = todoPanelDerecho.cerrarPanelDerechoSiVacio;
const alternarPanelDerecho = todoPanelDerecho.alternarPanelDerecho;
const pintarToggleDerecho = todoPanelDerecho.pintarToggleDerecho;

// Dependencias de los laterales: se resuelven aquí porque el panel derecho,
// la sidebar y la fábrica de paneles ya existen; los usos anteriores son
// cierres de runtime (clic), cuando todo ya está definido.
const depsLaterales: LateralesDeps = {
  paneles: panelesRegistrados,
  getConversaciones: sesionVista.getConversaciones,
  crearPanel: (tipo, idPrefijo, opts) => crearPanel(depsCrearPanel, tipo, idPrefijo, opts),
  activarPanel,
  panelActivo,
  asegurarPanelDerecho,
  cerrarPanelDerechoSiVacio,
  panelDerecho,
  sidebar,
  avisar: (texto, meta, detalle) => avisoGlobal(texto, meta, detalle),
  renombrarEnLista: (id, titulo) => barraLateral.renombrarEnLista(id, titulo),
};

// ---------- Montaje del DOM ----------
cuerpo.append(sidebar.raiz, grip);
paneles.appendChild(principal.raiz);
cuerpo.appendChild(paneles);
// [089A-2] El navegador vive detached hasta abrir su tab del panel derecho
// (que se monta bajo demanda con `asegurarPanelDerecho`).
// [089A-3] La barra superior global va primera (a todo el ancho, por
// encima de sidebar/paneles/panel derecho). `app` ya es `raizApp`, así que no
// se vuelve a insertar a sí mismo (eso provoca un HierarchyRequestError).
app.appendChild(barra.raiz);
app.appendChild(cuerpo);

/* El arranque (estado inicial, reloj, historial, sesión real) vive en
 * `orquestador/arranque` y se ejecuta al final del fichero. */

// Añade las capas globales fuera de #app (hermanas del layout).
cuerpoDocumento().appendChild(toastGlobal.raiz);
cuerpoDocumento().appendChild(todoVistaModal.modal.raiz);

// [069A-Proyectos] Modal "Nuevo proyecto" autocontenido (en vistaModal).
cuerpoDocumento().appendChild(todoVistaModal.modalProyecto.raiz);

// ---------- Arranque: estado inicial + sesión + última conversación ----------
ejecutarArranque({
  cuerpo,
  persistencia: depsPersistencia,
  barraLateral,
  pintarToggleDerecho,
  restaurarPanelDerecho: () => todoPanelDerecho.restaurarEstado(),
  paneles: panelesRegistrados,
  panelMeta,
  usaReal: USA_REAL,
  usaMock: USA_MOCK,
  usaTauri: USA_TAURI,
  baseApi: BASE_API,
  modoTexto: MODO_TEXTO,
  hayTurno: () => vistaMeta.hayTurnoGlobal(),
  panelActivo,
  usoUltimoTurno: () => adaptador.usoUltimoTurno(),
  asegurarSesion: () => adaptador.asegurarSesion(opcionesArranque()),
  configLeer: (id) => adaptador.sesion.configLeer(id),
  aplicarSesionGuardada: (sesion) => todoVistaModal.aplicarSesionGuardada(sesion),
  sincronizarPanelMeta: vistaMeta.sincronizarPanelMeta,
  resincronizarSidebar: sesionVista.resincronizarSidebar,
  claveTemaOscuro: CLAVE_TEMA_OSCURO,
  getConversaciones: sesionVista.getConversaciones,
  principal,
  seleccionarSidebar: (id) => sidebar.seleccionar(id),
  activarPanel,
});
