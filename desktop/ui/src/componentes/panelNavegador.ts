// ============================================================
// Panel Navegador (plan 069A-1, F3). Fábrica duplicable que
// encapsula los controles del navegador interno WebView2 child:
// barra URL, botones de navegación, vista previa de captura,
// log de acciones del agente y anotaciones (placeholder F4).
// ============================================================

import { invoke } from '@tauri-apps/api/core';
import { el } from '../util/dom';

// ---------- Tipos ----------

/** Evento de acción del agente en el navegador (para el log). */
export interface AccionNavegador {
  herramienta: string;
  descripcion: string;
  ok: boolean;
  tiempo: number;
}

/** API pública que el orquestador (main.ts) usa para conectar el panel. */
export interface PanelNavegador {
  raiz: HTMLElement;
  /** Contenedor donde se monta la webview child (HWND). */
  contenedor: HTMLElement;
  /** Abre/cierra el panel (display). */
  mostrar(visible: boolean): void;
  /** Registra una acción en el log. */
  registrarAccion(accion: AccionNavegador): void;
  /** Limpia el log de acciones. */
  limpiarLog(): void;
  /** Establece la URL actual en la barra (tras navegar). */
  fijarURL(url: string): void;
  /** [069A-2 fix] Navega a una URL: IPC en Tauri, iframe en modo web. */
  irA(url: string): void;
  /** [069A-1 F6] Muestra una captura base64 del navegador. */
  actualizarCaptura(base64: string): void;
}

export interface PanelNavegadorOpciones {
  /** Prefijo de ids DOM. */
  idPrefijo?: string;
  /** Ancho inicial del panel (px). Default 480. */
  ancho?: number;
  /** [069A-2 fix] Se invoca al cerrar desde el botón interno para que el
   * orquestador sincronice estado y grip. Opcional: sin él el panel se cierra
   * igualmente (y en Tauri cierra la webview él mismo). */
  onCerrar?: () => void;
}

// ---------- Constantes ----------
const MAX_LOG = 200;

