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

import { montarSidebar } from './componentes/sidebar';
import type { ModoEjecucion } from './componentes/entrada';
import { montarModalConfiguracion } from './componentes/modal';
import { montarModalProyecto } from './componentes/modalProyecto';
import './estilos/modalProyecto.css';
import { montarPanelMeta } from './componentes/panelMeta';
import { montarPanelChat, type PanelChat } from './componentes/panelChat';
import { montarBarraSuperior } from './componentes/barraSuperior';
import { montarPanelNavegador } from './componentes/panelNavegador';
import { montarPanelDerecho } from './componentes/panelDerecho';
import { montarPanelVisor } from './componentes/panelVisor';
import { renderizarBloque } from './componentes/mensajes';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from './componentes/menu';

import { crearSimulacion } from './simulacion/simulacion';
import { invoke } from '@tauri-apps/api/core';
import { crearAdaptadorReal, esEntornoTauri } from './tauri/real';
import { crearAdaptadorApi } from './adaptadores/api';
import type { HooksAdaptador, InfoSesion, UsoTurno } from './tauri/real';
import { el } from './util/dom';
import { copiarAlPortapapeles } from './util/portapapeles';

const raizApp = document.getElementById('app');
if (!raizApp) throw new Error('falta #app');

// ---------- Layout raíz ----------
const app = el('div');
app.id = 'app';
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
  onToggleSidebar() {
    alternarSidebar();
  },
  onTogglePanelDerecho() {
    alternarPanelDerecho();
  },
  onAbrirVisor() {
    abrirVisor();
  },
});

// ---------- Estado compartido M1 (runtime único) ----------
let modeloActual: ModeloSeleccionado = MODELO_INICIAL;
let modoActual: ModoEjecucion = 'predeterminado';
let razonamientoActual = 'medium';
// Flag global: algún panel tiene turno en curso (M1: solo uno a la vez).
let turnoGlobal = false;
// Panel que envió el último mensaje (para el play/reanudar del panelMeta).
let panelUltimoEnvio: PanelChat | null = null;

const RAZONAMIENTO_ETIQUETA: Record<string, string> = {
  low: 'Bajo',
  medium: 'Medio',
  high: 'Alto',
};

const simulacion = crearSimulacion();
/** [069A-2 F4] Factoría simple: Tauri → IPC; `?api=`/`gh_api`/origen http
 * → adaptador HTTP/SSE; sin backend → mock o maqueta con aviso claro.
 * `?token=` aporta el maestro (solo memoria); la sesión viaja en cookie. */
function detectarBaseApi(): string | null {
  const q = new URLSearchParams(window.location.search).get('api');
  if (q) return q.replace(/\/$/, '');
  try {
    const g = window.localStorage.getItem('gh_api');
    if (g) return g.replace(/\/$/, '');
  } catch {
    /* sin localStorage */
  }
  // UI servida por `glory-harness web`: el backend existe trivialmente.
  if (window.location.protocol === 'http:' || window.location.protocol === 'https:') return '';
  return null;
}
const USA_TAURI = esEntornoTauri();
const BASE_API = USA_TAURI ? null : detectarBaseApi();
const USA_REAL = USA_TAURI || BASE_API !== null;
const USA_MOCK = !USA_REAL && import.meta.env.VITE_MOCK === '1';
const MODO_TEXTO = USA_TAURI ? 'tauri in-process' : BASE_API !== null ? `web (${BASE_API || 'mismo origen'})` : 'sin backend';
const CLAVE_TEMA_OSCURO = 'temaOscuro';

function aplicarTemaOscuro(activo: boolean): void {
  if (activo) document.documentElement.dataset.tema = 'oscuro';
  else delete document.documentElement.dataset.tema;
}

// Lista de conversaciones (fuente para la sidebar y el ⋯ de cabecera).
let conversaciones: Conversacion[] = USA_REAL
  ? []
  : CONVERSACIONES.map((c) => ({ ...c }));

// [069A-Proyectos] Proyectos registrados en la sesión + activo actual.
let proyectos: Workspace[] = [];
let proyectoActivo: Workspace | null = null;

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
  // [089A-2] Cambios de archivos del agente → tab Cambios del visor.
  // Seguro: corre en runtime, cuando `visor` ya existe.
  onCambioArchivo(cambio) {
    visor.registrarCambio(cambio);
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
    proyectoActivo = res.activa;
    sidebar.sustituirProyectos(proyectos, proyectoActivo);
    // Sincroniza el selector de workspace de todos los paneles.
    const seleccionadoId = proyectoActivo?.id ?? null;
    panelesRegistrados.forEach((p) => p.setWorkspaces(proyectos, seleccionadoId));
  } catch {
    // silencioso: el sidebar conserva el último estado válido
  }
}

/** Renombra en la lista local + títulos de paneles + backend. */
async function renombrarEnLista(id: string, titulo: string): Promise<void> {
  const conv = conversaciones.find((c) => c.id === id);
  if (conv) conv.titulo = titulo;
  panelesRegistrados.forEach((p) => {
    if (p.conversaId === id) p.ponerTitulo(titulo);
  });
  sidebar.sustituir(conversaciones);
  if (!USA_REAL) return;
  try {
    const ok = await adaptador.sesion.renombrar(id, titulo);
    if (!ok) {
      avisoGlobal('el backend no renombró (id ajeno o inexistente)', '', '');
      await resincronizarSidebar();
    }
  } catch (e: unknown) {
    avisoGlobal(`no se pudo renombrar: ${String(e)}`, '', '');
    await resincronizarSidebar();
  }
}

