// ============================================================
// Punto de entrada del front de glory-harness desktop (plan 039A-3).
// ORQUESTADOR DELGADO: posee los elementos de layout NO duplicables
// (sidebar + grip + #paneles + modal + panelMeta global M1) y delega
// cada chat (cabecera+mensajes+entrada) en la fábrica `montarPanelChat`
// (componentes/panelChat.ts). El estado compartido del runtime M1
// (modelo/modo/razonamiento + turno global) vive aquí y se inyecta por
// `deps`; el panel que lanza un turno es el destino (payloads sin
// panel_id, ver backend 045f7e1).
// ============================================================

import './estilos/index.css';

import { CONVERSACIONES } from './datos/conversaciones';
import { historialEjemplo } from './datos/historialEjemplo';
import { MODELO_INICIAL, PROVEEDORES } from './dominio/catalogoModelos';
import type { Conversacion, ModeloSeleccionado, Workspace } from './dominio/tipos';

import type { ModoEjecucion } from './componentes/entrada';
import { montarModalConfiguracion } from './componentes/modal';
import { montarModalProyecto } from './componentes/modalProyecto';
import './estilos/modalProyecto.css';
import { montarPanelMeta } from './componentes/panelMeta';
import type { PanelChat } from './componentes/panelChat';
import { montarBarraSuperior } from './componentes/barraSuperior';
import { montarNavegadorVista } from './orquestador/navegadorVista';
import { montarPanelDerechoTodo } from './orquestador/panelDerecho';
import { renderizarBloque } from './componentes/mensajes';

import { crearSimulacion } from './simulacion/simulacion';
import { invoke } from '@tauri-apps/api/core';
import { crearAdaptadorReal, esEntornoTauri } from './tauri/real';
import { crearAdaptadorApi } from './adaptadores/api';
import type { HooksAdaptador, InfoSesion, UsoTurno } from './tauri/real';
import { el } from './util/dom';
import {
  abrirAccionesPanel,
  abrirChatLateralVacio,
  abrirEnLateral,
  type LateralesDeps,
} from './orquestador/laterales';
import { crearPanel, type CrearPanelDeps } from './orquestador/crearPanel';
import { montarBarraLateral, CLAVE_ANCHO, CLAVE_COLAPSADA } from './orquestador/barraLateral';
import {
  CLAVE_TEMA_OSCURO,
  RAZONAMIENTO_ETIQUETA,
  aplicarTemaOscuro,
  opcionesArranque,
  resolverEntorno,
} from './orquestador/entorno';
import { leerSidebar, type PersistenciaDeps } from './orquestador/persistencia';

const raizApp = document.getElementById('app');
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

// [089A-3] Barra superior global estilo Synara (toggles + arrastre +
// botonera caption). Se monta como primera hija de #app (ver montaje).
// Los callbacks existen como declaraciones hoisted más abajo; solo se
// invocan en runtime (clic), cuando todo ya está definido.
const barra = montarBarraSuperior({
  onAlternarSidebar() {
    alternarSidebar();
  },
  // [089A-3] Atrás/adelante replican la navegación por historial de Synara;
  // lógica pendiente (roadmap): arrancan deshabilitados.
  onAtras() {
    /* pendiente: historial de la app */
  },
  onAdelante() {
    /* pendiente: historial de la app */
  },
  onAlternarPanelDerecho() {
    alternarPanelDerecho();
  },
});
barra.setPuedeNavegar(false, false);

// ---------- Estado compartido M1 (runtime único) ----------
let modeloActual: ModeloSeleccionado = MODELO_INICIAL;
let modoActual: ModoEjecucion = 'predeterminado';
let razonamientoActual = 'medium';
// Flag global: algún panel tiene turno en curso (M1: solo uno a la vez).
let turnoGlobal = false;
// Panel que envió el último mensaje (para el play/reanudar del panelMeta).
let panelUltimoEnvio: PanelChat | null = null;

const ent = resolverEntorno(esEntornoTauri());
const USA_TAURI = ent.usaTauri;
const BASE_API = ent.baseApi;
const USA_REAL = ent.usaReal;
const USA_MOCK = ent.usaMock;
const MODO_TEXTO = ent.modoTexto;