// ---------- Fábrica ----------

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
  const esTauri =
    typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== 'undefined';

  // ---- Raíz del panel ----
  const raiz = el('section');
  raiz.className = 'panel-navegador';
  raiz.id = `${idP}-panel`;
  raiz.style.display = 'none'; // oculto por defecto

  // ---- Barra de URL ----
  const barraURL = el('div', 'nav-url-barra');

  const inputURL = el('input') as HTMLInputElement;
  inputURL.id = `${idP}-url`;
  inputURL.type = 'text';
  inputURL.placeholder = 'https://ejemplo.com';
  inputURL.className = 'nav-url-input';

  const btnIr = el('button', 'nav-btn') as HTMLButtonElement;
  btnIr.type = 'button';
  btnIr.textContent = 'Ir';
  btnIr.title = 'Navegar a la URL';

  barraURL.appendChild(inputURL);
  barraURL.appendChild(btnIr);

  // ---- Botones de navegación ----
  const botones = el('div', 'nav-botones-barra');

  const btnAtras = el('button', 'nav-btn') as HTMLButtonElement;
  btnAtras.type = 'button';
  btnAtras.textContent = '←';
  btnAtras.title = 'Atrás';

  const btnAdelante = el('button', 'nav-btn') as HTMLButtonElement;
  btnAdelante.type = 'button';
  btnAdelante.textContent = '→';
  btnAdelante.title = 'Adelante';

  const btnRecargar = el('button', 'nav-btn') as HTMLButtonElement;
  btnRecargar.type = 'button';
  btnRecargar.textContent = '↻';
  btnRecargar.title = 'Recargar';

  const btnCapturar = el('button', 'nav-btn') as HTMLButtonElement;
  btnCapturar.type = 'button';
  btnCapturar.textContent = '📷';
  btnCapturar.title = 'Capturar pantalla';

  botones.appendChild(btnAtras);
  botones.appendChild(btnAdelante);
  botones.appendChild(btnRecargar);
  botones.appendChild(btnCapturar);

  // ---- Contenedor de la vista del navegador ----
  // Tauri: aloja la webview child (HWND). Web: aloja un iframe a pantalla
  // completa, creado una sola vez y reutilizado entre aperturas.
  const contenedor = el('div', 'nav-webview');
  contenedor.id = `${idP}-webview-contenedor`;
  contenedor.title = esTauri
    ? 'Área del navegador (webview nativa)'
    : 'Área del navegador (iframe)';

  // ---- Captura (imagen previsualizada) ----
  const capturaArea = el('div', 'nav-captura');
  capturaArea.id = `${idP}-captura`;
  capturaArea.style.display = 'none';

  const imgCaptura = el('img') as HTMLImageElement;
  imgCaptura.id = `${idP}-captura-img`;
  imgCaptura.alt = 'Captura del navegador';

  const cerrarCaptura = el('button', 'nav-btn') as HTMLButtonElement;
  cerrarCaptura.type = 'button';
  cerrarCaptura.textContent = '× cerrar';
  cerrarCaptura.title = 'Cerrar previsualización';

  capturaArea.appendChild(imgCaptura);
  capturaArea.appendChild(cerrarCaptura);

  // [069A-2 fix] En modo web no hay HWND que incrustar: se monta un iframe
  // dentro del contenedor. Nace en blanco; la URL la pone el usuario.
  let iframe: HTMLIFrameElement | null = null;
  if (!esTauri) {
    iframe = document.createElement('iframe');
    iframe.className = 'nav-iframe';
    iframe.src = 'about:blank';
    // Sin sandbox: muchos sitios (buscadores, youtube...) rompen con
    // restricciones; es una vista de confianza del propio usuario.
    contenedor.appendChild(iframe);
  }

  // ---- Log de acciones del agente ----
  const logArea = el('div', 'nav-log');
  const logTitulo = el('h3', 'nav-log-titulo');
  logTitulo.textContent = 'Acciones del agente';

  const logLista = el('ol', 'nav-log-lista');
  logLista.id = `${idP}-log`;

  const btnLimpiarLog = el('button', 'nav-btn') as HTMLButtonElement;
  btnLimpiarLog.type = 'button';
  btnLimpiarLog.textContent = 'Limpiar';
  btnLimpiarLog.title = 'Limpiar log de acciones';

  logArea.appendChild(logTitulo);
  logArea.appendChild(logLista);
  logArea.appendChild(btnLimpiarLog);

  // ---- Botón de cerrar navegador ----
  const botonCerrar = el('div', 'nav-cerrar');
  const btnCerrar = el('button', 'nav-btn') as HTMLButtonElement;
  btnCerrar.type = 'button';
  btnCerrar.textContent = '× Cerrar navegador';
  btnCerrar.title = 'Cierra la webview y oculta el panel';
  botonCerrar.appendChild(btnCerrar);

  // ---- Montar todo ----
  raiz.appendChild(barraURL);
  raiz.appendChild(botones);
  raiz.appendChild(contenedor);
  raiz.appendChild(capturaArea);
  raiz.appendChild(logArea);
  raiz.appendChild(botonCerrar);

  // ---- Estado interno ----
  let urlActual = '';
  let acciones: AccionNavegador[] = [];
  let ventanaAbierta = false;

  // ---- Helpers ----
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

  // ---- Eventos de UI ----

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
      void invocarNavegadorComando('navegador_navegar', { url }).catch(() => {});
    } else {
      navegarWeb(url);
    }
  }

  btnIr.addEventListener('click', navegarURL);
  inputURL.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') navegarURL();
  });

  btnCapturar.addEventListener('click', () => {
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

  btnRecargar.addEventListener('click', () => {
    if (!urlActual) return;
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

  btnAtras.addEventListener('click', () => {
    void (async () => {
      try {
        if (esTauri) {
          // CDP: Runtime.evaluate con history.back()
          await invoke('navegador_js', { codigo: 'window.history.back()' });
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

  btnAdelante.addEventListener('click', () => {
    void (async () => {
      try {
        if (esTauri) {
          await invoke('navegador_js', { codigo: 'window.history.forward()' });
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

  btnCerrar.addEventListener('click', () => {
    void (async () => {
      ventanaAbierta = false;
      raiz.style.display = 'none';
      if (esTauri) {
        // Sin orquestador, esta fábrica cierra la webview ella misma; con
        // onCerrar lo hace el orquestador (estado + IPC) para no duplicar.
        if (!opts.onCerrar) {
          try {
            await invoke('navegador_cerrar');
          } catch { /* ignorar */ }
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

  cerrarCaptura.addEventListener('click', () => {
    capturaArea.style.display = 'none';
    imgCaptura.src = '';
  });

  btnLimpiarLog.addEventListener('click', () => {
    logLista.replaceChildren();
    acciones = [];
  });

  // ---- API pública ----
  return {
    raiz,
    contenedor,
    mostrar(visible: boolean) {
      raiz.style.display = visible ? '' : 'none';
      if (visible) {
        ventanaAbierta = true;
      } else {
        ventanaAbierta = false;
      }
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