/* Panel Navegador (plan 069A-1, F3). Fábrica duplicable que encapsula los
 * controles del navegador interno WebView2 child: barra URL, botones de
 * navegación, vista previa de captura, log de acciones del agente y modo
 * "seleccionar elemento" (placeholder F4). El DOM vive en
 * `panelNavegadorControles.ts` y la selección en `panelNavegadorSeleccion.ts`;
 * aquí quedan el estado, el IPC y el cableado de eventos. */
import { invoke } from '@tauri-apps/api/core';
import { el } from '../util/dom';
import { esEntornoTauri } from '../tauri/real';
import { crearControlesNav } from './panelNavegadorControles';
import { crearSeleccionNav } from './panelNavegadorSeleccion';
import { codigoHistorialAdelante, codigoHistorialAtras } from '../plataforma/webview';
import { MAX_LOG } from './panelNavegadorTipos';
import type {
  AccionNavegador,
  PanelNavegador,
  PanelNavegadorOpciones,
} from './panelNavegadorTipos';

export type { AccionNavegador, PanelNavegador, PanelNavegadorOpciones } from './panelNavegadorTipos';

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
  const { raiz, inputURL, contenedor, logLista, capturaArea, imgCaptura, iframe } = n;

  // Estado interno.
  let urlActual = '';
  let acciones: AccionNavegador[] = [];
  let ventanaAbierta = false;

  function anadirAlLog(accion: AccionNavegador): void {
    acciones.push(accion);
    if (acciones.length > MAX_LOG) {
      acciones = acciones.slice(acciones.length - MAX_LOG);
    }
    const item = el('li', 'nav-log-item');
    item.dataset.ok = String(accion.ok);
    const ts = new Date(accion.tiempo).toLocaleTimeString();
    item.textContent = `[${ts}] ${accion.herramienta}: ${accion.descripcion}`;
    logLista.appendChild(item);
    logLista.scrollTop = logLista.scrollHeight;
  }

  function actualizarCaptura(base64: string): void {
    imgCaptura.src = `data:image/png;base64,${base64}`;
    capturaArea.style.display = '';
  }

  async function invocarNavegadorComando(
    comando: string,
    args: Record<string, unknown> = {},
  ): Promise<void> {
    if (!ventanaAbierta) {
      anadirAlLog({ herramienta: comando, descripcion: 'navegador cerrado', ok: false, tiempo: Date.now() });
      return;
    }
    try {
      await invoke(comando, args);
      anadirAlLog({
        herramienta: comando,
        descripcion: args.url ? `navegar a ${(args.url as string).slice(0, 80)}` : comando,
        ok: true,
        tiempo: Date.now(),
      });
    } catch (error) {
      anadirAlLog({
        herramienta: comando,
        descripcion: `error: ${String(error).slice(0, 120)}`,
        ok: false,
        tiempo: Date.now(),
      });
      throw error;
    }
  }

  // [seleccionar] El modo vive en su módulo; navegar/recargar/atrás/adelante
  // lo apagan porque la inyección no sobrevive al cambio de página.
  const seleccion = crearSeleccionNav({
    btnSeleccionar: n.btnSeleccionar,
    esTauri,
    ventanaAbierta: () => ventanaAbierta,
    urlActual: () => urlActual,
    onSeleccionar: opts.onSeleccionar,
    log: (herramienta, descripcion, ok) =>
      anadirAlLog({ herramienta, descripcion, ok, tiempo: Date.now() }),
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
    anadirAlLog({
      herramienta: 'navegar',
      descripcion: `navegar a ${destino.slice(0, 80)}`,
      ok: true,
      tiempo: Date.now(),
    });
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
        if (!esTauri) {
          anadirAlLog({
            herramienta: 'capturar',
            descripcion: 'captura solo en la app de escritorio (sin WebView2)',
            ok: false,
            tiempo: Date.now(),
          });
          return;
        }
        const base64 = await invoke<string>('navegador_capturar');
        actualizarCaptura(base64);
        anadirAlLog({
          herramienta: 'capturar',
          descripcion: `captura PNG (${Math.round(base64.length * 0.75 / 1024)} KB estimado)`,
          ok: true,
          tiempo: Date.now(),
        });
      } catch (error) {
        anadirAlLog({
          herramienta: 'capturar',
          descripcion: `error: ${String(error).slice(0, 120)}`,
          ok: false,
          tiempo: Date.now(),
        });
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
      anadirAlLog({
        herramienta: 'recargar',
        descripcion: `recargar ${destino.slice(0, 80)}`,
        ok: true,
        tiempo: Date.now(),
      });
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
        anadirAlLog({ herramienta: 'atras', descripcion: 'navegar atrás', ok: true, tiempo: Date.now() });
      } catch (error) {
        anadirAlLog({ herramienta: 'atras', descripcion: `error: ${String(error).slice(0, 120)}`, ok: false, tiempo: Date.now() });
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
        anadirAlLog({ herramienta: 'adelante', descripcion: 'navegar adelante', ok: true, tiempo: Date.now() });
      } catch (error) {
        anadirAlLog({ herramienta: 'adelante', descripcion: `error: ${String(error).slice(0, 120)}`, ok: false, tiempo: Date.now() });
      }
    })();
  });

  n.btnCerrar.addEventListener('click', () => {
    void (async () => {
      ventanaAbierta = false;
      seleccion.apagar();
      raiz.style.display = 'none';
      if (esTauri) {
        // Sin orquestador, esta fábrica cierra la webview ella misma; con
        // onCerrar lo hace el orquestador (estado + IPC) para no duplicar.
        if (!opts.onCerrar) {
          try {
            await invoke('navegador_cerrar');
          } catch {
            /* ignorar */
          }
        }
        // Limpia el contenedor (la webview se cierra aparte)
        contenedor.replaceChildren();
      } else if (iframe) {
        // [069A-2 fix] En web se conserva el iframe (reutilizable); se vacía.
        iframe.src = 'about:blank';
      }
      anadirAlLog({ herramienta: 'cerrar', descripcion: 'navegador cerrado', ok: true, tiempo: Date.now() });
      opts.onCerrar?.();
    })();
  });

  n.cerrarCaptura.addEventListener('click', () => {
    capturaArea.style.display = 'none';
    imgCaptura.src = '';
  });

  n.btnLimpiarLog.addEventListener('click', () => {
    logLista.replaceChildren();
    acciones = [];
  });

  return {
    raiz,
    contenedor,
    mostrar(visible: boolean) {
      raiz.style.display = visible ? '' : 'none';
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
    registrarAccion(accion: AccionNavegador) {
      anadirAlLog(accion);
    },
    limpiarLog() {
      logLista.replaceChildren();
      acciones = [];
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
