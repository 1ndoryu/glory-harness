/* Panel derecho con tabs + Files/Git + grip de ancho (extraído de main.ts [089A-16 F1b]).
 * Monta toastGlobal, files, git y el panel derecho; gestiona visibilidad,
 * grip de redimensionado y apertura de tabs. Todo el estado compartido llega
 * por `deps`; sin importes del orquestador. `abrirNavegador` y
 * `abrirChatLateral` cierran sobre piezas declaradas después en el arranque
 * y solo se invocan desde eventos de UI, fuera de la TDZ. */

import { invoke } from '@tauri-apps/api/core';
import { porId } from '../util/dom';
import { montarPanelDerecho, type PanelDerecho } from '../componentes/panelDerecho';
import { montarPanelFiles, type PanelFiles } from '../componentes/panelFiles';
import { montarPanelGit, type PanelGit } from '../componentes/panelGit';
import { montarToastGlobal, type ToastGlobal } from '../componentes/toastGlobal';
import type { PanelChat } from '../componentes/panelChat';
import type { BarraSuperior } from '../componentes/barraSuperior';
import type { AdaptadorReal } from '../tauri/real';
import { crearAnchoPanelDerecho } from './panelDerechoAncho';
import type { PersistenciaDeps } from './persistencia';
import {
  CLAVE_LATERAL_ANCHO,
  CLAVE_PANEL_DERECHO,
  guardarSidebar,
  leerPreferencia,
} from './persistencia';

/** Núcleo del panel derecho: montaje, adaptador y paneles. */
export interface PanelDerechoNucleo {
  cuerpo: HTMLElement;
  app: HTMLElement;
  barra: BarraSuperior;
  adaptador: AdaptadorReal;
  usaTauri: boolean;
  persistencia: PersistenciaDeps;
  paneles: PanelChat[];
  panelActivo: () => PanelChat | null;
  getConversaciones: () => Array<{ id: string }>;
}

/** Navegador embebido y aperturas delegadas. */
export interface PanelDerechoNavegador {
  onCambioWorkspace: (accion: (ruta: string | null) => void) => void;
  navegadorRaiz: HTMLElement;
  mostrarNavegador: (visible: boolean) => void;
  estaNavegadorAbierto: () => boolean;
  abrirNavegador: () => void;
  abrirChatLateral: () => void;
  abrirChatLateralPorId: (id: string) => Promise<void>;
}

export interface PanelDerechoDeps
  extends PanelDerechoNucleo, PanelDerechoNavegador {}

/** Piezas montadas del panel derecho. */
export interface PanelDerechoPiezas {
  panelDerecho: PanelDerecho;
  files: PanelFiles;
  git: PanelGit;
  toastGlobal: ToastGlobal;
}

/** Visibilidad del panel derecho (vacío = oculto). */
export interface PanelDerechoVisibilidad {
  asegurarPanelDerecho: () => void;
  ocultarPanelDerecho: () => void;
  alternarPanelDerecho: () => void;
  cerrarPanelDerechoSiVacio: () => void;
  pintarToggleDerecho: () => void;
  restaurarEstado: () => Promise<void>;
}

/** Aperturas delegadas (archivos, git, reposicionado del webview). */
export interface PanelDerechoAperturas {
  abrirFiles: () => void;
  abrirGit: () => void;
  reposicionarWebview: () => void;
}

export interface PanelDerechoTodo
  extends PanelDerechoPiezas, PanelDerechoVisibilidad, PanelDerechoAperturas {}