/** ¿Se puede ofrecer "Abrir en lateral"? (multi-chat: hasta 8 laterales en
 * tabs; el ancho ya no limita porque comparten el panel derecho). */
const MAX_LATERALES = 8;
function puedeAbrirLateral(): boolean {
  return panelesRegistrados.filter((p) => p.tipo === 'lateral').length < MAX_LATERALES;
}

/** Abre/activa un lateral en su propia tab `chat:<id>` (multi-chat). Si la
 * conversación ya está abierta, solo se activa su tab. */
let contadorLaterales = 0;
function abrirEnLateral(id: string): void {
  const tabId = `chat:${id}`;
  const yaAbierto = panelesRegistrados.find(
    (p) => p.tipo === 'lateral' && p.raiz.dataset.tabId === tabId,
  );
  if (yaAbierto) {
    asegurarPanelDerecho();
    panelDerecho.activarTab(tabId);
    activarPanel(yaAbierto);
    yaAbierto.enfocarEntrada();
    return;
  }
  if (!puedeAbrirLateral()) {
    avisoGlobal('demasiados laterales abiertos', '', 'cierra alguna tab del panel derecho');
    return;
  }
  contadorLaterales += 1;
  const titulo = conversaciones.find((c) => c.id === id)?.titulo ?? 'Chat';
  const lateral = crearPanel('lateral', `lateral-${contadorLaterales}`, {
    onCerrar() {
      cerrarLateral(tabId);
    },
  });
  lateral.raiz.dataset.tabId = tabId;
  // [089A-2] El lateral vive como tab del panel derecho (convive con el
  // navegador, el visor y otros chats). Sin grip propio: el panel derecho
  // trae su grip único.
  asegurarPanelDerecho();
  panelDerecho.abrirTab(tabId, titulo, lateral.raiz, () => cerrarLateral(tabId));
  // [039A-3 retoque] El textarea del lateral nace con height 0px porque el
  // `medir()` de montarEntrada corre en el constructor, antes de estar en el
  // DOM (scrollHeight 0). Al montar el panel ya se puede medir: recalcula la
  // altura del input (si no, el área de escritura del lateral queda invisible).
  lateral.medir();
  // Carga la conversación elegida en el lateral y lo enfoca.
  void (async () => {
    await lateral.cargarConversacion(id);
    activarPanel(lateral);
  })();
}

/** Cierra un lateral (`tabId`) o todos (el principal permanece). */
function cerrarLateral(tabId?: string): void {
  const victimas = panelesRegistrados.filter(
    (p) => p.tipo === 'lateral' && (!tabId || p.raiz.dataset.tabId === tabId),
  );
  if (victimas.length === 0) return;
  const habiaActivo = victimas.some((p) => p === panelActivo());
  victimas.forEach((p) => {
    const idx = panelesRegistrados.indexOf(p);
    if (idx >= 0) panelesRegistrados.splice(idx, 1);
    const tid = p.raiz.dataset.tabId;
    if (tid) panelDerecho.cerrarTab(tid);
  });
  // [089A-2] Desmonta sus tabs; si no quedan tabs se oculta el panel derecho
  // (con su grip). El nodo lo conserva el panel hasta reabrirlo.
  cerrarPanelDerechoSiVacio();
  if (habiaActivo) {
    const principal = panelesRegistrados.find((p) => p.tipo === 'principal');
    if (principal) {
      activarPanel(principal);
      principal.enfocarEntrada();
    }
  }
}

