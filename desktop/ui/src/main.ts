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
import type { Conversacion, ModeloSeleccionado } from './dominio/tipos';

import { montarSidebar } from './componentes/sidebar';
import type { ModoEjecucion } from './componentes/entrada';
import { montarModalConfiguracion } from './componentes/modal';
import { montarPanelMeta } from './componentes/panelMeta';
import { montarPanelChat, type PanelChat } from './componentes/panelChat';
import { renderizarBloque } from './componentes/mensajes';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from './componentes/menu';

import { crearSimulacion } from './simulacion/simulacion';
import { crearAdaptadorReal, esEntornoTauri } from './tauri/real';
import type { InfoSesion, UsoTurno } from './tauri/real';
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
const USA_REAL = esEntornoTauri();
const USA_MOCK = !USA_REAL && import.meta.env.VITE_MOCK === '1';

// Lista de conversaciones (fuente para la sidebar y el ⋯ de cabecera).
let conversaciones: Conversacion[] = USA_REAL
  ? []
  : CONVERSACIONES.map((c) => ({ ...c }));

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
const adaptador = crearAdaptadorReal({
  onSesion(info: InfoSesion) {
    sincronizarModeloDesdeSesion(info);
    const ws = info.workspace;
    if (ws && ws !== '<desconocido>') modal.asignarValor('workspace', ws);
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
});

// ---------- Gestión de paneles (1 principal + 0..1 lateral) ----------
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
  const lista = await adaptador.sesion.listar();
  conversaciones = lista.map((c) => ({
    id: c.id,
    titulo: c.titulo,
    archivada: c.archivada,
  }));
  sidebar.sustituir(conversaciones);
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

/** ¿Se puede ofrecer "Abrir en panel lateral"? (<2 chats y ancho mínimo). */
function puedeAbrirLateral(): boolean {
  return panelesRegistrados.length < 2 && window.innerWidth >= 900;
}

/** Abre/activa un segundo panel lateral mostrando la conversación dada. */
function abrirEnLateral(id: string): void {
  if (!puedeAbrirLateral()) {
    if (panelesRegistrados.length >= 2) {
      avisoGlobal('ya hay dos paneles abiertos', '', 'cierra el lateral para abrir otro');
    } else {
      avisoGlobal('pantalla demasiado estrecha', '', 'amplía la ventana para usar dos paneles');
    }
    return;
  }
  const lateral = crearPanel('lateral', 'lateral', {
    onCerrar() {
      cerrarLateral();
    },
  });
  // [039A-3 P6b] Divisor redimensionable entre principal y lateral: se
  // inserta ANTES del lateral y se restaura el ancho persistido.
  gripLateral = crearGripLateral();
  restaurarLateralAncho();
  paneles.appendChild(gripLateral);
  paneles.appendChild(lateral.raiz);
  // Carga la conversación elegida en el lateral y lo enfoca.
  void (async () => {
    await lateral.cargarConversacion(id);
    activarPanel(lateral);
  })();
}

/** Cierra el panel lateral (el principal permanece). */
function cerrarLateral(): void {
  const idx = panelesRegistrados.findIndex((p) => p.tipo === 'lateral');
  if (idx < 0) return;
  const lateral = panelesRegistrados[idx];
  lateral.raiz.remove();
  panelesRegistrados.splice(idx, 1);
  gripLateral?.remove();
  gripLateral = null;
  const principal = panelesRegistrados.find((p) => p.tipo === 'principal');
  if (principal) {
    activarPanel(principal);
    principal.enfocarEntrada();
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
        // Si algún panel mostraba el id eliminado, lo sustituye por `actual`.
        panelesRegistrados.forEach((p) => {
          if (p.conversaId === id) void p.cargarConversacion(actual.id);
        });
      } catch (e: unknown) {
        avisoGlobal(`no se pudo eliminar: ${String(e)}`, '', '');
        await resincronizarSidebar();
      }
    })();
  },
  onAccionNav(accion) {
    if (accion === 'nueva') {
      void (async () => {
        if (USA_REAL) {
          await panelActivo()?.nuevaConversacion();
          return;
        }
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
      hayTurnoGlobal,
      notificarTurnoInicio,
      notificarTurnoFin,
      registrarUltimoEnvio,
      resincronizarSidebar,
      onConversacionCambio(id) {
        // Al cambiar la conversación del panel ENFOCADO, la sidebar lo marca.
        if (panelActivo() === panel && id) sidebar.seleccionar(id);
      },
    },
    onAcciones(rect) {
      // Menú ⋯ de la cabecera de ESTE panel.
      abrirAccionesPanel(panel, rect);
    },
    onCerrar: opts.onCerrar,
    onToggleSidebar() {
      // [039A-3 P6b] El botón de la lista SOLO la muestra; nunca la oculta.
      mostrarSidebar();
    },
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
  });

  if (tipo === 'principal') {
    // PanelMeta global DENTRO de la entrada del principal, antes de .caja.
    const entradaRaiz = panel.raiz.querySelector<HTMLElement>('.entrada');
    if (entradaRaiz) entradaRaiz.insertBefore(panelMeta.raiz, entradaRaiz.firstChild);
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
  const MIN = 180;
  const MAX = 420;
  let arrastrando = false;
  function anchoDesdeCursor(clientX: number): number {
    const izquierda = cuerpo.getBoundingClientRect().left;
    return Math.min(MAX, Math.max(MIN, Math.round(clientX - izquierda)));
  }
  grip.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrando = true;
    document.body.classList.add('redimensionando-sidebar');
  });
  window.addEventListener('mousemove', (e) => {
    if (!arrastrando) return;
    const ancho = anchoDesdeCursor(e.clientX);
    cuerpo.style.setProperty('--sidebar-ancho', `${ancho}px`);
  });
  window.addEventListener('mouseup', () => {
    if (!arrastrando) return;
    arrastrando = false;
    document.body.classList.remove('redimensionando-sidebar');
    const ancho = Math.round(
      parseFloat(getComputedStyle(cuerpo).getPropertyValue('--sidebar-ancho')) || 260,
    );
    guardarSidebar(CLAVE_ANCHO, String(ancho));
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
  if (USA_REAL) return null;
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

// Escucha resize para el auto-ocultado de la lista por ancho mínimo.
window.addEventListener('resize', () => aplicarSidebar());

// ---------- Grip de redimensionado del panel lateral (2 chats) ----------
// [039A-3 P6b] Divisor vertical arrastrable entre el principal y el lateral.
// Se inserta en #paneles antes del lateral al abrirlo y se quita al cerrarlo.
// Ancho parte de la mitad (--lateral-ancho: 50%) y el arrastre lo fija en px
// dentro de [260, 70% del ancho de #paneles]; se persiste para restaurarlo.
const CLAVE_LATERAL_ANCHO = 'lateral_ancho';
let gripLateral: HTMLElement | null = null;
let lateralAnchoFijado: number | null = null;
function medirPanelesAncho(): number {
  return paneles.getBoundingClientRect().width;
}
function aplicarLateralAncho(px: number): void {
  lateralAnchoFijado = px;
  paneles.style.setProperty('--lateral-ancho', `${px}px`);
}
function crearGripLateral(): HTMLElement {
  const g = el('div', 'lateral-grip');
  g.setAttribute('aria-hidden', 'true');
  let arrastrando = false;
  g.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrando = true;
    document.body.classList.add('redimensionando-lateral');
  });
  window.addEventListener('mousemove', (e) => {
    if (!arrastrando) return;
    const anchoPaneles = medirPanelesAncho();
    const izquierda = paneles.getBoundingClientRect().left;
    const cursorEnLateral = e.clientX - izquierda;
    // El lateral va DESPUÉS del grip: ancho del lateral ≈ cursor - grip.
    const ancho = cursorEnLateral;
    const MIN = 260;
    const MAX = Math.round(anchoPaneles * 0.7);
    const clampeado = Math.min(MAX, Math.max(MIN, Math.round(ancho)));
    aplicarLateralAncho(clampeado);
  });
  window.addEventListener('mouseup', () => {
    if (!arrastrando) return;
    arrastrando = false;
    document.body.classList.remove('redimensionando-lateral');
    if (lateralAnchoFijado !== null) {
      guardarSidebar(CLAVE_LATERAL_ANCHO, String(lateralAnchoFijado));
    }
  });
  return g;
}

