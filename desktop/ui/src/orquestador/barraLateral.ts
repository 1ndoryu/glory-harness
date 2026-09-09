/* Sidebar + grip de redimensionado + auto-ocultado (extraído de main.ts [089A-16 F1b]).
 * Incluye `renombrarEnLista` y las claves de persistencia del ancho/colapso.
 * Todo el estado compartido llega por `deps`; sin importes del orquestador.
 * Los callbacks que cierran sobre piezas declaradas después en el arranque
 * (modal, modalProyecto, navegador, principal, laterales) solo se invocan
 * desde eventos de UI posteriores al montaje, fuera de la TDZ. */

import { el } from '../util/dom';
import { montarSidebar, type Sidebar } from '../componentes/sidebar';
import type { BarraSuperior } from '../componentes/barraSuperior';
import type { Conversacion, Workspace } from '../dominio/tipos';
import type { PanelChat } from '../componentes/panelChat';
import type { AdaptadorReal } from '../tauri/real';
import type { PersistenciaDeps } from './persistencia';
import { guardarSidebar } from './persistencia';
import { puedeAbrirLateralEn } from './laterales';

export const CLAVE_ANCHO = 'sidebar_ancho';
export const CLAVE_COLAPSADA = 'sidebar_colapsada';

export interface BarraLateralDeps {
  cuerpo: HTMLElement;
  barra: BarraSuperior;
  adaptador: AdaptadorReal;
  usaReal: boolean;
  usaMock: boolean;
  persistencia: PersistenciaDeps;
  conversacionesIniciales: Conversacion[];
  proyectosIniciales: Workspace[];
  proyectoActivoInicial: Workspace | null;
  getConversaciones: () => Conversacion[];
  setConversaciones: (c: Conversacion[]) => void;
  getTurnoGlobal: () => boolean;
  paneles: PanelChat[];
  panelActivo: () => PanelChat | null;
  activarPanel: (panel: PanelChat | null) => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
  resincronizarSidebar: () => Promise<void>;
  abrirEnLateral: (id: string) => void;
  abrirConfig: () => void;
  abrirModalProyecto: () => void;
  alternarNavegador: () => void;
  getPrincipal: () => PanelChat | null;
}

export interface BarraLateral {
  sidebar: Sidebar;
  grip: HTMLElement;
  alternarSidebar: () => void;
  pintarSidebar: () => void;
  fijarAbierta: (abierta: boolean) => void;
  renombrarEnLista: (id: string, titulo: string) => Promise<void>;
}

const UMBRAL_AUTO_SIDEBAR = 720;