/** Menú ⋯ de la cabecera de un panel (sobre la conversación de ESE panel). */
function abrirAccionesPanel(panel: PanelChat, rect: DOMRect): void {
  const id = panel.conversaId;
  const conv = conversaciones.find((c) => c.id === id);
  if (!conv) {
    avisoGlobal('no hay conversación activa', '', 'abre una conversación desde la lista');
    return;
  }
  abrirMenuContextual({
    rect,
    construir(m) {
      m.appendChild(
        crearItemMenu({
          texto: 'Cambiar nombre',
          onClick() {
            cerrarMenuActual();
            // Renombrar inline en la cabecera de ESTE panel.
            panel.empezarRenombrarCabecera(conv.titulo, (nuevo) => {
              void renombrarEnLista(conv.id, nuevo);
            });
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: conv.archivada ? 'Desarchivar' : 'Archivar',
          onClick() {
            sidebar.archivarConversacion(conv.id);
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: 'Copiar ID',
          onClick() {
            cerrarMenuActual();
            void copiarAlPortapapeles(conv.id);
          },
        }),
      );
      // [039A-3 P5] "Abrir en panel lateral" desde el ⋯ del principal.
      if (puedeAbrirLateral() && panel.tipo === 'principal') {
        m.appendChild(crearSeparadorMenu());
        m.appendChild(
          crearItemMenu({
            texto: 'Abrir en panel lateral',
            onClick() {
              cerrarMenuActual();
              abrirEnLateral(conv.id);
            },
          }),
        );
      }
      m.appendChild(crearSeparadorMenu());
      m.appendChild(
        crearItemMenu({
          texto: 'Eliminar',
          onClick() {
            sidebar.eliminarConversacion(conv.id);
          },
        }),
      );
    },
  });
}

// ---------- Sidebar ----------
const sidebar = montarSidebar({
  conversaciones,
  proyectos,
  proyectoActivo,
  onSeleccionar(id) {
    // La sidebar carga la conversación en el panel ENFOCADO.
    const panel = panelActivo();
    if (!panel) return;
    if (USA_MOCK) {
      panel.ponerTitulo(conversaciones.find((c) => c.id === id)?.titulo ?? 'Sin conversación');
      void panel.cargarConversacion(id);
      activarPanel(panel);
      return;
    }
    void (async () => {
      await panel.cargarConversacion(id);
      activarPanel(panel);
    })();
  },
  onRenombrar(id, titulo) {
    void renombrarEnLista(id, titulo);
  },
  onArchivar(id, archivada) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) conv.archivada = archivada;
    if (!USA_REAL) return;
    void (async () => {
      try {
        const ok = await adaptador.sesion.archivar(id, archivada);
        if (!ok) {
          avisoGlobal('el backend no archivó (id ajeno o inexistente)', '', '');
          await resincronizarSidebar();
        }
      } catch (e: unknown) {
        avisoGlobal(`no se pudo archivar: ${String(e)}`, '', '');
        await resincronizarSidebar();
      }
    })();
  },
  onEliminar(id) {
    // Quitar de la lista local y repintar.
    conversaciones = conversaciones.filter((c) => c.id !== id);
    sidebar.sustituir(conversaciones);
    if (!USA_REAL) {
      // En mock: si algún panel mostraba el id, muéstrale otra o límpialo.
      const resto = conversaciones.find((c) => !c.archivada);
      panelesRegistrados.forEach((p) => {
        if (p.conversaId === id) {
          if (resto) void p.cargarConversacion(resto.id);
          else {
            p.limpiar();
            p.ponerTitulo('Sin conversación');
          }
        }
      });
      return;
    }
    void (async () => {
      try {
        const actual = await adaptador.sesion.eliminar(id, panelActivo()?.tipo);
        await resincronizarSidebar();
        // [069A-7] Si algún panel mostraba el id eliminado: si el backend
        // devolvió una conversación (quedaba otra), se carga; si `null`
        // (no quedó ninguna), el panel pasa a BORRADOR sin fila fantasma.
        panelesRegistrados.forEach((p) => {
          if (p.conversaId === id) {
            if (actual) void p.cargarConversacion(actual.id);
            else p.ponerBorrador();
          }
        });
      } catch (e: unknown) {
        avisoGlobal(`no se pudo eliminar: ${String(e)}`, '', '');
        await resincronizarSidebar();
      }
    })();
  },
  onAccionNav(accion) {
    if (accion === 'nueva') {
      // [069A-7] "Nueva conversación" = borrador local (sin fila): la fila
      // se crea al escribir el primer mensaje (create-on-write). En mock se
      // mantiene la semántica histórica (crea una conversación local).
      if (USA_REAL) {
        const panel = panelActivo();
        if (panel) {
          panel.ponerBorrador();
          activarPanel(panel);
        }
        return;
      }
      void (async () => {
        const n = conversaciones.length + 1;
        const nueva: Conversacion = {
          id: `local-${Date.now()}`,
          titulo: `Conversación ${n}`,
        };
        conversaciones = [nueva, ...conversaciones];
        sidebar.sustituir(conversaciones);
        const panel = panelActivo();
        if (panel) {
          panel.limpiar();
          panel.ponerTitulo(nueva.titulo);
          await panel.cargarConversacion(nueva.id);
          activarPanel(panel);
          sidebar.seleccionar(nueva.id);
        }
      })();
      return;
    }
    const nombre =
      accion === 'agente'
        ? 'Agentes'
        : accion === 'flujo'
          ? 'Flujo'
          : accion === 'complementos'
            ? 'Complementos'
            : accion;
    if (accion === 'navegador') {
      if (navegadorAbierto) {
        cerrarNavegador();
      } else {
        abrirNavegador();
      }
      return;
    }
    panelActivo()?.avisoLocal(
      `${nombre}: próximamente`,
      'sin backend',
      `${nombre} no está disponible en esta versión.`,
    );
  },
  puedeAbrirLateral,
  onAbrirEnLateral(id) {
    abrirEnLateral(id);
  },
  /** [069A-Proyectos] Abre el modal para crear un nuevo proyecto. */
  onCrearProyecto() {
    modalProyecto.abrir();
  },
  /** [069A-Proyectos] Cambia el proyecto activo por ruta. */
  onSeleccionarProyecto(ruta: string) {
    if (!USA_REAL) return;
    void (async () => {
      try {
        // No permitir si hay turno en curso.
        if (turnoGlobal) {
          avisoGlobal('hay un turno en curso', '', 'espera a que termine antes de cambiar de proyecto');
          return;
        }
        await adaptador.sesion.workspaces.activarPorRuta(ruta);
        // onSesion / refrescarProyectos refrescarán sidebar + lista.
        // El panel principal pasa a borrador (sin conversación del otro proyecto).
        principal.ponerBorrador();
        activarPanel(principal);
      } catch (e: unknown) {
        avisoGlobal(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
      }
    })();
  },
  abrirConfig: () => modal.abrir(),
});

/** Construcción de un panel. El principal recibe los PROVEEDORES (selector
 * de modelo) y el panelMeta global montado DENTRO de su entrada (único M1).
 * [039A-3 P6b] El lateral también recibe PROVEEDORES (misma entrada completa,
 * con modelo/modo/razonamiento compartidos M1); no tiene panelMeta propio. */
function crearPanel(
  tipo: 'principal' | 'lateral',
  idPrefijo: string,
  opts: { onCerrar?: () => void } = {},
): PanelChat {
  const panel = montarPanelChat({
    tipo,
    idPrefijo,
    proveedores: PROVEEDORES,
    deps: {
      adaptador,
      simulacion,
      usaReal: USA_REAL,
      usaMock: USA_MOCK,
      panelMeta,
      sidebar,
      conversaciones: () => conversaciones,
      getModelo: () => modeloActual,
      getModo: () => modoActual,
      getRazonamiento: () => razonamientoActual,
      getWorkspaces: () => proyectos,
      getWorkspaceSeleccionadoId: () => proyectoActivo?.id ?? null,
      prepararWorkspaceSeleccionado: async () => {
        // No-op: la activación se delega al callback.
      },
      onWorkspaceCambiado(id) {
        // Siempre hay un workspace activo (el agente trabaja en algún
        // área). Si id es null no se hace nada (fallback al activo actual).
        if (id === null) return;
        const ws = proyectos.find((p) => p.id === id);
        if (!ws) return;
        if (turnoGlobal) return;
        void (async () => {
          try {
            await adaptador.sesion.workspaces.activarPorRuta(ws.ruta);
            // onSesion refrescará sidebar + lista.
            principal.ponerBorrador();
            activarPanel(principal);
          } catch (e: unknown) {
            avisoGlobal(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
          }
        })();
      },
      hayTurnoGlobal,
      notificarTurnoInicio,
      notificarTurnoFin,
      registrarUltimoEnvio,
      resincronizarSidebar,
      onConversacionCambio(id) {
        // Al cambiar la conversación del panel ENFOCADO, la sidebar lo marca.
        // [069A-7] `null` (borrador) → deselecciona la lista.
        if (panelActivo() === panel) {
          if (id) sidebar.seleccionar(id);
          else sidebar.seleccionar('');
        }
        // El panel meta no forma parte del borrador inicial: aparece al
        // escribir el primer mensaje o al cargar una conversación real.
        sincronizarPanelMeta();
      },
    },
    onAcciones(rect) {
      // Menú ⋯ de la cabecera de ESTE panel.
      abrirAccionesPanel(panel, rect);
    },
    onCerrar: opts.onCerrar,
    onToggleSidebar() {
      // [089A-2] El botón de la cabecera alterna la lista (mostrar/ocultar).
      alternarSidebar();
    },
    // [089A-2] Toggle del panel derecho (solo el principal lo muestra).
    onTogglePanelDerecho: tipo === 'principal' ? () => alternarPanelDerecho() : undefined,
    // [089A-2] Botón del visor (solo el principal lo muestra).
    onAbrirVisor: tipo === 'principal' ? () => abrirVisor() : undefined,
    onModeloCambiado(nuevo) {
      modeloActual = nuevo;
      // [039A-3 P6b] Propaga a TODOS los paneles (ambos son completos y
      // comparten el runtime M1): el selector del panel que cambió ya está
      // actualizado; el resto se sincroniza aquí.
      panelesRegistrados.forEach((p) => p.setModelo(nuevo));
      modal.setModelo(nuevo);
      if (USA_REAL) {
        void adaptador.sesion
          .configGuardar('proveedor', nuevo.proveedor)
          .then(() => adaptador.sesion.configGuardar('modelo', nuevo.modelo))
          .catch((e: unknown) => avisoGlobal(`no se pudo guardar el modelo: ${String(e)}`, '', ''));
      }
    },
    onModoCambiado(nuevo) {
      modoActual = nuevo;
      // [039A-3 P6b] Sincroniza el modo en ambos paneles (M1 compartido).
      panelesRegistrados.forEach((p) => p.setModo(nuevo));
      modal.asignarValor('modo', nuevo);
      if (USA_REAL) {
        void adaptador.sesion
          .configGuardar('modo', nuevo)
          .catch((e: unknown) => avisoGlobal(`no se pudo guardar el modo: ${String(e)}`, '', ''));
      }
      sincronizarPanelMeta();
    },
    onRazonamientoCambiado(nuevo) {
      razonamientoActual = nuevo;
      // [039A-3 P6b] Sincroniza el razonamiento en ambos paneles (M1).
      panelesRegistrados.forEach((p) => p.setRazonamiento(nuevo));
      modal.asignarValor('nivelRazonamiento', nuevo);
      if (USA_REAL) {
        void adaptador.sesion
          .configGuardar('nivelRazonamiento', nuevo)
          .catch((e: unknown) =>
            avisoGlobal(`no se pudo guardar el razonamiento: ${String(e)}`, '', ''),
          );
      }
    },
    onWorkspaceCambiado(id) {
      // Siempre hay un workspace activo. Si id es null, no se hace nada.
      if (id === null) return;
      const ws = proyectos.find((p) => p.id === id);
      if (!ws) return;
      if (turnoGlobal) return;
      void (async () => {
        try {
          await adaptador.sesion.workspaces.activarPorRuta(ws.ruta);
          principal.ponerBorrador();
          activarPanel(principal);
        } catch (e: unknown) {
          avisoGlobal(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
        }
      })();
    },
  });

  if (tipo === 'principal') {
    // El panel meta comparte la entrada principal, pero queda después del
    // selector de workspace. Además permanece oculto hasta que el primer
    // mensaje cree una conversación real; así el borrador inicial solo ocupa
    // el selector y la caja, sin solapamientos.
    const entradaRaiz = panel.raiz.querySelector<HTMLElement>('.entrada');
    const selectorWorkspace = entradaRaiz?.querySelector('.selector-workspace-box');
    if (entradaRaiz && selectorWorkspace) {
      entradaRaiz.insertBefore(panelMeta.raiz, selectorWorkspace.nextSibling);
    }
  }

  panelesRegistrados.push(panel);
  return panel;
}

// ---------- Grip de redimensionado de la sidebar ----------
const grip = el('div', 'sidebar-grip');
grip.setAttribute('aria-hidden', 'true');
let sidebarAbierta = true;
const CLAVE_ANCHO = 'sidebar_ancho';
const CLAVE_COLAPSADA = 'sidebar_colapsada';
{
  // [039A-3 P4] La sidebar se redimensiona entre MIN y MAX arrastrando el grip.
  // [039A-3 P6b retoque] Si se arrastra hasta dejar el ancho por debajo de
  // UMBRAL_COLAPSO, al soltar la lista se COLAPSA del todo (desaparece) en vez
  // de quedarse clavada en MIN. No hay botón manual de ocultar: este gesto de
  // arrastre hasta el borde es la forma de cerrarla, y el botón "mostrar
  // lista" de la cabecera la vuelve a abrir (a ANCHO_REABRIR).
  const MIN = 180;
  const MAX = 420;
  const UMBRAL_COLAPSO = 100;
  const ANCHO_REABRIR = 260;
  let arrastrando = false;
  function anchoArrastre(clientX: number): number {
    const izquierda = cuerpo.getBoundingClientRect().left;
    // En vivo se permite encoger hasta el umbral; el colapso total se decide
    // al soltar (si colapsase en vivo perderíamos el grip a mitad de gesto).
    return Math.min(MAX, Math.max(UMBRAL_COLAPSO, Math.round(clientX - izquierda)));
  }
  grip.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrando = true;
    document.body.classList.add('redimensionando-sidebar');
  });
  window.addEventListener('mousemove', (e) => {
    if (!arrastrando) return;
    cuerpo.style.setProperty('--sidebar-ancho', `${anchoArrastre(e.clientX)}px`);
  });
  window.addEventListener('mouseup', () => {
    if (!arrastrando) return;
    arrastrando = false;
    document.body.classList.remove('redimensionando-sidebar');
    const ancho = Math.round(
      parseFloat(getComputedStyle(cuerpo).getPropertyValue('--sidebar-ancho')) || 260,
    );
    if (ancho <= UMBRAL_COLAPSO) {
      // Arrastrado hasta el borde → colapsar la lista: oculta sidebar y grip,
      // deja el chat a ancho completo y muestra el botón "mostrar lista".
      sidebarAbierta = false;
      aplicarSidebar();
      cuerpo.style.setProperty('--sidebar-ancho', `${ANCHO_REABRIR}px`);
      guardarSidebar(CLAVE_ANCHO, String(ANCHO_REABRIR));
      guardarSidebar(CLAVE_COLAPSADA, '1');
    } else {
      cuerpo.style.setProperty('--sidebar-ancho', `${Math.max(MIN, ancho)}px`);
      guardarSidebar(CLAVE_ANCHO, String(Math.max(MIN, ancho)));
      guardarSidebar(CLAVE_COLAPSADA, '0');
    }
  });
}

function guardarSidebar(clave: string, valor: string): void {
  if (USA_REAL) {
    void adaptador.sesion
      .configGuardar(clave, valor)
      .catch((e: unknown) => avisoGlobal(`no se pudo guardar ${clave}: ${String(e)}`, '', ''));
  } else {
    try {
      window.localStorage.setItem(clave, valor);
    } catch {
      /* sin persistencia local */
    }
  }
}
function leerSidebar(clave: string): string | null {
  // El modo web usa localStorage para estas claves; Tauri las restaura por IPC
  // cuando termina de abrir la sesión persistida en SQLite.
  if (USA_TAURI) return null;
  try {
    return window.localStorage.getItem(clave);
  } catch {
    return null;
  }
}

// [039A-3 P6b] La lista de conversaciones NO se oculta con un botón manual:
// se oculta sola si la ventana se reduce por debajo de un ancho mínimo
// (auto), y el botón de la cabecera solo sirve para MOSTRARLA (forzar) si
// quedó oculta por ese auto-ocultado. `sidebarForzada` recuerda que el
// usuario pidió verla aunque la ventana sea angosta; al ensanchar se resetea.
const UMBRAL_AUTO_SIDEBAR = 720;
let sidebarForzada = false;

/** ¿La ventana es tan angosta que la lista no debe ocupar espacio? */
function ventanaAngosta(): boolean {
  // [039A-3 P6b] Auto-ocultado de la lista: por debajo de un ancho mínimo de
  // ventana la sidebar ya no cabe junto al chat y se oculta sola.
  return window.innerWidth < UMBRAL_AUTO_SIDEBAR;
}

/** Aplica el estado efectivo (preferencia + auto-ocultado por ancho). */
function aplicarSidebar(): void {
  const angosta = ventanaAngosta();
  if (!angosta) sidebarForzada = false;
  const visible = sidebarAbierta && (!angosta || sidebarForzada);
  cuerpo.classList.toggle('sidebar-colapsada', !visible);
  panelesRegistrados.forEach((p) => p.setSidebarAbierta(visible));
  // [089A-3] Espejo en la barra superior global (la cabecera ya no tiene
  // el toggle; su setter es no-op).
  barra.setSidebarAbierta(visible);
}
function pintarSidebar(): void {
  aplicarSidebar();
}
/** El botón de la cabecera SOLO muestra la lista (nunca la oculta). */
function mostrarSidebar(): void {
  sidebarAbierta = true;
  if (ventanaAngosta()) sidebarForzada = true;
  aplicarSidebar();
  guardarSidebar(CLAVE_COLAPSADA, '0');
}
/** [089A-2] Oculta la lista (manual; el auto-ocultado por ancho sigue). */
function ocultarSidebar(): void {
  sidebarAbierta = false;
  sidebarForzada = false;
  aplicarSidebar();
  guardarSidebar(CLAVE_COLAPSADA, '1');
}
/** [089A-2] Alterna la lista (botón siempre visible de la cabecera). */
function alternarSidebar(): void {
  const visible = !cuerpo.classList.contains('sidebar-colapsada');
  if (visible) ocultarSidebar();
  else mostrarSidebar();
}

// Escucha resize para el auto-ocultado de la lista por ancho mínimo.
window.addEventListener('resize', () => aplicarSidebar());

// ---------- Grip de redimensionado del panel derecho (tabs) ----------
// [089A-2] Divisor vertical arrastrable entre #paneles y el panel derecho.
// Vive en #cuerpo (hijo flex de 9px, no absolute) y fija
// `--panel-derecho-ancho` en #cuerpo dentro de [260, 70%]; se persiste en
// la misma clave de siempre (sigue siendo "el ancho del panel derecho").
const CLAVE_LATERAL_ANCHO = 'lateral_ancho';
let gripPanelDerecho: HTMLElement | null = null;
let anchoPanelDerechoFijado: number | null = null;
function medirAnchoCuerpo(): number {
  return cuerpo.getBoundingClientRect().width;
}
function aplicarPanelDerechoAncho(px: number): void {
  anchoPanelDerechoFijado = px;
  cuerpo.style.setProperty('--panel-derecho-ancho', `${px}px`);
}
function crearGripPanelDerecho(): HTMLElement {
  const g = el('div', 'panel-derecho-grip');
  g.setAttribute('aria-hidden', 'true');
  let arrastrando = false;
  g.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrando = true;
    document.body.classList.add('redimensionando-lateral');
  });
  window.addEventListener('mousemove', (e) => {
    if (!arrastrando) return;
    const rect = cuerpo.getBoundingClientRect();
    // El panel derecho está ANCLADO al borde derecho de #cuerpo: su ancho
    // es la distancia del cursor hasta el borde derecho (igual que antes
    // con el lateral en #paneles).
    const ancho = rect.right - e.clientX;
    const MIN = 260;
    const MAX = Math.round(rect.width * 0.7);
    const clampeado = Math.min(MAX, Math.max(MIN, Math.round(ancho)));
    aplicarPanelDerechoAncho(clampeado);
  });
  window.addEventListener('mouseup', () => {
    if (!arrastrando) return;
    arrastrando = false;
    document.body.classList.remove('redimensionando-lateral');
    if (anchoPanelDerechoFijado !== null) {
      guardarSidebar(CLAVE_LATERAL_ANCHO, String(anchoPanelDerechoFijado));
    }
  });
  return g;
}