// Al reabrir el lateral se restaura el ancho persistido (si no supera el 70%
// disponible, p. ej. si la ventana se redujo respecto del último arrastre).
function restaurarLateralAncho(): void {
  const base = leerSidebar(CLAVE_LATERAL_ANCHO);
  const anchoPaneles = medirPanelesAncho();
  const MAX = Math.round(anchoPaneles * 0.7);
  let px = Math.round(anchoPaneles * 0.5); // parte de la mitad
  if (base) {
    const n = Number(base);
    if (Number.isFinite(n)) px = Math.round(Math.min(MAX, Math.max(260, n)));
  }
  aplicarLateralAncho(Math.min(MAX, Math.max(260, px)));
}

function sincronizarPanelMeta(): void {
  const hayMeta = panelMeta.getMeta().trim().length > 0;
  const visible = hayMeta || modoActual === 'meta';
  panelMeta.mostrar(visible);
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
    }
    if (
      USA_REAL &&
      (id === 'modo' || id === 'nivelRazonamiento' || id === 'contexto_max_ventana')
    ) {
      void adaptador.sesion
        .configGuardar(id, String(valor))
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

// ---------- Montaje del DOM ----------
cuerpo.appendChild(sidebar.raiz);
cuerpo.appendChild(grip);
paneles.appendChild(principal.raiz);
cuerpo.appendChild(paneles);
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
    'tauri in-process',
    'escribe y envía',
  );
}

