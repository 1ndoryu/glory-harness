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
import {
  scriptSeleccionActivar,
  scriptSeleccionLeer,
  scriptSeleccionLimpiar,
} from '../plataforma/webview';

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

/* Los fragmentos inyectados en la página viven en `plataforma/webview.ts`
 * (son texto remoto, no código del host). */

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
      void invoke('navegador_js', { codigo: scriptSeleccionLimpiar() }).catch(() => {});
    }
  }

  /** Activa el modo selección: inyecta el script y arranca el polling. */
  function encender(): void {
    seleccionando = true;
    pintarBoton();
    void (async () => {
      try {
        await invoke('navegador_js', { codigo: scriptSeleccionActivar() });
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
      const raw = await invoke<string>('navegador_js', { codigo: scriptSeleccionLeer() });
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