// Al abrir el panel derecho se restaura el ancho persistido (si cabe en el
// 70% disponible).
function restaurarPanelDerechoAncho(): void {
  const base = leerSidebar(CLAVE_LATERAL_ANCHO);
  const anchoCuerpo = medirAnchoCuerpo();
  const MAX = Math.round(anchoCuerpo * 0.7);
  let px = Math.round(anchoCuerpo * 0.5); // parte de la mitad
  if (base) {
    const n = Number(base);
    if (Number.isFinite(n)) px = Math.round(Math.min(MAX, Math.max(260, n)));
  }
  aplicarPanelDerechoAncho(Math.min(MAX, Math.max(260, px)));
}

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

// ---------- Panel principal ----------
const principal = crearPanel('principal', 'principal');

// ---------- Navegador interno (069A-1 F3 / 069A-2 fix web) ----------
// [069A-2 fix] En Tauri el panel pilota la webview child (IPC); en modo web,
// un iframe del propio navegador. `onCerrar` sincroniza el estado del
// orquestador (navegadorAbierto, grip, RO) cuando se cierra desde el botón
// interno del panel, para no duplicar la lógica de cierre en dos sitios.
const navegador = montarPanelNavegador({
  onCerrar() {
    cerrarNavegador();
  },
  // [seleccionar] El usuario eligió un elemento de la página con el modo
  // "seleccionar": se adjunta como badge al chat activo para que el modelo
  // reciba el descriptor (URL + selector CSS) en el próximo mensaje.
  onSeleccionar(elem) {
    const panel = panelActivo();
    if (!panel) {
      avisoGlobal('abre un chat para recibir el elemento', '', elem.etiqueta);
      return;
    }
    activarPanel(panel);
    panel.adjuntarElemento(elem);
  },
});
let navegadorAbierto = false;