export function montarBarraLateral(deps: BarraLateralDeps): BarraLateral {
  async function renombrarEnLista(id: string, titulo: string): Promise<void> {
    const conv = deps.getConversaciones().find((c) => c.id === id);
    if (conv) conv.titulo = titulo;
    deps.paneles.forEach((p) => {
      if (p.conversaId === id) p.ponerTitulo(titulo);
    });
    sidebar.sustituir(deps.getConversaciones());
    if (!deps.usaReal) return;
    try {
      const ok = await deps.adaptador.sesion.renombrar(id, titulo);
      if (!ok) {
        deps.avisar('el backend no renombró (id ajeno o inexistente)', '', '');
        await deps.resincronizarSidebar();
      }
    } catch (e: unknown) {
      deps.avisar(`no se pudo renombrar: ${String(e)}`, '', '');
      await deps.resincronizarSidebar();
    }
  }

  const sidebar = montarSidebar({
    conversaciones: deps.conversacionesIniciales,
    proyectos: deps.proyectosIniciales,
    proyectoActivo: deps.proyectoActivoInicial,
    onSeleccionar(id) {
      // La sidebar carga la conversación en el panel ENFOCADO.
      const panel = deps.panelActivo();
      if (!panel) return;
      if (deps.usaMock) {
        panel.ponerTitulo(
          deps.getConversaciones().find((c) => c.id === id)?.titulo ?? 'Sin conversación',
        );
        void panel.cargarConversacion(id);
        deps.activarPanel(panel);
        return;
      }
      void (async () => {
        await panel.cargarConversacion(id);
        deps.activarPanel(panel);
      })();
    },
    onRenombrar(id, titulo) {
      void renombrarEnLista(id, titulo);
    },
    onArchivar(id, archivada) {
      const conv = deps.getConversaciones().find((c) => c.id === id);
      if (conv) conv.archivada = archivada;
      if (!deps.usaReal) return;
      void (async () => {
        try {
          const ok = await deps.adaptador.sesion.archivar(id, archivada);
          if (!ok) {
            deps.avisar('el backend no archivó (id ajeno o inexistente)', '', '');
            await deps.resincronizarSidebar();
          }
        } catch (e: unknown) {
          deps.avisar(`no se pudo archivar: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
    onEliminar(id) {
      // Quitar de la lista local y repintar.
      deps.setConversaciones(deps.getConversaciones().filter((c) => c.id !== id));
      sidebar.sustituir(deps.getConversaciones());
      if (!deps.usaReal) {
        // En mock: si algún panel mostraba el id, muéstrale otra o límpialo.
        const resto = deps.getConversaciones().find((c) => !c.archivada);
        deps.paneles.forEach((p) => {
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
          const actual = await deps.adaptador.sesion.eliminar(id, deps.panelActivo()?.tipo);
          await deps.resincronizarSidebar();
          // [069A-7] Si algún panel mostraba el id eliminado: si el backend
          // devolvió una conversación (quedaba otra), se carga; si `null`
          // (no quedó ninguna), el panel pasa a BORRADOR sin fila fantasma.
          deps.paneles.forEach((p) => {
            if (p.conversaId === id) {
              if (actual) void p.cargarConversacion(actual.id);
              else p.ponerBorrador();
            }
          });
        } catch (e: unknown) {
          deps.avisar(`no se pudo eliminar: ${String(e)}`, '', '');
          await deps.resincronizarSidebar();
        }
      })();
    },
    onAccionNav(accion) {
      if (accion === 'nueva') {
        // [069A-7] "Nueva conversación" = borrador local (sin fila): la fila
        // se crea al escribir el primer mensaje (create-on-write). En mock se
        // mantiene la semántica histórica (crea una conversación local).
        if (deps.usaReal) {
          const panel = deps.panelActivo();
          if (panel) {
            panel.ponerBorrador();
            deps.activarPanel(panel);
          }
          return;
        }
        void (async () => {
          const n = deps.getConversaciones().length + 1;
          const nueva: Conversacion = {
            id: `local-${Date.now()}`,
            titulo: `Conversación ${n}`,
          };
          deps.setConversaciones([nueva, ...deps.getConversaciones()]);
          sidebar.sustituir(deps.getConversaciones());
          const panel = deps.panelActivo();
          if (panel) {
            panel.limpiar();
            panel.ponerTitulo(nueva.titulo);
            await panel.cargarConversacion(nueva.id);
            deps.activarPanel(panel);
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
        deps.alternarNavegador();
        return;
      }
      deps.panelActivo()?.avisoLocal(
        `${nombre}: próximamente`,
        'sin backend',
        `${nombre} no está disponible en esta versión.`,
      );
    },
    puedeAbrirLateral: () => puedeAbrirLateralEn(deps.paneles),
    onAbrirEnLateral(id) {
      deps.abrirEnLateral(id);
    },
    // [069A-Proyectos] Abre el modal para crear un nuevo proyecto.
    onCrearProyecto() {
      deps.abrirModalProyecto();
    },
    // [069A-Proyectos] Cambia el proyecto activo por ruta.
    onSeleccionarProyecto(ruta: string) {
      if (!deps.usaReal) return;
      void (async () => {
        try {
          // No permitir si hay turno en curso.
          if (deps.getTurnoGlobal()) {
            deps.avisar(
              'hay un turno en curso',
              '',
              'espera a que termine antes de cambiar de proyecto',
            );
            return;
          }
          await deps.adaptador.sesion.workspaces.activarPorRuta(ruta);
          // onSesion / refrescarProyectos refrescarán sidebar + lista.
          // El panel principal pasa a borrador (sin conversación del otro proyecto).
          const principal = deps.getPrincipal();
          if (principal) {
            principal.ponerBorrador();
            deps.activarPanel(principal);
          }
        } catch (e: unknown) {
          deps.avisar(`no se pudo cambiar de proyecto: ${String(e)}`, '', '');
        }
      })();
    },
    abrirConfig: () => deps.abrirConfig(),
  });

  const grip = el('div', 'sidebar-grip');
  grip.setAttribute('aria-hidden', 'true');
  let sidebarAbierta = true;
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
    const cuerpo = deps.cuerpo;
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
        guardarSidebar(deps.persistencia, CLAVE_ANCHO, String(ANCHO_REABRIR));
        guardarSidebar(deps.persistencia, CLAVE_COLAPSADA, '1');
      } else {
        cuerpo.style.setProperty('--sidebar-ancho', `${Math.max(MIN, ancho)}px`);
        guardarSidebar(deps.persistencia, CLAVE_ANCHO, String(Math.max(MIN, ancho)));
        guardarSidebar(deps.persistencia, CLAVE_COLAPSADA, '0');
      }
    });
  }

  // [039A-3 P6b] La lista de conversaciones NO se oculta con un botón manual:
  // se oculta sola si la ventana se reduce por debajo de un ancho mínimo
  // (auto), y el botón de la cabecera solo sirve para MOSTRARLA (forzar) si
  // quedó oculta por ese auto-ocultado. `sidebarForzada` recuerda que el
  // usuario pidió verla aunque la ventana sea angosta; al ensanchar se resetea.
  let sidebarForzada = false;

  // ¿La ventana es tan angosta que la lista no debe ocupar espacio?
  function ventanaAngosta(): boolean {
    // [039A-3 P6b] Auto-ocultado de la lista: por debajo de un ancho mínimo de
    // ventana la sidebar ya no cabe junto al chat y se oculta sola.
    return window.innerWidth < UMBRAL_AUTO_SIDEBAR;
  }

  // Aplica el estado efectivo (preferencia + auto-ocultado por ancho).
  function aplicarSidebar(): void {
    const angosta = ventanaAngosta();
    if (!angosta) sidebarForzada = false;
    const visible = sidebarAbierta && (!angosta || sidebarForzada);
    deps.cuerpo.classList.toggle('sidebar-colapsada', !visible);
    deps.paneles.forEach((p) => p.setSidebarAbierta(visible));
    // [089A-3] Espejo en la barra superior global (la cabecera ya no tiene
    // el toggle; su setter es no-op).
    deps.barra.setSidebarAbierta(visible);
  }
  function pintarSidebar(): void {
    aplicarSidebar();
  }
  // El botón de la cabecera SOLO muestra la lista (nunca la oculta).
  function mostrarSidebar(): void {
    sidebarAbierta = true;
    if (ventanaAngosta()) sidebarForzada = true;
    aplicarSidebar();
    guardarSidebar(deps.persistencia, CLAVE_COLAPSADA, '0');
  }
  // [089A-2] Oculta la lista (manual; el auto-ocultado por ancho sigue).
  function ocultarSidebar(): void {
    sidebarAbierta = false;
    sidebarForzada = false;
    aplicarSidebar();
    guardarSidebar(deps.persistencia, CLAVE_COLAPSADA, '1');
  }
  // [089A-2] Alterna la lista (botón siempre visible de la cabecera).
  function alternarSidebar(): void {
    const visible = !deps.cuerpo.classList.contains('sidebar-colapsada');
    if (visible) ocultarSidebar();
    else mostrarSidebar();
  }

  // Fija la preferencia de apertura (arranque: colapso persistido).
  function fijarAbierta(abierta: boolean): void {
    sidebarAbierta = abierta;
  }

  return { sidebar, grip, alternarSidebar, pintarSidebar, fijarAbierta, renombrarEnLista };
}
