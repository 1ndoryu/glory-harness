// ============================================================
// Panel Navegador (plan 069A-1, F3). Fábrica duplicable que
// encapsula los controles del navegador interno WebView2 child:
// barra URL, botones de navegación, vista previa de captura,
// log de acciones del agente y anotaciones (placeholder F4).
// ============================================================

import { invoke } from '@tauri-apps/api/core';
import { icono } from './iconos';
import { el } from '../util/dom';
import type { ElementoSeleccionado } from '../dominio/tipos';

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
  /** [seleccionar] Desactiva el modo selección (si está activo) y limpia el
   * JS inyectado. Lo usa el orquestador al cerrar el navegador. */
  desactivarSeleccion(): void;
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
  /** [seleccionar] Se invoca cuando el usuario eligió un elemento de la página
   * (modo selección + hover + clic, solo Tauri). El orquestador lo entrega al
   * chat activo como badge pendiente. */
  onSeleccionar?: (elem: ElementoSeleccionado) => void;
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
  btnAtras.title = 'Atrás';
  btnAtras.appendChild(icono('flecha-izq', true));

  const btnAdelante = el('button', 'nav-btn') as HTMLButtonElement;
  btnAdelante.type = 'button';
  btnAdelante.title = 'Adelante';
  btnAdelante.appendChild(icono('flecha-der', true));

  const btnRecargar = el('button', 'nav-btn') as HTMLButtonElement;
  btnRecargar.type = 'button';
  btnRecargar.title = 'Recargar';
  btnRecargar.appendChild(icono('recargar', true));

  const btnCapturar = el('button', 'nav-btn') as HTMLButtonElement;
  btnCapturar.type = 'button';
  btnCapturar.title = 'Capturar pantalla';
  btnCapturar.appendChild(icono('camara', true));

  // [seleccionar] Botón de "seleccionar elemento": entra en un modo donde el
  // elemento bajo el cursor se resalta al hacer hover y un clic lo captura
  // para pasarlo al modelo. Solo tiene efecto real en Tauri (WebView2); en el
  // modo web la página va en un iframe cross-origin y no se puede inspeccionar.
  const btnSeleccionar = el('button', 'nav-btn') as HTMLButtonElement;
  btnSeleccionar.type = 'button';
  btnSeleccionar.title = esTauri
    ? 'Seleccionar elemento de la página'
    : 'Seleccionar elemento (requiere la app de escritorio)';
  btnSeleccionar.appendChild(icono('seleccionar', true));

  botones.appendChild(btnAtras);
  botones.appendChild(btnAdelante);
  botones.appendChild(btnRecargar);
  botones.appendChild(btnCapturar);
  botones.appendChild(btnSeleccionar);

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
  btnCerrar.title = 'Cierra la webview y oculta el panel';
  btnCerrar.appendChild(icono('x', true));
  btnCerrar.appendChild(el('span')).textContent = ' Cerrar navegador';
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
  // [seleccionar] Modo "elegir elemento" activo + temporizador de lectura.
  let seleccionando = false;
  let pollSeleccion: number | null = null;

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
      // [seleccionar] Navegar descarta el modo selección: la inyección vive
      // en el documento actual y no sobrevive al cambio de página.
      apagarSeleccion();
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
    apagarSeleccion();
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
        apagarSeleccion();
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
        apagarSeleccion();
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

  // =====================================================================
  // [seleccionar] Modo "elegir elemento de la página" (solo Tauri/WebView2).
  // Al activarlo se inyecta JS en la webview child (vía `navegador_js`, que
  // ejecuta en el contexto raíz del documento de la página, no en un iframe
  // cross-origin). El script:
  //   1. resalta con un contorno el elemento bajo el cursor (mouseover);
  //   2. al hacer clic, evita la navegación y guarda un descriptor JSON en
  //      `window.__ghSel__` (selector CSS robusto + etiqueta + texto + URL);
  //   3. expone `window.__ghSelLimpia__()` para desactivarse limpiamente.
  // El host lee el resultado con polling de `navegador_js` (ExecuteScript
  // devuelve el resultado serializado a JSON) y lo entrega como badge.
  // =====================================================================

  /** Activa el modo dentro de la página (idempotente; guard `__ghSelActivo__`). */
  const SCRIPT_SELECCION_ACTIVAR = `(() => {
    if (window.__ghSelActivo__) return 'modo seleccion ya activo';
    var limpiarPrevio = window.__ghSelLimpia__;
    if (typeof limpiarPrevio === 'function') { try { limpiarPrevio(); } catch (_e) { window.__ghSelError__ = String((_e && _e.message) || _e); } }
    var estilo = document.getElementById('gh-sel-estilo');
    if (!estilo) {
      estilo = document.createElement('style');
      estilo.id = 'gh-sel-estilo';
      estilo.textContent = '.gh-sel-resaltado{outline:2px solid #000 !important;outline-offset:-1px !important;cursor:crosshair !important;background:rgba(0,0,0,0.06) !important;} html.gh-sel-modo, html.gh-sel-modo *{cursor:crosshair !important;}';
      document.documentElement.appendChild(estilo);
    }
    document.documentElement.classList.add('gh-sel-modo');
    var actual = null;
    var selectorDe = function (el) {
      if (!el || el.nodeType !== 1) return 'body';
      if (el.id) return '#' + CSS.escape(el.id);
      var sel = el.tagName.toLowerCase();
      var clases = [];
      if (el.classList) {
        for (var i = 0; i < el.classList.length; i++) {
          var c = el.classList[i];
          if (c.indexOf('gh-sel') === 0) continue;
          clases.push(c);
          if (clases.length >= 2) break;
        }
      }
      if (clases.length) sel += '.' + clases.map(function (c) { return CSS.escape(c); }).join('.');
      if (el.parentElement) {
        var hermanos = Array.prototype.filter.call(el.parentElement.children, function (h) { return h.tagName === el.tagName; });
        if (hermanos.length > 1) {
          sel += ':nth-child(' + (Array.prototype.indexOf.call(el.parentElement.children, el) + 1) + ')';
        }
      }
      return sel;
    };
    var onMove = function (e) {
      var t = e.target;
      if (!t || t.nodeType !== 1) return;
      if (t === actual) return;
      if (actual && actual.classList) actual.classList.remove('gh-sel-resaltado');
      actual = t;
      if (actual && actual.classList) actual.classList.add('gh-sel-resaltado');
    };
    var onClick = function (e) {
      var t = e.target;
      if (!t || t.nodeType !== 1) return;
      if (e.defaultPrevented) return;
      e.preventDefault();
      e.stopPropagation();
      e.stopImmediatePropagation();
      var sel = selectorDe(t);
      var etiqueta = t.tagName.toLowerCase();
      if (t.id) etiqueta += '#' + t.id;
      if (t.classList && t.classList.length) {
        var cls = [];
        for (var j = 0; j < t.classList.length; j++) {
          var cc = t.classList[j];
          if (cc.indexOf('gh-sel') === 0) continue;
          cls.push(cc);
          if (cls.length >= 2) break;
        }
        if (cls.length) etiqueta += '.' + cls.join('.');
      }
      var texto = (t.innerText || t.textContent || '').replace(/\\s+/g, ' ').trim().slice(0, 120);
      window.__ghSel__ = JSON.stringify({
        selector: sel,
        etiqueta: etiqueta,
        texto: texto,
        pagina: location.href
      });
      var limpia = window.__ghSelLimpia__;
      if (typeof limpia === 'function') { try { limpia(); } catch (_e2) { window.__ghSelError__ = String((_e2 && _e2.message) || _e2); } }
    };
    var limpiar = function () {
      window.__ghSelActivo__ = false;
      document.documentElement.classList.remove('gh-sel-modo');
      var est = document.getElementById('gh-sel-estilo');
      if (est) est.remove();
      if (actual && actual.classList) actual.classList.remove('gh-sel-resaltado');
      document.removeEventListener('mouseover', onMove, true);
      document.removeEventListener('click', onClick, true);
      if (window.__ghSelLimpia__ === limpiar) delete window.__ghSelLimpia__;
    };
    window.__ghSelLimpia__ = limpiar;
    window.__ghSelActivo__ = true;
    document.addEventListener('mouseover', onMove, true);
    document.addEventListener('click', onClick, true);
    return 'modo seleccion activo';
  })()`;

  /** Lee el descriptor pendiente y lo limpia de la página (o devuelve ''). */
  const SCRIPT_SELECCION_LEER = `(() => {
    var s = window.__ghSel__;
    if (!s) return '';
    delete window.__ghSel__;
    try { var v = JSON.parse(s); return v && v.selector ? v : ''; } catch (_e) { return ''; }
  })()`;

  /** Limpia el modo selección dentro de la página (si sigue inyectado). */
  const SCRIPT_SELECCION_LIMPIAR = `(() => {
    var l = window.__ghSelLimpia__;
    if (typeof l === 'function') { try { l(); } catch (_e) { window.__ghSelError__ = String((_e && _e.message) || _e); } return 'limpiado'; }
    return 'sin modo activo';
  })()`;

  /** Marca el botón como activo/inactivo (estado monocromo). */
  function pintarBotonSeleccionar(): void {
    btnSeleccionar.classList.toggle('activo', seleccionando);
    btnSeleccionar.title = seleccionando
      ? 'Seleccionar: haz clic en el elemento de la página (o pulsa para cancelar)'
      : esTauri
        ? 'Seleccionar elemento de la página'
        : 'Seleccionar elemento (requiere la app de escritorio)';
  }

  /** Detiene el polling y limpia el modo en la página. */
  function apagarSeleccion(): void {
    seleccionando = false;
    if (pollSeleccion !== null) {
      window.clearInterval(pollSeleccion);
      pollSeleccion = null;
    }
    pintarBotonSeleccionar();
    if (esTauri && ventanaAbierta) {
      void invoke('navegador_js', { codigo: SCRIPT_SELECCION_LIMPIAR }).catch(() => {});
    }
  }

  /** Activa el modo selección: inyecta el script y arranca el polling. */
  function encenderSeleccion(): void {
    seleccionando = true;
    pintarBotonSeleccionar();
    void (async () => {
      try {
        await invoke('navegador_js', { codigo: SCRIPT_SELECCION_ACTIVAR });
        anadirAlLog({
          herramienta: 'seleccionar',
          descripcion: 'modo selección activo: pasa el cursor y haz clic en el elemento',
          ok: true,
          tiempo: Date.now(),
        });
        if (!seleccionando) return; // se apagó mientras inyectaba
        pollSeleccion = window.setInterval(() => {
          void leerSeleccion();
        }, 250);
      } catch (error) {
        seleccionando = false;
        pintarBotonSeleccionar();
        anadirAlLog({
          herramienta: 'seleccionar',
          descripcion: `error: ${String(error).slice(0, 120)}`,
          ok: false,
          tiempo: Date.now(),
        });
      }
    })();
  }

  /** Un tick del polling: lee el descriptor si el usuario ya hizo clic. */
  async function leerSeleccion(): Promise<void> {
    if (!seleccionando) return;
    try {
      const raw = await invoke<string>('navegador_js', { codigo: SCRIPT_SELECCION_LEER });
      if (!seleccionando) return; // se apagó mientras se leía
      // ExecuteScript devuelve el resultado serializado a JSON: un string
      // vacío llega como `""` (JSON), un objeto como su JSON directo.
      let valor: unknown = '';
      if (raw !== undefined && raw !== null && raw !== '' && raw !== '""') {
        try {
          valor = JSON.parse(raw);
        } catch {
          valor = raw;
        }
      }
      const desc = valor && typeof valor === 'object'
        ? (valor as { selector?: unknown; etiqueta?: unknown; texto?: unknown; pagina?: unknown })
        : null;
      if (desc && typeof desc.selector === 'string' && desc.selector) {
        apagarSeleccion();
        const elem = {
          selector: desc.selector,
          etiqueta: typeof desc.etiqueta === 'string' ? desc.etiqueta : desc.selector,
          texto: typeof desc.texto === 'string' ? desc.texto : '',
          pagina: typeof desc.pagina === 'string' ? desc.pagina : urlActual,
        };
        anadirAlLog({
          herramienta: 'seleccionar',
          descripcion: `elemento elegido: ${elem.etiqueta} (selector ${elem.selector})`,
          ok: true,
          tiempo: Date.now(),
        });
        opts.onSeleccionar?.(elem);
      }
    } catch (error) {
      // La webview pudo cerrarse o la página cambió: apagar sin ruido.
      apagarSeleccion();
      anadirAlLog({
        herramienta: 'seleccionar',
        descripcion: `se detuvo: ${String(error).slice(0, 120)}`,
        ok: false,
        tiempo: Date.now(),
      });
    }
  }

  /** Alterna el modo selección desde el botón. */
  function alternarSeleccion(): void {
    if (seleccionando) {
      apagarSeleccion();
      anadirAlLog({ herramienta: 'seleccionar', descripcion: 'modo selección cancelado', ok: true, tiempo: Date.now() });
      return;
    }
    if (!esTauri) {
      anadirAlLog({
        herramienta: 'seleccionar',
        descripcion: 'seleccionar elemento requiere la app de escritorio (WebView2)',
        ok: false,
        tiempo: Date.now(),
      });
      return;
    }
    if (!ventanaAbierta) {
      anadirAlLog({ herramienta: 'seleccionar', descripcion: 'navegador cerrado', ok: false, tiempo: Date.now() });
      return;
    }
    encenderSeleccion();
  }

  btnSeleccionar.addEventListener('click', alternarSeleccion);
  pintarBotonSeleccionar();

  btnCerrar.addEventListener('click', () => {
    void (async () => {
      ventanaAbierta = false;
      apagarSeleccion();
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
        apagarSeleccion();
      }
    },
    desactivarSeleccion() {
      apagarSeleccion();
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
        apagarSeleccion();
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