// ---------- Panel derecho con tabs + visor (089A-2) ----------
// Chat lateral, Navegador y Visor conviven como tabs (antes el navegador
// y el lateral se excluían). El grip del navegador desaparece: el panel
// derecho trae su grip único (ancho persistido en la clave de siempre).
const visor = montarPanelVisor();
const panelDerecho = montarPanelDerecho({
  onCerrarTodo() {
    cerrarLateral();
    cerrarNavegador();
    panelDerecho.cerrarTab('visor');
  },
  onCambioTab(id) {
    // La webview hija es nativa: no respeta `hidden`. Al salir de su tab
    // se oculta (sin destruirla) y al volver se muestra + reposiciona.
    if (!USA_TAURI || !navegadorAbierto) return;
    if (id === 'navegador') {
      void invoke('navegador_mostrar', { visible: true })
        .then(() => reposicionarWebview())
        .catch(() => {});
    } else {
      void invoke('navegador_mostrar', { visible: false }).catch(() => {});
    }
  },
});

/** Visibilidad del panel derecho (independiente de sus tabs: ocultar no
 * destruye; las tabs y sus nodos vivos se conservan). Arranca oculto. */
let panelDerechoVisible = false;

/** Refleja la visibilidad en el toggle de la barra superior global. */
function pintarToggleDerecho(): void {
  panelesRegistrados
    .find((p) => p.tipo === 'principal')
    ?.setPanelDerechoAbierto(panelDerechoVisible);
  // [089A-3] La cabecera ya no tiene el toggle (su setter es no-op); el
  // espejo real vive en la barra superior.
  barra.setPanelDerechoAbierto(panelDerechoVisible);
}