// Añade el modal fuera de #app (hermano del layout).
document.body.appendChild(modal.raiz);

// ---------- Arranque real: sesión + lista + última conversación ----------
if (USA_REAL) {
  void (async () => {
    try {
      await adaptador.asegurarSesion(opcionesArranque());
      const [provG, modG, modoG, razG, anchoG, colG, ctxG] = await Promise.all([
        adaptador.sesion.configLeer('proveedor'),
        adaptador.sesion.configLeer('modelo'),
        adaptador.sesion.configLeer('modo'),
        adaptador.sesion.configLeer('nivelRazonamiento'),
        adaptador.sesion.configLeer(CLAVE_ANCHO),
        adaptador.sesion.configLeer(CLAVE_COLAPSADA),
        adaptador.sesion.configLeer('contexto_max_ventana'),
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
      sincronizarPanelMeta();
      await resincronizarSidebar();
      const candidatas = conversaciones.filter((c) => !c.archivada);
      const primera = candidatas[0];
      if (primera) {
        await principal.cargarConversacion(primera.id);
      }
      activarPanel(principal);
    } catch (e: unknown) {
      principal.avisoLocal(
        `el backend no arrancó: ${String(e)}`,
        'tauri',
        'puedes escribir igual (reintenta al enviar)',
      );
    }
  })();
}

/** Opciones de turno para `asegurarSesion` en el arranque real. */
function opcionesArranque() {
  let proveedor = modeloActual.proveedor;
  if (proveedor === 'commandcode' && /^(meta|stealth)\//.test(modeloActual.modelo)) {
    proveedor = 'glory';
  }
  return {
    proveedor,
    modelo: modeloActual.modelo,
    modo: modoActual,
    razonamiento: razonamientoActual,
  };
}
