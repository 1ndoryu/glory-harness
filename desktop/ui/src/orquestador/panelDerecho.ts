/* Panel derecho con tabs + Files/Git/Consola + grip de ancho (extraído de main.ts [089A-16 F1b]).
 * Monta toastGlobal, files, git, consola y el panel derecho; gestiona visibilidad,
 * grip de redimensionado y apertura de tabs. Todo el estado compartido llega
 * por `deps`; sin importes del orquestador. `abrirNavegador` y
 * `abrirChatLateral` cierran sobre piezas declaradas después en el arranque
 * y solo se invocan desde eventos de UI, fuera de la TDZ. */

import { invoke } from '@tauri-apps/api/core';
import { porId } from '../util/dom';
import { montarPanelDerecho } from '../componentes/panelDerecho';
import type { EventoConsola } from '../componentes/panelConsola';
import { crearFichaSubagente, type FichaSubagente } from '../componentes/subagentesViva';
import { crearAnchoPanelDerecho } from './panelDerechoAncho';
import { montarPiezasPanelDerecho } from './panelDerechoMontaje';
import type { PanelDerechoDeps, PanelDerechoTodo } from './panelDerechoTipos';
import {
  CLAVE_LATERAL_ANCHO,
  CLAVE_PANEL_DERECHO,
  guardarSidebar,
  leerPreferencia,
} from './persistencia';

// Los contratos viven en `panelDerechoTipos.ts`; las piezas montadas en
// `panelDerechoMontaje.ts`. Aquí queda la visibilidad y las aperturas.

export function montarPanelDerechoTodo(deps: PanelDerechoDeps): PanelDerechoTodo {
  /* [109A-6] Ancho + grip viven en `panelDerechoAncho` (medir, aplicar y
   * persistir); aquí solo se monta el divisor y se restaura lo guardado. */
  const ancho = crearAnchoPanelDerecho({
    cuerpo: deps.cuerpo,
    app: deps.app,
    persistencia: deps.persistencia,
  });
  let gripPanelDerecho: HTMLElement | null = null;

  // Las piezas (toast, Files, Cambios, Consola, supresión) las monta
  // `montarPiezasPanelDerecho` (ver `panelDerechoMontaje.ts`).
  const {
    toastGlobal,
    files,
    cambios: git,
    consola,
    supresionConsola,
  } = montarPiezasPanelDerecho({ adaptador: deps.adaptador, panelActivo: deps.panelActivo });
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

  // Monta el panel derecho en #cuerpo si aún no está. [07AA-1 F3] El grip
  // vive DENTRO del panel (anclado a su borde izquierdo): con la fila en
  // scroll horizontal, un grip anclado a #cuerpo se despegaba del divisor.
  function asegurarPanelDerecho(): void {
    if (!panelDerecho.raiz.parentNode) deps.cuerpo.appendChild(panelDerecho.raiz);
    if (!gripPanelDerecho) {
      gripPanelDerecho = ancho.crearGrip();
      ancho.restaurar();
    }
    if (!gripPanelDerecho.parentNode) panelDerecho.raiz.appendChild(gripPanelDerecho);
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

  // [129A-11 F3] Ficha de solo lectura del subagente en su propia tab
  // (`subagente:<n>`, efímera como `chat:nuevo-*`: el filtro de restauración
  // no la conoce y no se restaura). Sin caja de escritura por construcción
  // (la ficha solo tiene texto). Contador propio: los `seq` de cada tarjeta
  // empiezan en 1 y las tabs son globales del panel derecho.
  let contadorSubagentes = 0;
  function abrirSubagenteEn(ficha: FichaSubagente): void {
    contadorSubagentes += 1;
    const tabId = `subagente:${contadorSubagentes}`;
    const nodo = crearFichaSubagente(ficha);
    asegurarPanelDerecho();
    panelDerecho.abrirTab(tabId, `Subagente ${ficha.perfil}`, nodo, () => {
      panelDerecho.cerrarTab(tabId);
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
    abrirSubagenteEn,
    onConsolaEvento,
    notificarTurnoInicioConsola,
    reposicionarWebview,
  };
}
