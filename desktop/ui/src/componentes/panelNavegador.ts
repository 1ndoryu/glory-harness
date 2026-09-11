/* Panel Navegador (plan 069A-1, F3). Fábrica duplicable que encapsula los
 * controles del navegador interno WebView2 child: barra URL, botones,
 * vista previa de captura y modo "seleccionar elemento" (placeholder F4).
 * El DOM vive en `panelNavegadorControles.ts` y la selección en
 * `panelNavegadorSeleccion.ts`; aquí quedan el estado, el IPC y el cableado
 * de eventos. */
import { invoke } from '@tauri-apps/api/core';
import { esEntornoTauri } from '../tauri/real';
import { crearControlesNav } from './panelNavegadorControles';
import { crearHistorialNavegador } from './panelNavegadorHistorial';
import { crearSeleccionNav } from './panelNavegadorSeleccion';
import { codigoHistorialAdelante, codigoHistorialAtras } from '../plataforma/webview';
import type { PanelNavegador, PanelNavegadorOpciones } from './panelNavegadorTipos';

export type { PanelNavegador, PanelNavegadorOpciones } from './panelNavegadorTipos';

export function montarPanelNavegador(opts: PanelNavegadorOpciones): PanelNavegador {
  const idP = opts.idPrefijo ?? 'navegador';
  // El ancho lo fija el CSS (panel-navegador, 480px): no hay grip de arrastre,
  // así que la opción se conserva en la interfaz por contrato, hoy es inerte.
  void opts.ancho;

  /* [109A-9] Sin fallos silenciosos: el panel no escribe en `console.*`; sus
   * fallos salen por `onAviso` y el orquestador los enruta al chat activo. */
  const avisar = (texto: string, detalle = ''): void => opts.onAviso(texto, detalle);

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
  /* [109A-11] Pila de URLs propia para el modo web: el iframe puede ser de otro
   * origen y entonces el padre no puede tocar su `history` (SecurityError). */
  const historial = crearHistorialNavegador();

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
    avisar,
  });

  /** [069A-2 fix] Completa el esquema si la URL no lo trae (modo web). */
  function urlConEsquema(texto: string): string {
    const t = texto.trim();
    if (!t) return '';
    if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(t)) return t;
    return `https://${t}`;
  }

  /** [069A-2 fix] Navega el iframe (solo modo web).
   * [109A-11] `registrar` deja la URL que se abandona en la pila propia: solo
   * se registra en navegaciones nuevas (Ir, `irA`), no al volver/avanzar, que
   * ya mueven las pilas en su propio paso. */
  function navegarWeb(url: string, registrar = true): void {
    if (!iframe) return;
    const destino = urlConEsquema(url);
    if (!destino) return;
    if (registrar && urlActual && urlActual !== destino) historial.registrar(urlActual);
    urlActual = destino;
    inputURL.value = destino;
    iframe.src = destino;
  }

  function navegarURL(): void {
    const url = inputURL.value.trim();
    if (!url) return;
    if (esTauri) {
      urlActual = url;
      seleccion.apagar();
      void invocarNavegadorComando('navegador_navegar', { url }).catch(() => {});
    } else {
      // [109A-11] En web no se asigna `urlActual` aquí: `navegarWeb` necesita
      // la URL abandonada para apilarla, y la asignación previa la borraría
      // (el aviso de "no hay páginas anteriores" era el síntoma).
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
        // [109A-9] La captura es del WebView2 nativo: en modo web el botón
        // queda visible, así que avisa en vez de no hacer nada en silencio.
        if (!esTauri) {
          avisar('la captura de pantalla requiere la app de escritorio (WebView2)');
          return;
        }
        const base64 = await invoke<string>('navegador_capturar');
        actualizarCaptura(base64);
      } catch (error) {
        avisar('no se pudo capturar el navegador', String(error));
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
          // [109A-11] El iframe suele ser de otro origen: `history.back()` del
          // padre lanza SecurityError y no hay forma de leer el historial
          // ajeno. Se vuelve por la pila propia, con aviso si está vacía.
          const anterior = historial.atras(urlActual);
          if (anterior === null) {
            avisar('no hay páginas anteriores en el panel');
            return;
          }
          navegarWeb(anterior, false);
        }
      } catch (error) {
        avisar('no se pudo navegar atrás', String(error));
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
          // [109A-11] Igual que Atrás: pila propia, nunca el historial ajeno.
          const siguiente = historial.adelante(urlActual);
          if (siguiente === null) {
            avisar('no hay páginas siguientes en el panel');
            return;
          }
          navegarWeb(siguiente, false);
        }
      } catch (error) {
        avisar('no se pudo navegar adelante', String(error));
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