const simulacion = crearSimulacion();

// Lista de conversaciones (fuente para la sidebar y el ⋯ de cabecera).
let conversaciones: Conversacion[] = USA_REAL
  ? []
  : CONVERSACIONES.map((c) => ({ ...c }));

// [069A-Proyectos] Proyectos registrados en la sesión + activo actual.
let proyectos: Workspace[] = [];
let proyectoActivo: Workspace | null = null;
// [089A-11] Suscriptores al cambio de workspace activo (área de trabajo):
// Files/Git dependen de la raíz que resuelve el backend, así que recargan
// cuando el área cambia y su tab está abierta. El orquestador registra la
// acción donde se montan los paneles (evita referencias previas al montaje).
const suscriptoresCambioWorkspace: Array<(ruta: string | null) => void> = [];
function onCambioWorkspace(accion: (ruta: string | null) => void): void {
  suscriptoresCambioWorkspace.push(accion);
}

// ---------- PanelMeta global (M1): lo monta el orquestador dentro de la
// entrada del principal (el lateral no tiene panelMeta propio). ----------
const panelMeta = montarPanelMeta({
  onMetaCambiada(meta) {
    if (!USA_REAL) return;
    const valor = meta.trim() ? meta.trim() : null;
    void adaptador.sesion
      .actualizarMeta(valor)
      .catch((e: unknown) => avisoGlobal(`no se pudo fijar la meta: ${String(e)}`, '', ''));
  },
  onPausar() {
    if (!turnoGlobal) return;
    // Pausar = cancelar el turno global en curso (M1).
    if (USA_REAL) adaptador.detener();
    else simulacion.detener();
    panelMeta.setEstado('pausado');
    panelesRegistrados.forEach((p) => p.setCorriendoGlobal(false));
    turnoGlobal = false;
  },
  onReanudar() {
    if (turnoGlobal) return;
    if (!panelUltimoEnvio) {
      avisoGlobal('nada que reanudar: envía un mensaje primero', '', '');
      return;
    }
    panelUltimoEnvio.reanudarUltimo();
  },
});

// ---------- Backend real: adaptador (compartido). ----------
// onSesion se ejecuta en runtime (tras montar todo); `modal` es seguro.
const hooksAdaptador: HooksAdaptador = {
  onSesion(info: InfoSesion) {
    sincronizarModeloDesdeSesion(info);
    const ws = info.workspace;
    if (ws && ws !== '<desconocido>') modal.asignarValor('workspace', ws);
    // [069A-Proyectos] Tras cambiar de workspace (proyecto), refrescar
    // la lista de proyectos + conversaciones. No esperar si falla.
    if (USA_REAL) {
      void refrescarProyectos().then(() => resincronizarSidebar());
    }
  },
  // [039A-3 P6] El `ContextoDetalle`/`usage` del backend repinta el indicador
  // circular de TODOS los paneles con el % y la ventana real (fuente única).
  // Seguro: se ejecuta en runtime, cuando `panelesRegistrados` ya existe.
  onContexto(u: UsoTurno) {
    panelesRegistrados.forEach((p) =>
      p.setContexto({
        pct: u.ocupacionPct,
        maxVentana: u.maxVentana,
        reservaSalida: u.reservaSalida,
        totalEntrada: u.totalEntrada,
      }),
    );
  },
  // [069A-1 F6] Refleja las tools de navegador del agente en el panel UI,
  // incluyendo captura base64 para mostrar la imagen.
  onToolNavegador(ev) {
    const urlPart = ev.url ? ` (${ev.url.slice(0, 60)})` : '';
    navegador.registrarAccion({
      herramienta: ev.accion,
      descripcion: `${ev.descripcion}${urlPart}`,
      ok: ev.ok,
      tiempo: Date.now(),
    });
    if (ev.ok && ev.url) navegador.fijarURL(ev.url);
    // [069A-1 F6] Mostrar captura base64 si viene en el evento
    if (ev.accion === 'capturar' && ev.captura_base64) {
      navegador.actualizarCaptura(ev.captura_base64);
    }
  },
  // [069A-2 F4] Estado de conexión SSE (solo modo web): visible, nunca
  // silencioso. Seguro: `avisoGlobal` solo corre en runtime.
  onConexion(estado, detalle) {
    if (estado !== 'en-linea') avisoGlobal(`backend web: ${estado}`, '', detalle ?? '');
  },
  // [089A-12] Cambios de archivos del agente → preview integrado en Files.
  // Seguro: corre en runtime, cuando `files` ya existe.
  onCambioArchivo(cambio) {
    files.registrarCambio(cambio);
  },
};
// Tauri → IPC in-process; web (`?api=`/`gh_api`/mismo origen) → HTTP/SSE.
// `adaptador` se usa en cierres de runtime; en modo ni-ni nunca se monta.
const adaptador = USA_TAURI
  ? crearAdaptadorReal(hooksAdaptador)
  : crearAdaptadorApi(BASE_API ?? '', hooksAdaptador);

