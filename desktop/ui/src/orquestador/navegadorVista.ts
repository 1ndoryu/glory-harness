/* Navegador interno como tab del panel derecho (extraído de main.ts [089A-16 F1b]).
 * [069A-2 fix] En Tauri pilota la webview child (IPC); en modo web, un iframe
 * autogestionado. Todo el estado compartido llega por `deps`; sin importes
 * del orquestador. Las piezas del panel derecho llegan como cierres perezosos
 * porque ese módulo se crea después (solo se invocan en runtime). */

import { invoke } from '@tauri-apps/api/core';
import { porId } from '../util/dom';
import { montarPanelNavegador, type PanelNavegador } from '../componentes/panelNavegador';
import type { PanelChat } from '../componentes/panelChat';
import { crearSupresionNavegador } from './navegadorAuto';

export interface NavegadorVistaDeps {
  usaTauri: boolean;
  panelActivo: () => PanelChat | null;
  activarPanel: (panel: PanelChat | null) => void;
  avisar: (texto: string, meta: string, detalle: string) => void;
  asegurarPanelDerecho: () => void;
  cerrarPanelDerechoSiVacio: () => void;
  abrirTabNavegador: (raiz: HTMLElement, onCerrar: () => void) => void;
  cerrarTabNavegador: () => void;
  /** [129A-10 F1] true si hay un turno corriendo (cierre = intencional). */
  turnoEnCurso: () => boolean;
}

export interface NavegadorVista {
  navegador: PanelNavegador;
  estaAbierto: () => boolean;
  abrirNavegador: () => void;
  cerrarNavegador: () => void;
  /** [129A-10 F1] El orquestador la llama al iniciar cada turno (levanta la
   * supresión del turno anterior). */
  notificarTurnoInicio: () => void;
  /** [129A-10 F1] Auto-apertura por el agente: abre sin robar el foco
   * (`abrirTab` solo conmuta la tab visible); 'suprimida' = el usuario la
   * cerró a mitad de turno (el llamador solo avisa por toast). */
  abrirPorAgente: () => 'abierta' | 'ya' | 'suprimida';
}

export function montarNavegadorVista(deps: NavegadorVistaDeps): NavegadorVista {
  const navegador = montarPanelNavegador({
    // [seleccionar] El usuario eligió un elemento de la página con el modo
    // "seleccionar": se adjunta como badge al chat activo para que el modelo
    // reciba el descriptor (URL + selector CSS) en el próximo mensaje.
    onSeleccionar(elem) {
      const panel = deps.panelActivo();
      if (!panel) {
        deps.avisar('abre un chat para recibir el elemento', '', elem.etiqueta);
        return;
      }
      deps.activarPanel(panel);
      panel.adjuntarElemento(elem);
    },
    /* [109A-9] Los fallos internos del panel (capturar, atrás/adelante,
     * selección) salen por el mismo canal que los del orquestador en vez de
     * quedarse en `console.*`. */
    onAviso: (texto, detalle = '') => deps.avisar(texto, '', detalle),
  });
  let navegadorAbierto = false;
  // [129A-10 F1] Cierre intencional a mitad de turno: el auto-abrir del
  // agente se abstiene hasta el siguiente turno o reapertura manual.
  const supresion = crearSupresionNavegador();

  function estaAbierto(): boolean {
    return navegadorAbierto;
  }

  function abrirNavegador(): void {
    if (navegadorAbierto) return;
    supresion.alAbrirManual();
    // [089A-2] El navegador vive como tab del panel derecho y convive con el
    // chat lateral, Files y Git local.
    navegadorAbierto = true;
    deps.asegurarPanelDerecho();
    deps.abrirTabNavegador(navegador.raiz, () => cerrarNavegador());
    navegador.mostrar(true);

    if (!deps.usaTauri) {
      // [069A-2 fix] Modo web: el iframe se autogestiona; solo se fija la URL
      // inicial para que el área no quede en blanco.
      navegador.irA('https://example.com');
      return;
    }

    // Modo Tauri: calcular la posición del contenedor y abrir la webview child.
    void (async () => {
      try {
        const contenedor = porId('navegador-webview-contenedor');
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
        deps.avisar('no se pudo abrir el navegador', '', String(error));
      }
    })();
  }

  function cerrarNavegador(): void {
    if (!navegadorAbierto) return;
    navegadorAbierto = false;
    // Los únicos llamadores son el × de la tab y el toggle: cierre manual.
    supresion.alCerrarManual(deps.turnoEnCurso());
    navegador.mostrar(false);
    // [089A-2] Desmonta su tab; si no quedan tabs se cierra el panel derecho.
    deps.cerrarTabNavegador();
    deps.cerrarPanelDerechoSiVacio();
    // Solo en Tauri existe la webview child que cerrar; en web el iframe
    // permanece reutilizable y el panel se oculta.
    if (deps.usaTauri) {
      void invoke('navegador_cerrar').catch(() => {});
    }
    // Limpiar ResizeObserver (solo se crea en Tauri)
    const ro = (navegador as unknown as Record<string, unknown>).__resizeObserver as
      | ResizeObserver
      | undefined;
    if (ro) {
      ro.disconnect();
      delete (navegador as unknown as Record<string, unknown>).__resizeObserver;
    }
  }

  function notificarTurnoInicio(): void {
    supresion.alIniciarTurno();
  }

  function abrirPorAgente(): 'abierta' | 'ya' | 'suprimida' {
    if (supresion.suprimido()) return 'suprimida';
    if (navegadorAbierto) return 'ya';
    abrirNavegador();
    return 'abierta';
  }

  return { navegador, estaAbierto, abrirNavegador, cerrarNavegador, notificarTurnoInicio, abrirPorAgente };
}
