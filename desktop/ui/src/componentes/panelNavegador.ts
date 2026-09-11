/* Panel Navegador (plan 069A-1, F3). Fábrica duplicable que encapsula los
 * controles del navegador interno WebView2 child: barra URL, botones,
 * vista previa de captura y modo "seleccionar elemento" (placeholder F4).
 * El DOM vive en `panelNavegadorControles.ts` y la selección en
 * `panelNavegadorSeleccion.ts`; aquí quedan el estado, el IPC y el cableado
 * de eventos. */
import { invoke } from '@tauri-apps/api/core';
import { esEntornoTauri } from '../tauri/real';
import { crearControlesNav } from './panelNavegadorControles';
import { crearSeleccionNav } from './panelNavegadorSeleccion';
import { codigoHistorialAdelante, codigoHistorialAtras } from '../plataforma/webview';
import type { PanelNavegador, PanelNavegadorOpciones } from './panelNavegadorTipos';

export type { PanelNavegador, PanelNavegadorOpciones } from './panelNavegadorTipos';

export function montarPanelNavegador(opts: PanelNavegadorOpciones = {}): PanelNavegador {
  const idP = opts.idPrefijo ?? 'navegador';
  // El ancho lo controla el CSS (panel-navegador, redimensionable por grip);
  // la opción se conserva en la interfaz por contrato, hoy es inerte.
  void opts.ancho;

  // [069A-2 fix] La webview nativa (WebView2 child + IPC de Tauri) solo existe
  // en la app de escritorio. En modo web el panel pilota un <iframe> del propio
  // navegador: es una vista MANUAL del usuario (las tools de navegador del
  // agente son de Tauri y no aplican aquí). `esTauri` decide la rama de cada
  // control.
  const esTauri = esEntornoTauri();

  const n = crearControlesNav(idP, esTauri);
  const { raiz, inputURL, contenedor, capturaArea, imgCaptura, iframe } = n;

  // Estado interno.
  let urlActual = '';
  let ventanaAbierta = false;

  function actualizarCaptura(base64: string): void {
    imgCaptura.src = `data:image/png;base64,${base64}`;
    capturaArea.hidden = false;
  }

  async function invocarNavegadorComando(
    comando: string,
    args: Record<string, unknown> = {},
  ): Promise<void> {
    if (!ventanaAbierta) return;
    await invoke(comando, args);
  }

  // [seleccionar] El modo vive en su módulo; navegar/recargar/atrás/adelante
  // lo apagan porque la inyección no sobrevive al cambio de página.
  const seleccion = crearSeleccionNav({
    btnSeleccionar: n.btnSeleccionar,
    esTauri,
    ventanaAbierta: () => ventanaAbierta,
    urlActual: () => urlActual,
    onSeleccionar: opts.onSeleccionar,
  });

  /** [069A-2 fix] Completa el esquema si la URL no lo trae (modo web). */
  function urlConEsquema(texto: string): string {
    const t = texto.trim();
    if (!t) return '';
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t)) return t;
    return `https://${t}`;
  }

  /** [069A-2 fix] Navega el iframe (solo modo web). */
  function navegarWeb(url: string): void {
    if (!iframe) return;
    const destino = urlConEsquema(url);
    if (!destino) return;
    urlActual = destino;
    inputURL.value = destino;
    iframe.src = destino;
  }

  function navegarURL(): void {
    const url = inputURL.value.trim();
    if (!url) return;
    urlActual = url;
    if (esTauri) {
      seleccion.apagar();
      void invocarNavegadorComando('navegador_navegar', { url }).catch(() => {});
    } else {
      navegarWeb(url);
    }
  }

  n.btnIr.addEventListener('click', navegarURL);
  inputURL.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') navegarURL();
  });

  n.btnCapturar.addEventListener('click', () => {
    void (async () => {
      try {
        if (!esTauri) return;
        const base64 = await invoke<string>('navegador_capturar');
        actualizarCaptura(base64);
      } catch (error) {
        console.error('No se pudo capturar el navegador', error);
      }
    })();
  });

  n.btnRecargar.addEventListener('click', () => {
    if (!urlActual) return;
    seleccion.apagar();
    if (esTauri) {
      void invocarNavegadorComando('navegador_navegar', { url: urlActual }).catch(() => {});
    } else if (iframe) {
      // [069A-2 fix] Recargar el iframe: location.reload() desde el padre está
      // permitido (navegación); sin ventana se reasigna el src.
      const destino = urlActual;
      const w = iframe.contentWindow;
      if (w) {
        try {
          w.location.reload();
        } catch {
          iframe.src = destino;
        }
      } else {
        iframe.src = destino;
      }
    }
  });

  n.btnAtras.addEventListener('click', () => {
    void (async () => {
      try {
        seleccion.apagar();
        if (esTauri) {
          // CDP: Runtime.evaluate con history.back()
          await invoke('navegador_js', { codigo: codigoHistorialAtras() });
        } else {
          // [069A-2 fix] El historial del iframe se controla desde el padre.
          iframe?.contentWindow?.history.back();
        }
      } catch (error) {
        console.error('No se pudo navegar atrás', error);
      }
    })();
  });

  n.btnAdelante.addEventListener('click', () => {
    void (async () => {
      try {
        seleccion.apagar();
        if (esTauri) {
          await invoke('navegador_js', { codigo: codigoHistorialAdelante() });
        } else {
          // [069A-2 fix] El historial del iframe se controla desde el padre.
          iframe?.contentWindow?.history.forward();
        }
      } catch (error) {
        console.error('No se pudo navegar adelante', error);
      }
    })();
  });

  n.cerrarCaptura.addEventListener('click', () => {
    capturaArea.hidden = true;
    imgCaptura.src = '';
  });

  return {
    raiz,
    contenedor,
    mostrar(visible: boolean) {
      raiz.hidden = !visible;
      if (visible) {
        ventanaAbierta = true;
      } else {
        ventanaAbierta = false;
        seleccion.apagar();
      }
    },
    desactivarSeleccion() {
      seleccion.apagar();
    },
    fijarURL(url: string) {
      urlActual = url;
      inputURL.value = url;
    },
    irA(url: string) {
      if (esTauri) {
        urlActual = url;
        inputURL.value = url;
        seleccion.apagar();
        void invocarNavegadorComando('navegador_navegar', { url }).catch(() => {});
      } else {
        navegarWeb(url);
      }
    },
    actualizarCaptura(base64: string) {
      actualizarCaptura(base64);
    },
  };
}
