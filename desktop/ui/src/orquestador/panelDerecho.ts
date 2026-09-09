/* Panel derecho con tabs + Files/Git + grip de ancho (extraído de main.ts [089A-16 F1b]).
 * Monta toastGlobal, files, git y el panel derecho; gestiona visibilidad,
 * grip de redimensionado y apertura de tabs. Todo el estado compartido llega
 * por `deps`; sin importes del orquestador. `abrirNavegador` y
 * `abrirChatLateral` cierran sobre piezas declaradas después en el arranque
 * y solo se invocan desde eventos de UI, fuera de la TDZ. */

import { invoke } from '@tauri-apps/api/core';
import { el } from '../util/dom';
import { montarPanelDerecho, type PanelDerecho } from '../componentes/panelDerecho';
import { montarPanelFiles, type PanelFiles } from '../componentes/panelFiles';
import { montarPanelGit, type PanelGit } from '../componentes/panelGit';
import { montarToastGlobal, type ToastGlobal } from '../componentes/toastGlobal';
import type { PanelChat } from '../componentes/panelChat';
import type { BarraSuperior } from '../componentes/barraSuperior';
import type { AdaptadorReal } from '../tauri/real';
import type { PersistenciaDeps } from './persistencia';
import { guardarSidebar, leerSidebar } from './persistencia';

const CLAVE_LATERAL_ANCHO = 'lateral_ancho';

export interface PanelDerechoDeps {
  cuerpo: HTMLElement;
  app: HTMLElement;
  barra: BarraSuperior;
  adaptador: AdaptadorReal;
  usaTauri: boolean;
  persistencia: PersistenciaDeps;
  paneles: PanelChat[];
  panelActivo: () => PanelChat | null;
  onCambioWorkspace: (accion: (ruta: string | null) => void) => void;
  navegadorRaiz: HTMLElement;
  mostrarNavegador: (visible: boolean) => void;
  estaNavegadorAbierto: () => boolean;
  abrirNavegador: () => void;
  abrirChatLateral: () => void;
}

export interface PanelDerechoTodo {
  panelDerecho: PanelDerecho;
  files: PanelFiles;
  git: PanelGit;
  toastGlobal: ToastGlobal;
  asegurarPanelDerecho: () => void;
  ocultarPanelDerecho: () => void;
  alternarPanelDerecho: () => void;
  cerrarPanelDerechoSiVacio: () => void;
  pintarToggleDerecho: () => void;
  abrirFiles: () => void;
  abrirGit: () => void;
  reposicionarWebview: () => void;
}

export function montarPanelDerechoTodo(deps: PanelDerechoDeps): PanelDerechoTodo {
  // [089A-2] Divisor vertical arrastrable entre #paneles y el panel derecho.
  // Vive en #cuerpo (hijo flex de 9px, no absolute) y fija
  // `--panel-derecho-ancho` en #cuerpo dentro de [260, 70%]; se persiste en
  // la misma clave de siempre (sigue siendo "el ancho del panel derecho").
  let gripPanelDerecho: HTMLElement | null = null;
  let anchoPanelDerechoFijado: number | null = null;
  function medirAnchoCuerpo(): number {
    return deps.cuerpo.getBoundingClientRect().width;
  }
  function aplicarPanelDerechoAncho(px: number): void {
    anchoPanelDerechoFijado = px;
    deps.cuerpo.style.setProperty('--panel-derecho-ancho', `${px}px`);
    deps.app.style.setProperty('--panel-derecho-ancho', `${px}px`);
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
      const rect = deps.cuerpo.getBoundingClientRect();
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
        guardarSidebar(deps.persistencia, CLAVE_LATERAL_ANCHO, String(anchoPanelDerechoFijado));
      }
    });
    return g;
  }

  // Al abrir el panel derecho se restaura el ancho persistido (si cabe en el
  // 70% disponible).
  function restaurarPanelDerechoAncho(): void {
    const base = leerSidebar(deps.persistencia, CLAVE_LATERAL_ANCHO);
    const anchoCuerpo = medirAnchoCuerpo();
    const MAX = Math.round(anchoCuerpo * 0.7);
    let px = Math.round(anchoCuerpo * 0.5); // parte de la mitad
    if (base) {
      const n = Number(base);
      if (Number.isFinite(n)) px = Math.round(Math.min(MAX, Math.max(260, n)));
    }
    aplicarPanelDerechoAncho(Math.min(MAX, Math.max(260, px)));
  }

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

  // Refleja la visibilidad en el toggle de la barra superior global.
  function pintarToggleDerecho(): void {
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
      gripPanelDerecho = crearGripPanelDerecho();
      restaurarPanelDerechoAncho();
    }
    if (!gripPanelDerecho.parentNode) deps.cuerpo.appendChild(gripPanelDerecho);
    if (!panelDerecho.raiz.parentNode) deps.cuerpo.appendChild(panelDerecho.raiz);
    panelDerechoVisible = true;
    pintarToggleDerecho();
  }

  // Oculta el panel derecho sin destruir sus tabs (el toggle lo reabre).
  function ocultarPanelDerecho(): void {
    gripPanelDerecho?.remove();
    gripPanelDerecho = null;
    panelDerecho.raiz.remove();
    panelDerechoVisible = false;
    pintarToggleDerecho();
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

  function abrirFiles(): void {
    files.sincronizarCambios(deps.panelActivo()?.listarCambios() ?? []);
    files.recargar();
    asegurarPanelDerecho();
    panelDerecho.abrirTab('files', 'Files', files.raiz, () => {
      panelDerecho.cerrarTab('files');
      cerrarPanelDerechoSiVacio();
    });
  }

  function abrirGit(): void {
    git.recargar();
    asegurarPanelDerecho();
    panelDerecho.abrirTab('git', 'Git local', git.raiz, () => {
      panelDerecho.cerrarTab('git');
      cerrarPanelDerechoSiVacio();
    });
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
    abrirFiles,
    abrirGit,
    reposicionarWebview,
  };
}
