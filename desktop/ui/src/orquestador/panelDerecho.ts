/* Panel derecho con tabs + Files/Git/Consola + grip de ancho (extraído de main.ts [089A-16 F1b]).
 * Monta toastGlobal, files, git, consola y el panel derecho; gestiona visibilidad,
 * grip de redimensionado y apertura de tabs. Todo el estado compartido llega
 * por `deps`; sin importes del orquestador. `abrirNavegador` y
 * `abrirChatLateral` cierran sobre piezas declaradas después en el arranque
 * y solo se invocan desde eventos de UI, fuera de la TDZ. */

import { invoke } from '@tauri-apps/api/core';
import { porId } from '../util/dom';
import { montarPanelDerecho, type PanelDerecho } from '../componentes/panelDerecho';
import { montarPanelFiles, type PanelFiles } from '../componentes/panelFiles';
import { montarPanelCambios, type PanelCambios } from '../componentes/panelCambios';
import {
  montarPanelConsola,
  type EventoConsola,
  type PanelConsola,
} from '../componentes/panelConsola';
import { crearSupresionNavegador as crearSupresionAutoapertura } from './navegadorAuto';
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
  /** Turno en curso (para la supresión de auto-apertura: igual que F1). */
  turnoEnCurso: () => boolean;
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
  /** [129A-7] La tab "Git local" ahora es "Cambios" (filesystem por turno +
   * estado git debajo). El id de tab 'git' se conserva. */
  cambios: PanelCambios;
  /** [209A-1 F3] La tab Consola: visor de ejecuciones `comando` en vivo. */
  consola: PanelConsola;
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
  /** [129A-8] Abre Cambios y revela el archivo (enlace del resumen). */
  abrirCambiosEn: (ruta: string) => void;
  /** [129A-10 F2] Abre Files y previsualiza la ruta (vista del agente). */
  abrirFilesEn: (ruta: string) => void;
  /** [209A-1 F3] Abre la Consola (tab manual: inicio, menú +, persistencia). */
  abrirConsola: () => void;
  /** [209A-1 F3] Abre la Consola y revela la ejecución (enlace del resumen). */
  abrirConsolaEn: (id: string) => void;
  /** [209A-1 F3] Auto-apertura por `consola_inicio` del agente (misma
   * máquina de supresión que F1: 'suprimida' = el usuario la cerró a mitad
   * de turno y solo se acumula hasta el próximo turno). */
  abrirConsolaPorAgente: (id: string) => 'abierta' | 'ya' | 'suprimida';
  /** [209A-1 F3] Streaming de consola hacia el store (hook del adaptador).
   * Devuelve lo que decidió la auto-apertura (para el aviso visible). */
  onConsolaEvento: (ev: EventoConsola) => 'abierta' | 'ya' | 'suprimida';
  /** [209A-1 F3] Reinicia la supresión al empezar un turno (igual que F1). */
  notificarTurnoInicioConsola: () => void;
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
  // [139A-7] Resumen batch si el transporte lo expone; si no, el panel usa
  // el fan-out clásico (sin simular el batch en web).
  const gitResumenBatch = deps.adaptador.sesion.filesystem.gitResumen;
  const git = montarPanelCambios({
    git: {
      estado: (ruta) => deps.adaptador.sesion.filesystem.gitEstado(ruta),
      repos: () => deps.adaptador.sesion.filesystem.gitRepos(),
      ...(gitResumenBatch ? { resumen: () => gitResumenBatch() } : {}),
    },
    cambios: {
      listar: (conv) => deps.adaptador.sesion.cambios(conv),
      rechazar: (conv, turno, ruta) => deps.adaptador.sesion.rechazarCambio(conv, turno, ruta),
    },
    convId: () => deps.panelActivo()?.conversaId ?? null,
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
    onToast(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
  });
  // [209A-1 F3] La tab Consola: store por `id_ejecucion` + lista y visor.
  // Los errores del panel (portapapeles) van al toast global, como Files.
  // [209A-1 F4-resto] La × sobre una viva mata en el backend (`false` =
  // ya terminó: sin aviso, el `consola_fin` la congela en la vista).
  const consola = montarPanelConsola({
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
    onMatar(idEjecucion) {
      deps.adaptador.sesion
        .matarConsola(idEjecucion)
        .catch((err: unknown) => {
          toastGlobal.mostrar('no se pudo matar la consola', String(err));
        });
    },
    // [219A-3] Puentes al backend para la sub-barra + backfill + stdin. Los
    // fallos los muestra el panel vía `onError` (aquí no se duplican).
    onSincronizar() {
      return deps.adaptador.sesion.listarConsolas();
    },
    onLeerSalida(idEjecucion) {
      return deps.adaptador.sesion.leerSalidaConsola(idEjecucion);
    },
    onEscribir(idEjecucion, texto) {
      return deps.adaptador.sesion.escribirConsola(idEjecucion, texto);
    },
  });
  // [209A-1 F3] Supresión de auto-apertura: la misma máquina pura que F1
  // (`crearSupresionNavegador` no sabe de navegadores: abrir/suprimir por
  // turno; aquí gobierna la tab Consola).
  const supresionConsola = crearSupresionAutoapertura();
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
      else if (opcion === 'consola') abrirConsola();
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
    panelDerecho.abrirTab('git', 'Cambios', git.raiz, () => {
      panelDerecho.cerrarTab('git');
      guardarEstadoPanel();
      cerrarPanelDerechoSiVacio();
    });
    guardarEstadoPanel();
  }

  // [129A-8] El resumen enlaza al archivo: abre la tab y revela su fila (con
  // su diff vivo si lo hay). `revelar` se aplica en el próximo pintado porque
  // la lista se recarga asíncrona.
  function abrirCambiosEn(ruta: string): void {
    abrirGit();
    git.revelar(ruta);
  }

  // [129A-10 F2] La tool `mostrar_archivo` enseña un archivo: abre la tab
  // Files y lo previsualiza (vista, no cambio). `abrirTab` solo conmuta la
  // tab visible, sin robar el foco del chat (igual que F1).
  function abrirFilesEn(ruta: string): void {
    abrirFiles();
    files.mostrarArchivo(ruta);
  }

  // [209A-1 F3] La Consola manual: abre la tab (el visor sigue el último
  // inicio; el usuario elige la entrada en la lista). Apertura manual: se
  // olvida la supresión (igual que F1).
  // [219A-3] Cada apertura sincroniza la sub-barra con el backend (backfill
  // al abrir a mitad de turno; sin robar la selección ni duplicar el vivo).
  function abrirConsola(): void {
    supresionConsola.alAbrirManual();
    asegurarPanelDerecho();
    panelDerecho.abrirTab('consola', 'Consola', consola.raiz, () => {
      // El único llamador es el × de la tab: cierre manual (igual que F1).
      supresionConsola.alCerrarManual(deps.turnoEnCurso());
      panelDerecho.cerrarTab('consola');
      guardarEstadoPanel();
      cerrarPanelDerechoSiVacio();
    });
    guardarEstadoPanel();
    void consola.sincronizar();
  }

  // [209A-1 F3] El resumen enlaza a la ejecución: abre la tab y revela su
  // entrada (con su salida viva si sigue corriendo).
  function abrirConsolaEn(id: string): void {
    abrirConsola();
    consola.revelar(id);
  }

  // [209A-1 F3] `consola_inicio` abre la tab SIN robar el foco (`abrirTab`
  // solo conmuta la tab visible). El cierre intencional a mitad de turno se
  // respeta (solo se acumula) hasta el próximo turno o reapertura manual,
  // igual que F1.
  function abrirConsolaPorAgente(id: string): 'abierta' | 'ya' | 'suprimida' {
    if (supresionConsola.suprimido()) {
      consola.revelar(id);
      return 'suprimida';
    }
    if (panelDerecho.tiene('consola')) {
      consola.revelar(id);
      return 'ya';
    }
    abrirConsola();
    consola.revelar(id);
    return 'abierta';
  }

  // [209A-1 F3] Cada evento alimenta el store; el inicio además auto-abre.
  // Devuelve la decisión para el aviso visible del hook.
  function onConsolaEvento(ev: EventoConsola): 'abierta' | 'ya' | 'suprimida' {
    consola.manejarEvento(ev);
    if (ev.tipo === 'consola_inicio') return abrirConsolaPorAgente(ev.id_ejecucion);
    return 'ya';
  }

  function notificarTurnoInicioConsola(): void {
    supresionConsola.alIniciarTurno();
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
              (id === 'files' ||
                id === 'git' ||
                id === 'navegador' ||
                id === 'consola') ||
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
        else if (id === 'consola') abrirConsola();
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
    cambios: git,
    consola,
    toastGlobal,
    asegurarPanelDerecho,
    ocultarPanelDerecho,
    alternarPanelDerecho,
    cerrarPanelDerechoSiVacio,
    pintarToggleDerecho,
    restaurarEstado,
    abrirFiles,
    abrirGit,
    abrirCambiosEn,
    abrirFilesEn,
    abrirConsola,
    abrirConsolaEn,
    abrirConsolaPorAgente,
    onConsolaEvento,
    notificarTurnoInicioConsola,
    reposicionarWebview,
  };
}
