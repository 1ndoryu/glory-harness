/* [seleccionar] Modo "elegir elemento de la página" (solo Tauri/WebView2).
 * Al activarlo se inyecta JS en la webview child (vía `navegador_js`, que
 * ejecuta en el contexto raíz del documento de la página, no en un iframe
 * cross-origin). El script resalta con un contorno el elemento bajo el cursor
 * (mouseover); al hacer clic evita la navegación y guarda un descriptor JSON
 * en `window.__ghSel__` (selector CSS robusto + etiqueta + texto + URL), y
 * expone `window.__ghSelLimpia__()` para desactivarse limpiamente. El host lee
 * el resultado con polling de `navegador_js` (ExecuteScript devuelve el
 * resultado serializado a JSON) y lo entrega como badge. */
import { invoke } from '@tauri-apps/api/core';
import type { ElementoSeleccionado } from '../dominio/tipos';

export interface SeleccionNavDeps {
  btnSeleccionar: HTMLButtonElement;
  esTauri: boolean;
  ventanaAbierta(): boolean;
  urlActual(): string;
  onSeleccionar?: (elem: ElementoSeleccionado) => void;
  /** Registra una línea en el log del panel (la aporta la fábrica). */
  log(herramienta: string, descripcion: string, ok: boolean): void;
}

export interface SeleccionNav {
  /** Alterna el modo selección desde el botón. */
  alternar(): void;
  /** Detiene el polling y limpia el modo en la página. */
  apagar(): void;
}

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

export function crearSeleccionNav(d: SeleccionNavDeps): SeleccionNav {
  let seleccionando = false;
  let pollSeleccion: number | null = null;

  /** Marca el botón como activo/inactivo (estado monocromo). */
  function pintarBoton(): void {
    d.btnSeleccionar.classList.toggle('activo', seleccionando);
    d.btnSeleccionar.title = seleccionando
      ? 'Seleccionar: haz clic en el elemento de la página (o pulsa para cancelar)'
      : d.esTauri
        ? 'Seleccionar elemento de la página'
        : 'Seleccionar elemento (requiere la app de escritorio)';
  }

  /** Detiene el polling y limpia el modo en la página. */
  function apagar(): void {
    seleccionando = false;
    if (pollSeleccion !== null) {
      clearInterval(pollSeleccion);
      pollSeleccion = null;
    }
    pintarBoton();
    if (d.esTauri && d.ventanaAbierta()) {
      void invoke('navegador_js', { codigo: SCRIPT_SELECCION_LIMPIAR }).catch(() => {});
    }
  }

  /** Activa el modo selección: inyecta el script y arranca el polling. */
  function encender(): void {
    seleccionando = true;
    pintarBoton();
    void (async () => {
      try {
        await invoke('navegador_js', { codigo: SCRIPT_SELECCION_ACTIVAR });
        d.log('seleccionar', 'modo selección activo: pasa el cursor y haz clic en el elemento', true);
        if (!seleccionando) return; // se apagó mientras inyectaba
        pollSeleccion = setInterval(() => {
          void leer();
        }, 250);
      } catch (error) {
        seleccionando = false;
        pintarBoton();
        d.log('seleccionar', `error: ${String(error).slice(0, 120)}`, false);
      }
    })();
  }

  /** Un tick del polling: lee el descriptor si el usuario ya hizo clic. */
  async function leer(): Promise<void> {
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
      const desc =
        valor && typeof valor === 'object'
          ? (valor as { selector?: unknown; etiqueta?: unknown; texto?: unknown; pagina?: unknown })
          : null;
      if (desc && typeof desc.selector === 'string' && desc.selector) {
        apagar();
        const elem: ElementoSeleccionado = {
          selector: desc.selector,
          etiqueta: typeof desc.etiqueta === 'string' ? desc.etiqueta : desc.selector,
          texto: typeof desc.texto === 'string' ? desc.texto : '',
          pagina: typeof desc.pagina === 'string' ? desc.pagina : d.urlActual(),
        };
        d.log('seleccionar', `elemento elegido: ${elem.etiqueta} (selector ${elem.selector})`, true);
        d.onSeleccionar?.(elem);
      }
    } catch (error) {
      // La webview pudo cerrarse o la página cambió: apagar sin ruido.
      apagar();
      d.log('seleccionar', `se detuvo: ${String(error).slice(0, 120)}`, false);
    }
  }

  /** Alterna el modo selección desde el botón. */
  function alternar(): void {
    if (seleccionando) {
      apagar();
      d.log('seleccionar', 'modo selección cancelado', true);
      return;
    }
    if (!d.esTauri) {
      d.log('seleccionar', 'seleccionar elemento requiere la app de escritorio (WebView2)', false);
      return;
    }
    if (!d.ventanaAbierta()) {
      d.log('seleccionar', 'navegador cerrado', false);
      return;
    }
    encender();
  }

  d.btnSeleccionar.addEventListener('click', alternar);
  pintarBoton();

  return { alternar, apagar };
}