// ---------- Gestión de paneles (1 principal + 0..N laterales) ----------
// [089A-2] Cada lateral vive en su propia tab `chat:<conversaId>` del panel
// derecho (multi-chat estilo Paseo): ya no hay límite de 2 paneles.
const panelesRegistrados: PanelChat[] = [];

function panelActivo(): PanelChat | null {
  // Último panel enfocado; si ninguno, el principal.
  return (
    panelesRegistrados.find((p) => p.raiz.classList.contains('enfocado')) ??
    panelesRegistrados.find((p) => p.tipo === 'principal') ??
    null
  );
}

function activarPanel(panel: PanelChat | null): void {
  if (!panel) return;
  panelesRegistrados.forEach((p) => {
    if (p !== panel) p.desenfocar();
  });
  panel.activar();
  const id = panel.conversaId;
  // [069A-7] `null` = borrador (sin conversación): deselecciona la sidebar.
  if (id) sidebar.seleccionar(id);
  else sidebar.seleccionar('');
  // [089A-4] Gating del inicio: "Chat lateral" solo con conversación activa.
  panelDerecho.fijarInicioChatDisponible(id != null);
}

function hayTurnoGlobal(): boolean {
  return turnoGlobal;
}

function notificarTurnoInicio(): void {
  turnoGlobal = true;
  panelesRegistrados.forEach((p) => p.setCorriendoGlobal(true));
}

function notificarTurnoFin(): void {
  turnoGlobal = false;
  panelesRegistrados.forEach((p) => p.setCorriendoGlobal(false));
  if (USA_REAL) void resincronizarSidebar();
}

function registrarUltimoEnvio(panel: PanelChat): void {
  panelUltimoEnvio = panel;
}

function avisoGlobal(texto: string, meta: string, detalle: string): void {
  panelActivo()?.avisoLocal(texto, meta, detalle);
}

function sincronizarModeloDesdeSesion(info: InfoSesion): void {
  const corte = info.modelo.indexOf('/');
  if (corte < 0) return;
  const proveedor = info.modelo.slice(0, corte);
  const modelo = info.modelo.slice(corte + 1);
  if (proveedor === modeloActual.proveedor && modelo === modeloActual.modelo) return;
  modeloActual = { proveedor, modelo, nombre: modelo };
  panelesRegistrados.forEach((p) => p.setModelo(modeloActual));
  modal.setModelo(modeloActual);
}

async function resincronizarSidebar(): Promise<void> {
  if (!USA_REAL) return;
  await refrescarProyectos();
  const lista = await adaptador.sesion.listar();
  conversaciones = lista.map((c) => ({
    id: c.id,
    titulo: c.titulo,
    archivada: c.archivada,
    workspaceId: c.workspace_id,
    workspaceNombre: c.workspace_nombre,
  }));
  sidebar.sustituir(conversaciones);
}