/** Monta el panel derecho en #cuerpo (con su grip) si aún no está. */
function asegurarPanelDerecho(): void {
  if (!gripPanelDerecho) {
    gripPanelDerecho = crearGripPanelDerecho();
    restaurarPanelDerechoAncho();
  }
  if (!gripPanelDerecho.parentNode) cuerpo.appendChild(gripPanelDerecho);
  if (!panelDerecho.raiz.parentNode) cuerpo.appendChild(panelDerecho.raiz);
  panelDerechoVisible = true;
  pintarToggleDerecho();
}

/** Oculta el panel derecho sin destruir sus tabs (el toggle lo reabre). */
function ocultarPanelDerecho(): void {
  gripPanelDerecho?.remove();
  gripPanelDerecho = null;
  panelDerecho.raiz.remove();
  panelDerechoVisible = false;
  pintarToggleDerecho();
}

/** Alterna el panel derecho (botón siempre visible de la cabecera). Sin
 * tabs, abre el visor para que el botón siempre haga algo visible. */
function alternarPanelDerecho(): void {
  if (panelDerechoVisible) ocultarPanelDerecho();
  else if (panelDerecho.hayTabs()) asegurarPanelDerecho();
  else abrirVisor();
}

/** Quita el panel derecho (y su grip) si ya no tiene tabs. */
function cerrarPanelDerechoSiVacio(): void {
  if (panelDerecho.hayTabs()) return;
  ocultarPanelDerecho();
}

