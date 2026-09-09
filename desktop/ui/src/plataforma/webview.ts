// Fragmentos JS que se ejecutan DENTRO de la webview hija (vía CDP
// `Runtime.evaluate` / comando `navegador_js`). Son texto remoto, no código
// del host: viven centralizados aquí (boundary `window` de Sentinel) para
// que los componentes no contengan literales `window.*`/`document.*` que
// solo existen en el documento de la página visitada.

/** Vuelve atrás en el historial de la página (CDP `navegador_js`). */
export function codigoHistorialAtras(): string {
  return 'window.history.back()';
}

/** Avanza en el historial de la página (CDP `navegador_js`). */
export function codigoHistorialAdelante(): string {
  return 'window.history.forward()';
}

/** Resalta un elemento con outline rojo durante `duracionMs` y lo quita. */
export function codigoResaltar(selector: string, duracionMs: number): string {
  return `(function(){
        var e=document.querySelector(${JSON.stringify(selector)});
        if(!e)return;
        e.style.outline='2px solid red';
        e.style.outlineOffset='-1px';
        setTimeout(function(){e.style.outline='';e.style.outlineOffset=''},${duracionMs});
      })()`;
}

/** Activa el modo "elegir elemento" dentro de la página (idempotente). */
export function scriptSeleccionActivar(): string {
  return `(() => {
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
}

/** Lee el descriptor pendiente del modo selección y lo limpia (o ''). */
export function scriptSeleccionLeer(): string {
  return `(() => {
  var s = window.__ghSel__;
  if (!s) return '';
  delete window.__ghSel__;
  try { var v = JSON.parse(s); return v && v.selector ? v : ''; } catch (_e) { return ''; }
})()`;
}

/** Desactiva el modo selección dentro de la página (si sigue inyectado). */
export function scriptSeleccionLimpiar(): string {
  return `(() => {
  var l = window.__ghSelLimpia__;
  if (typeof l === 'function') { try { l(); } catch (_e) { window.__ghSelError__ = String((_e && _e.message) || _e); } return 'limpiado'; }
  return 'sin modo activo';
})()`;
}
