/* DOM del panel Navegador: construye barra URL, botones, contenedor de vista
 * (webview child en Tauri, iframe reutilizable en modo web), área de captura,
 * log de acciones y botón de cierre. Sin estado ni IPC: solo nodos. */
import { icono } from './iconos';
import { el } from '../util/dom';

/** Todos los nodos que la fábrica necesita cablear. */
export interface NodosNav {
  raiz: HTMLElement;
  inputURL: HTMLInputElement;
  btnIr: HTMLButtonElement;
  btnAtras: HTMLButtonElement;
  btnAdelante: HTMLButtonElement;
  btnRecargar: HTMLButtonElement;
  btnCapturar: HTMLButtonElement;
  btnSeleccionar: HTMLButtonElement;
  contenedor: HTMLElement;
  /** Solo existe en modo web (iframe reutilizable entre aperturas). */
  iframe: HTMLIFrameElement | null;
  capturaArea: HTMLElement;
  imgCaptura: HTMLImageElement;
  cerrarCaptura: HTMLButtonElement;
  logLista: HTMLElement;
  btnLimpiarLog: HTMLButtonElement;
  btnCerrar: HTMLButtonElement;
}

function boton(titulo: string): HTMLButtonElement {
  const b = el('button', 'nav-btn') as HTMLButtonElement;
  b.type = 'button';
  b.title = titulo;
  return b;
}

export function crearControlesNav(idP: string, esTauri: boolean): NodosNav {
  const raiz = el('section');
  raiz.className = 'panel-navegador';
  raiz.id = `${idP}-panel`;
  raiz.style.display = 'none'; // oculto por defecto

  // Barra de URL.
  const barraURL = el('div', 'nav-url-barra');
  const inputURL = el('input') as HTMLInputElement;
  inputURL.id = `${idP}-url`;
  inputURL.type = 'text';
  inputURL.placeholder = 'https://ejemplo.com';
  inputURL.className = 'nav-url-input';
  const btnIr = boton('Navegar a la URL');
  btnIr.textContent = 'Ir';
  barraURL.appendChild(inputURL);
  barraURL.appendChild(btnIr);

  // Botones de navegación.
  const botones = el('div', 'nav-botones-barra');
  const btnAtras = boton('Atrás');
  btnAtras.appendChild(icono('flecha-izq', true));
  const btnAdelante = boton('Adelante');
  btnAdelante.appendChild(icono('flecha-der', true));
  const btnRecargar = boton('Recargar');
  btnRecargar.appendChild(icono('recargar', true));
  const btnCapturar = boton('Capturar pantalla');
  btnCapturar.appendChild(icono('camara', true));
  // [seleccionar] Botón de "seleccionar elemento": entra en un modo donde el
  // elemento bajo el cursor se resalta al hacer hover y un clic lo captura
  // para pasarlo al modelo. Solo tiene efecto real en Tauri (WebView2); en el
  // modo web la página va en un iframe cross-origin y no se puede inspeccionar.
  const btnSeleccionar = boton(
    esTauri
      ? 'Seleccionar elemento de la página'
      : 'Seleccionar elemento (requiere la app de escritorio)',
  );
  btnSeleccionar.appendChild(icono('seleccionar', true));
  botones.appendChild(btnAtras);
  botones.appendChild(btnAdelante);
  botones.appendChild(btnRecargar);
  botones.appendChild(btnCapturar);
  botones.appendChild(btnSeleccionar);

  // Contenedor de la vista: aloja la webview child (HWND) en Tauri; en web
  // aloja un iframe a pantalla completa, creado una sola vez y reutilizado.
  const contenedor = el('div', 'nav-webview');
  contenedor.id = `${idP}-webview-contenedor`;
  contenedor.title = esTauri ? 'Área del navegador (webview nativa)' : 'Área del navegador (iframe)';

  // Captura (imagen previsualizada).
  const capturaArea = el('div', 'nav-captura');
  capturaArea.id = `${idP}-captura`;
  capturaArea.style.display = 'none';
  const imgCaptura = el('img') as HTMLImageElement;
  imgCaptura.id = `${idP}-captura-img`;
  imgCaptura.alt = 'Captura del navegador';
  const cerrarCaptura = boton('Cerrar previsualización');
  cerrarCaptura.textContent = '× cerrar';
  capturaArea.appendChild(imgCaptura);
  capturaArea.appendChild(cerrarCaptura);

  // [069A-2 fix] En modo web no hay HWND que incrustar: se monta un iframe
  // dentro del contenedor. Nace en blanco; la URL la pone el usuario.
  let iframe: HTMLIFrameElement | null = null;
  if (!esTauri) {
    iframe = el('iframe') as HTMLIFrameElement;
    iframe.className = 'nav-iframe';
    iframe.src = 'about:blank';
    // Sin sandbox: muchos sitios (buscadores, youtube...) rompen con
    // restricciones; es una vista de confianza del propio usuario.
    contenedor.appendChild(iframe);
  }

  // Log de acciones del agente.
  const logArea = el('div', 'nav-log');
  const logTitulo = el('h3', 'nav-log-titulo');
  logTitulo.textContent = 'Acciones del agente';
  const logLista = el('ol', 'nav-log-lista');
  logLista.id = `${idP}-log`;
  const btnLimpiarLog = boton('Limpiar log de acciones');
  btnLimpiarLog.textContent = 'Limpiar';
  logArea.appendChild(logTitulo);
  logArea.appendChild(logLista);
  logArea.appendChild(btnLimpiarLog);

  // Botón de cerrar navegador.
  const botonCerrar = el('div', 'nav-cerrar');
  const btnCerrar = boton('Cierra la webview y oculta el panel');
  btnCerrar.appendChild(icono('x', true));
  btnCerrar.appendChild(el('span')).textContent = ' Cerrar navegador';
  botonCerrar.appendChild(btnCerrar);

  raiz.appendChild(barraURL);
  raiz.appendChild(botones);
  raiz.appendChild(contenedor);
  raiz.appendChild(capturaArea);
  raiz.appendChild(logArea);
  raiz.appendChild(botonCerrar);

  return {
    raiz,
    inputURL,
    btnIr,
    btnAtras,
    btnAdelante,
    btnRecargar,
    btnCapturar,
    btnSeleccionar,
    contenedor,
    iframe,
    capturaArea,
    imgCaptura,
    cerrarCaptura,
    logLista,
    btnLimpiarLog,
    btnCerrar,
  };
}