/** Reposiciona la webview hija sobre su contenedor (tras mostrar su tab). */
function reposicionarWebview(): void {
  const contenedor = document.getElementById('navegador-webview-contenedor');
  if (!contenedor) return;
  const r = contenedor.getBoundingClientRect();
  if (r.width < 2 || r.height < 2) return;
  void invoke('navegador_posicionar', {
    x: Math.round(r.left),
    y: Math.round(r.top),
    ancho: Math.round(r.width),
    alto: Math.round(r.height),
  }).catch(() => {});
}

/** Abre el tab Visor (vuelca los cambios del panel activo + fija el área). */
function abrirVisor(): void {
  visor.limpiarCambios();
  panelActivo()
    ?.listarCambios()
    .forEach((c) => visor.registrarCambio(c));
  visor.fijarBase(proyectoActivo?.ruta ?? null);
  asegurarPanelDerecho();
  panelDerecho.abrirTab('visor', 'Visor', visor.raiz, () => {
    panelDerecho.cerrarTab('visor');
    cerrarPanelDerechoSiVacio();
  });
}

function abrirNavegador(): void {
  if (navegadorAbierto) return;
  // [089A-2] El navegador vive como tab del panel derecho y convive con el
  // chat lateral y el visor (antes se excluían entre sí).
  navegadorAbierto = true;
  asegurarPanelDerecho();
  panelDerecho.abrirTab('navegador', 'Navegador', navegador.raiz, () => cerrarNavegador());
  navegador.mostrar(true);

  if (!USA_TAURI) {
    // [069A-2 fix] Modo web: el iframe se autogestiona; solo se fija la URL
    // inicial para que el área no quede en blanco.
    navegador.irA('https://example.com');
    navegador.registrarAccion({
      herramienta: 'abrir',
      descripcion: 'navegador iniciado en https://example.com',
      ok: true,
      tiempo: Date.now(),
    });
    return;
  }

  // Modo Tauri: calcular la posición del contenedor y abrir la webview child.
  void (async () => {
    try {
      const contenedor = document.getElementById('navegador-webview-contenedor');
      if (!contenedor) throw new Error('contenedor webview no encontrado');
      const rect = contenedor.getBoundingClientRect();

      await invoke('navegador_abrir', {
        url: 'https://example.com',
        ancho: Math.round(rect.width),
        alto: Math.round(rect.height),
        posX: Math.round(rect.left),
        posY: Math.round(rect.top),
      });
      navegador.fijarURL('https://example.com');
      navegador.registrarAccion({
        herramienta: 'abrir',
        descripcion: 'navegador iniciado en https://example.com',
        ok: true,
        tiempo: Date.now(),
      });

      // Observar cambios de tamaño/posición para reposicionar la webview
      const ro = new ResizeObserver(() => {
        const r = contenedor.getBoundingClientRect();
        void invoke('navegador_posicionar', {
          x: Math.round(r.left),
          y: Math.round(r.top),
          ancho: Math.round(r.width),
          alto: Math.round(r.height),
        }).catch(() => {});
      });
      ro.observe(contenedor);
      // Guardar observer para cleanup al cerrar
      (navegador as unknown as Record<string, unknown>).__resizeObserver = ro;
    } catch (error) {
      navegador.registrarAccion({
        herramienta: 'abrir',
        descripcion: `error: ${String(error).slice(0, 120)}`,
        ok: false,
        tiempo: Date.now(),
      });
    }
  })();
}