/** [069A-Proyectos] Refresca el estado de proyectos desde el backend. */
async function refrescarProyectos(): Promise<void> {
  if (!USA_REAL) return;
  try {
    const res = await adaptador.sesion.workspaces.listar();
    proyectos = res.workspaces;
    const rutaAnterior = proyectoActivo?.ruta ?? null;
    const rutaNueva = res.activa?.ruta ?? null;
    proyectoActivo = res.activa;
    sidebar.sustituirProyectos(proyectos, proyectoActivo);
    // Sincroniza el selector de workspace de todos los paneles.
    const seleccionadoId = proyectoActivo?.id ?? null;
    panelesRegistrados.forEach((p) => p.setWorkspaces(proyectos, seleccionadoId));
    // [089A-11] El área de trabajo cambió (ruta distinta): Files/Git
    // resuelven la raíz en el backend, así que recargan las tabs abiertas.
    if (rutaAnterior !== rutaNueva) {
      suscriptoresCambioWorkspace.forEach((fn) => fn(rutaNueva));
    }
  } catch {
    // silencioso: el sidebar conserva el último estado válido
  }
}

// Persistencia de preferencias: se resuelve contra el adaptador una vez
// creado (las llamadas son todas en runtime, tras el montaje).
const depsPersistencia: PersistenciaDeps = {
  usaReal: USA_REAL,
  usaTauri: USA_TAURI,
  guardarConfig: (clave, valor) => adaptador.sesion.configGuardar(clave, valor),
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
  conversacionesIniciales: conversaciones,
  proyectosIniciales: proyectos,
  proyectoActivoInicial: proyectoActivo,
  getConversaciones: () => conversaciones,
  setConversaciones: (c) => {
    conversaciones = c;
  },
  getTurnoGlobal: () => turnoGlobal,
  paneles: panelesRegistrados,
  panelActivo,
  activarPanel,
  avisar: avisoGlobal,
  resincronizarSidebar,
  abrirEnLateral: (id) => abrirEnLateral(depsLaterales, id),
  abrirConfig: () => modal.abrir(),
  abrirModalProyecto: () => modalProyecto.abrir(),
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
window.addEventListener('resize', () => barraLateral.pintarSidebar());

function sincronizarPanelMeta(): void {
  // La meta solo es editable y aplicable en ese modo. Mantener el panel
  // oculto fuera de `meta` evita sugerir que un turno autónomo la ejecutará;
  // el backend también la ignora fuera de ese modo como segunda barrera.
  const hayConversacion = panelesRegistrados.some((p) => p.conversaId !== null);
  panelMeta.mostrar(modoActual === 'meta' && hayConversacion);
}

// ---------- Modal ----------
const modal = montarModalConfiguracion({
  modelo: modeloActual,
  proveedores: PROVEEDORES,
  modo: modoActual,
  razonamiento: razonamientoActual,
  onCambio(id, valor) {
    if (id === 'modo') {
      modoActual = valor as ModoEjecucion;
      panelesRegistrados.forEach((p) => p.setModo(modoActual));
      sincronizarPanelMeta();
    } else if (id === 'nivelRazonamiento') {
      razonamientoActual = String(valor);
      panelesRegistrados.forEach((p) => p.setRazonamiento(razonamientoActual));
    } else if (id === 'contexto_max_ventana') {
      // [039A-3 P6] La ventana se persiste vía configGuardar (abajo); el
      // backend la consumirá al construir la sesión (inyección de
      // `contexto.max_ventana`, pendiente de P6 backend). No hay estado local
      // que actualizar: la fuente para el indicador es el ContextoDetalle.
    } else if (id === CLAVE_TEMA_OSCURO) {
      aplicarTemaOscuro(valor === true || valor === 'true');
    }
    if (
      USA_REAL &&
      (id === 'modo' || id === 'nivelRazonamiento' || id === 'contexto_max_ventana' || id === CLAVE_TEMA_OSCURO)
    ) {
      void adaptador.sesion
        .configGuardar(id, valor === true ? '1' : String(valor))
        .catch((e: unknown) => avisoGlobal(`no se pudo guardar ${id}: ${String(e)}`, '', ''));
    }
  },
  onModeloCambiado(nuevo) {
    modeloActual = nuevo;
    panelesRegistrados.forEach((p) => p.setModelo(nuevo));
    if (USA_REAL) {
      void adaptador.sesion
        .configGuardar('proveedor', nuevo.proveedor)
        .then(() => adaptador.sesion.configGuardar('modelo', nuevo.modelo))
        .catch((e: unknown) => avisoGlobal(`no se pudo guardar el modelo: ${String(e)}`, '', ''));
    }
  },
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
  modal,
  proveedores: PROVEEDORES,
  getConversaciones: () => conversaciones,
  getModelo: () => modeloActual,
  setModelo: (m) => {
    modeloActual = m;
  },
  getModo: () => modoActual,
  setModo: (m) => {
    modoActual = m;
  },
  getRazonamiento: () => razonamientoActual,
  setRazonamiento: (r) => {
    razonamientoActual = r;
  },
  getProyectos: () => proyectos,
  getProyectoActivoId: () => proyectoActivo?.id ?? null,
  getPrincipal: () => principal,
  hayTurnoGlobal,
  notificarTurnoInicio,
  notificarTurnoFin,
  registrarUltimoEnvio,
  panelActivo,
  activarPanel,
  resincronizarSidebar,
  sincronizarPanelMeta,
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
const navegador = todoNavegador.navegador;

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
  onCambioWorkspace,
  navegadorRaiz: todoNavegador.navegador.raiz,
  mostrarNavegador: (v) => todoNavegador.navegador.mostrar(v),
  estaNavegadorAbierto: todoNavegador.estaAbierto,
  abrirNavegador: todoNavegador.abrirNavegador,
  abrirChatLateral: () => abrirChatLateralVacio(depsLaterales),
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
  getConversaciones: () => conversaciones,
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
cuerpo.appendChild(sidebar.raiz);
cuerpo.appendChild(grip);
paneles.appendChild(principal.raiz);
cuerpo.appendChild(paneles);
// [089A-2] El navegador vive detached hasta abrir su tab del panel derecho
// (que se monta bajo demanda con `asegurarPanelDerecho`).
// [089A-3] La barra superior global va primera (a todo el ancho, por
// encima de sidebar/paneles/panel derecho). `app` ya es `raizApp`, así que no
// se vuelve a insertar a sí mismo (eso provoca un HierarchyRequestError).
app.appendChild(barra.raiz);
app.appendChild(cuerpo);

// Estado inicial de la sidebar (ancho/colapso persistidos + selección).
function pintarEstadoInicial(): void {
  const ancho = leerSidebar(depsPersistencia, CLAVE_ANCHO);
  if (ancho) {
    const n = Number(ancho);
    if (Number.isFinite(n)) cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
  }
  const col = leerSidebar(depsPersistencia, CLAVE_COLAPSADA);
  barraLateral.fijarAbierta(col !== '1');
  barraLateral.pintarSidebar();
}
pintarEstadoInicial();
// [089A-2] El toggle derecho arranca en "mostrar" (panel oculto, sin tabs).
pintarToggleDerecho();

// Recalcular alturas del textarea tras montar al DOM.
panelesRegistrados.forEach((p) => p.medir());
panelMeta.medir();
sincronizarPanelMeta();

// Reloj del turno (panelMeta global): refleja el turno del panel que lanzó.
window.setInterval(() => {
  if (!USA_REAL || !turnoGlobal) return;
  const p = panelActivo();
  const inicio = p?.getInicioTurno() ?? null;
  if (inicio === null) return;
  panelMeta.setTiempo((Date.now() - inicio) / 1000);
  const u = adaptador.usoUltimoTurno();
  panelMeta.setTokens(u.tokensPrompt + u.tokensComplecion);
}, 1000);

// ---------- Historial inicial (mock) ----------
if (USA_MOCK) {
  historialEjemplo().forEach((bloque) => {
    principal.mensajes.appendChild(renderizarBloque(bloque));
  });
  // La primera conversación activa queda como conversaId del principal
  // (para el ⋯ de cabecera y la selección de la sidebar).
  const candidata = conversaciones.find((c) => !c.archivada);
  if (candidata) {
    void principal.cargarConversacion(candidata.id);
    sidebar.seleccionar(candidata.id);
  }
  activarPanel(principal);
} else if (USA_REAL) {
  principal.avisoLocal(
    'Sesión real del núcleo (sin simulación)',
    MODO_TEXTO,
    'escribe y envía',
  );
}

// Añade las capas globales fuera de #app (hermanas del layout).
document.body.appendChild(toastGlobal.raiz);
document.body.appendChild(modal.raiz);

// [069A-Proyectos] Modal "Nuevo proyecto" autocontenido.
const modalProyecto = montarModalProyecto({
  invoke: USA_TAURI ? invoke : undefined,
  onGuardar(nombre, ruta) {
    if (!USA_REAL) return;
    void (async () => {
      try {
        if (turnoGlobal) {
          avisoGlobal('hay un turno en curso', '', 'espera a que termine para crear un proyecto');
          return;
        }
        await adaptador.sesion.workspaces.guardarProyecto(nombre, ruta);
        // onSesion / refrescarProyectos refrescarán sidebar + lista.
        principal.ponerBorrador();
        activarPanel(principal);
      } catch (e: unknown) {
        avisoGlobal(`no se pudo crear el proyecto: ${String(e)}`, '', '');
      }
    })();
  },
});
document.body.appendChild(modalProyecto.raiz);

// ---------- Arranque real: sesión + lista + última conversación ----------
if (USA_REAL) {
  void (async () => {
    try {
      await adaptador.asegurarSesion(opcionesArranque());
      const [provG, modG, modoG, razG, anchoG, colG, ctxG, temaG] = await Promise.all([
        adaptador.sesion.configLeer('proveedor'),
        adaptador.sesion.configLeer('modelo'),
        adaptador.sesion.configLeer('modo'),
        adaptador.sesion.configLeer('nivelRazonamiento'),
        adaptador.sesion.configLeer(CLAVE_ANCHO),
        adaptador.sesion.configLeer(CLAVE_COLAPSADA),
        adaptador.sesion.configLeer('contexto_max_ventana'),
        adaptador.sesion.configLeer(CLAVE_TEMA_OSCURO),
      ]);
      if (anchoG) {
        const n = Number(anchoG);
        if (Number.isFinite(n)) cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
      }
      if (colG) {
        barraLateral.fijarAbierta(colG !== '1');
        barraLateral.pintarSidebar();
      }
      if (modG) {
        modeloActual = {
          proveedor: provG ?? modeloActual.proveedor,
          modelo: modG,
          nombre: modG,
        };
        panelesRegistrados.forEach((p) => p.setModelo(modeloActual));
        modal.setModelo(modeloActual);
      }
      if (modoG === 'predeterminado' || modoG === 'meta' || modoG === 'autonomo') {
        modoActual = modoG;
        panelesRegistrados.forEach((p) => p.setModo(modoActual));
        modal.asignarValor('modo', modoActual);
        sincronizarPanelMeta();
      }
      if (razG && RAZONAMIENTO_ETIQUETA[razG]) {
        razonamientoActual = razG;
        panelesRegistrados.forEach((p) => p.setRazonamiento(razG));
        modal.asignarValor('nivelRazonamiento', razG);
      }
      // [039A-3 P6] Restaura la ventana de contexto persistida en el modal
      // (el backend la lee de config al construir la sesión; aquí solo se
      // refleja el valor guardado en el control del panel Contexto).
      if (ctxG && Number(ctxG) > 0) {
        modal.asignarValor('contexto_max_ventana', ctxG);
      }
      if (temaG !== null) {
        const activo = temaG === '1' || temaG === 'true';
        aplicarTemaOscuro(activo);
        modal.asignarValor(CLAVE_TEMA_OSCURO, activo);
      }
      sincronizarPanelMeta();
      await resincronizarSidebar();
      // [069A-7] Recargar con historial → carga la última conversación real
      // (decisión A). Con 0 conversaciones → el principal queda en BORRADOR
      // (create-on-write): NO se crea fila, NO se llama al backend.
      const candidatas = conversaciones.filter((c) => !c.archivada);
      const primera = candidatas[0];
      if (primera) {
        await principal.cargarConversacion(primera.id);
      } else {
        principal.ponerBorrador();
      }
      activarPanel(principal);
    } catch (e: unknown) {
      principal.avisoLocal(
        `el backend no arrancó: ${String(e)}`,
        MODO_TEXTO,
        BASE_API !== null && !USA_TAURI
          ? 'revisa ?api= y ?token= y recarga'
          : 'puedes escribir igual (reintenta al enviar)',
      );
    }
  })();
}