export function montarPanelDerechoTodo(deps: PanelDerechoDeps): PanelDerechoTodo {
  /* [109A-6] Ancho + grip viven en `panelDerechoAncho` (medir, aplicar y
   * persistir); aquí solo se monta el divisor y se restaura lo guardado. */
  const ancho = crearAnchoPanelDerecho({
    cuerpo: deps.cuerpo,
    app: deps.app,
    persistencia: deps.persistencia,
  });
  let gripPanelDerecho: HTMLElement | null = null;

  // Files es un pane único estilo Synara: árbol a la izquierda + preview a la
  // derecha; el preview forma parte del mismo pane.
  const toastGlobal: ToastGlobal = montarToastGlobal();
  const files = montarPanelFiles({
    transporte: deps.adaptador.sesion.filesystem,
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
  });
  const git = montarPanelGit({
    transporte: { estado: deps.adaptador.sesion.filesystem.gitEstado },
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
  });
  const panelDerecho = montarPanelDerecho({
    onCambioTab(id) {
      // La webview hija es nativa: no respeta `hidden`. Al salir de su tab
      // se oculta (sin destruirla) y al volver se muestra + reposiciona.
      guardarEstadoPanel();
      if (!deps.usaTauri || !deps.estaNavegadorAbierto()) return;
      if (id === 'navegador') {
        void invoke('navegador_mostrar', { visible: true })
          .then(() => reposicionarWebview())
          .catch(() => {});
      } else {
        void invoke('navegador_mostrar', { visible: false }).catch(() => {});
      }
    },
    // [089A-4] El usuario eligió una opción del inicio (panel sin tabs).
    onElegirInicio(opcion) {
      if (opcion === 'files') abrirFiles();
      else if (opcion === 'git') abrirGit();
      else if (opcion === 'navegador') deps.abrirNavegador();
      else deps.abrirChatLateral();
    },
  });
  // La barra de tabs comparte la barra superior; el panel derecho conserva
  // únicamente el contenido y el launcher para no dejar una segunda barra.
  deps.barra.montarTabs(panelDerecho.tabsBarra);

  // [089A-11] Al cambiar de área (workspace activo) se recargan las tabs
  // abiertas que dependen de la raíz del backend: Files y Git local.
  deps.onCambioWorkspace(() => {
    if (panelDerecho.tiene('files')) files.recargar();
    if (panelDerecho.tiene('git')) git.recargar();
  });

  // Visibilidad del panel derecho (independiente de sus tabs: ocultar no
  // destruye; las tabs y sus nodos vivos se conservan). Arranca oculto.
  let panelDerechoVisible = false;
  let restaurandoEstado = false;
  function guardarEstadoPanel(): void {
    if (restaurandoEstado) return;
    const estado = panelDerecho.estado();
    guardarSidebar(
      deps.persistencia,
      CLAVE_PANEL_DERECHO,
      // El JSON persistido expone `visible` (visibilidad del panel completo).
      JSON.stringify({ visible: panelDerechoVisible, tabs: estado.tabs, activa: estado.activa }),
    );
  }

  // Refleja la visibilidad en el toggle de la barra superior global.
  function pintarToggleDerecho(): void {
    panelDerecho.setTabsVisibles(panelDerechoVisible);
    deps.paneles
      .find((p) => p.tipo === 'principal')
      ?.setPanelDerechoAbierto(panelDerechoVisible);
    // [089A-3] La cabecera ya no tiene el toggle (su setter es no-op); el
    // espejo real vive en la barra superior.
    deps.barra.setPanelDerechoAbierto(panelDerechoVisible);
  }

  // Monta el panel derecho en #cuerpo (con su grip) si aún no está.
  function asegurarPanelDerecho(): void {
    if (!gripPanelDerecho) {
      gripPanelDerecho = ancho.crearGrip();
      ancho.restaurar();
    }
    if (!gripPanelDerecho.parentNode) deps.cuerpo.appendChild(gripPanelDerecho);
    if (!panelDerecho.raiz.parentNode) deps.cuerpo.appendChild(panelDerecho.raiz);
    panelDerechoVisible = true;
    pintarToggleDerecho();
    guardarEstadoPanel();
  }

  // Oculta el panel derecho sin destruir sus tabs (el toggle lo reabre).
  function ocultarPanelDerecho(): void {
    guardarEstadoPanel();
    gripPanelDerecho?.remove();
    gripPanelDerecho = null;
    panelDerecho.raiz.remove();
    panelDerechoVisible = false;
    pintarToggleDerecho();
    guardarEstadoPanel();
  }

  // Alterna el panel derecho (toggle de la barra superior). Sin tabs
  // muestra el inicio para elegir contenido (089A-4, estilo Synara).
  function alternarPanelDerecho(): void {
    if (panelDerechoVisible) ocultarPanelDerecho();
    else asegurarPanelDerecho();
  }

  // Sin tabs el panel se queda mostrando el inicio (089A-4): ya no se
  // desmonta al cerrar la última tab; el toggle superior controla su visibilidad.
  function cerrarPanelDerechoSiVacio(): void {
    if (panelDerecho.hayTabs()) return;
    /* el inicio ocupa el panel; nada que desmontar */
  }

  // Reposiciona la webview hija sobre su contenedor (tras mostrar su tab).
  function reposicionarWebview(): void {
    const contenedor = porId('navegador-webview-contenedor');
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

  function abrirFiles(): void {
    files.sincronizarCambios(deps.panelActivo()?.listarCambios() ?? []);
    files.recargar();
    asegurarPanelDerecho();
    panelDerecho.abrirTab('files', 'Files', files.raiz, () => {
      panelDerecho.cerrarTab('files');
      guardarEstadoPanel();
      cerrarPanelDerechoSiVacio();
    });
    guardarEstadoPanel();
  }

  function abrirGit(): void {
    git.recargar();
    asegurarPanelDerecho();
    panelDerecho.abrirTab('git', 'Git local', git.raiz, () => {
      panelDerecho.cerrarTab('git');
      guardarEstadoPanel();
      cerrarPanelDerechoSiVacio();
    });
    guardarEstadoPanel();
  }

  async function restaurarEstado(): Promise<void> {
    try {
      const anchoPreferido = await leerPreferencia(deps.persistencia, CLAVE_LATERAL_ANCHO);
      if (anchoPreferido) {
        const n = Number(anchoPreferido);
        if (Number.isFinite(n)) {
          ancho.aplicar(ancho.acotar(n));
        }
      }
      const serializado = await leerPreferencia(deps.persistencia, CLAVE_PANEL_DERECHO);
      if (!serializado) return;
      const candidato = JSON.parse(serializado) as {
        visible?: unknown;
        tabs?: unknown;
        activa?: unknown;
      };
      const tabs = Array.isArray(candidato.tabs)
        ? candidato.tabs.filter(
            (id): id is string =>
              (id === 'files' || id === 'git' || id === 'navegador') ||
              (typeof id === 'string' && /^chat:[^:]+$/.test(id) && !id.startsWith('chat:nuevo-')),
          )
        : [];
      const activa = typeof candidato.activa === 'string' && tabs.includes(candidato.activa)
        ? candidato.activa
        : null;
      // Estados anteriores no tenían `visible`: si conservaron tabs, el
      // comportamiento compatible es restaurar el panel abierto. Los estados
      // actuales distinguen explícitamente panel oculto de panel sin tabs.
      const visible = typeof candidato.visible === 'boolean'
        ? candidato.visible
        : tabs.length > 0;
      restaurandoEstado = true;
      if (tabs.length > 0 || visible) asegurarPanelDerecho();
      const noRestauradas: string[] = [];
      for (const id of tabs) {
        if (id === 'files') abrirFiles();
        else if (id === 'git') abrirGit();
        else if (id === 'navegador') deps.abrirNavegador();
        else if (deps.getConversaciones().some((c) => c.id === id.slice('chat:'.length))) {
          await deps.abrirChatLateralPorId(id.slice('chat:'.length));
        } else {
          // La conversación guardada ya no existe: no se restaura su tab ni
          // se conserva en el estado persistido (auto-sanación observable).
          noRestauradas.push(id);
        }
      }
      if (noRestauradas.length > 0) {
        deps.persistencia.avisar(
          `no se pudieron restaurar ${noRestauradas.length} pestaña(s) del panel derecho`,
        );
      }
      panelDerecho.restaurarActiva(activa);
      panelDerechoVisible = visible;
      if (panelDerechoVisible) {
        pintarToggleDerecho();
      } else {
        ocultarPanelDerecho();
      }
      restaurandoEstado = false;
      guardarEstadoPanel();
    } catch (e: unknown) {
      restaurandoEstado = false;
      deps.persistencia.avisar(`no se pudo restaurar el panel derecho: ${String(e)}`);
    }
  }

  return {
    panelDerecho,
    files,
    git,
    toastGlobal,
    asegurarPanelDerecho,
    ocultarPanelDerecho,
    alternarPanelDerecho,
    cerrarPanelDerechoSiVacio,
    pintarToggleDerecho,
    restaurarEstado,
    abrirFiles,
    abrirGit,
    reposicionarWebview,
  };
}