function cerrarNavegador(): void {
  if (!navegadorAbierto) return;
  navegadorAbierto = false;
  navegador.mostrar(false);
  // [089A-2] Desmonta su tab; si no quedan tabs se cierra el panel derecho.
  panelDerecho.cerrarTab('navegador');
  cerrarPanelDerechoSiVacio();
  // Solo en Tauri existe la webview child que cerrar; en web el iframe se
  // vacía dentro del propio panel (btnCerrar) y aquí solo se oculta.
  if (USA_TAURI) {
    void invoke('navegador_cerrar').catch(() => {});
  }
  // Limpiar ResizeObserver (solo se crea en Tauri)
  const ro = (navegador as unknown as Record<string, unknown>).__resizeObserver as ResizeObserver | undefined;
  if (ro) {
    ro.disconnect();
    delete (navegador as unknown as Record<string, unknown>).__resizeObserver;
  }
}

// ---------- Montaje del DOM ----------
cuerpo.appendChild(sidebar.raiz);
cuerpo.appendChild(grip);
paneles.appendChild(principal.raiz);
cuerpo.appendChild(paneles);
// [089A-2] El navegador y el visor viven detached hasta abrir su tab del
// panel derecho (que se monta bajo demanda con `asegurarPanelDerecho`).
// [089A-3] La barra superior global va primera (a todo el ancho, por
// encima de sidebar/paneles/panel derecho).
app.appendChild(barra.raiz);
app.appendChild(cuerpo);
raizApp.appendChild(app);

// Estado inicial de la sidebar (ancho/colapso persistidos + selección).
function pintarEstadoInicial(): void {
  const ancho = leerSidebar(CLAVE_ANCHO);
  if (ancho) {
    const n = Number(ancho);
    if (Number.isFinite(n)) cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
  }
  const col = leerSidebar(CLAVE_COLAPSADA);
  sidebarAbierta = col !== '1';
  pintarSidebar();
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

// Añade el modal fuera de #app (hermano del layout).
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
        sidebarAbierta = colG !== '1';
        pintarSidebar();
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

/** Opciones de turno para `asegurarSesion` en el arranque real.
 * Campos vacíos significan "resolver desde la configuración persistida";
 * enviar los defaults aquí sobrescribiría la configuración guardada. */
function opcionesArranque() {
  return {
    proveedor: '',
    modelo: '',
    modo: '',
    razonamiento: '',
  };
}
