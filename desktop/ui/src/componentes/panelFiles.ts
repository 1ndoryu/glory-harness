// ponytail: sin watcher nativo. recargar() en cada apertura + botón manual
// cubren detección de cambios externos. Files sigue el patrón de Synara:
// un único pane con árbol a la izquierda y preview del archivo a la derecha.

import '../estilos/files.css';
import { icono } from './iconos';
import { el, marcarCuerpo } from '../util/dom';
import { crearBotonIcono } from './chromePanel';
import { seguirPuntero } from '../plataforma/ventana';
import type { EntradaWorkspace, ErrorFilesystem, ListadoWorkspace, ResultadoBusqueda } from '../dominio/tipos';

export interface FilesTransport {
  listar(ruta: string, profundidad?: number): Promise<ListadoWorkspace>;
  buscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda>;
  leer(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }>;
  abrirCon(ruta: string): Promise<void>;
}

export interface CambioArchivoFiles {
  origen: 'tool';
  tool: 'file_write' | 'file_patch';
  ruta: string;
  titulo: string;
  resumen: string;
  diff: string | null;
}

export interface PanelFiles {
  raiz: HTMLElement;
  recargar(): void;
  registrarCambio(cambio: CambioArchivoFiles): void;
  sincronizarCambios(cambios: CambioArchivoFiles[]): void;
  /** [129A-10 F2] Previsualiza una ruta cualquiera (vista del agente, no
   * cambio: no toca el mapa de cambios). */
  mostrarArchivo(ruta: string): void;
}

export function montarPanelFiles(opts: {
  transporte: FilesTransport;
  onError?: (texto: string, detalle?: string) => void;
}): PanelFiles {
  const raiz = el('div', 'panel-files');
  const contenido = el('div', 'files-contenido');
  const explorador = el('div', 'files-explorador');
  const busqueda = el('div', 'files-busqueda');
  const input = el('input', 'files-input') as HTMLInputElement;
  input.type = 'search';
  input.placeholder = 'buscar archivos…';
  input.setAttribute('aria-label', 'buscar archivos por nombre');
  // (219A-1) Botones con el constructor único (canon 28×28 sin borde). El
  // click se cablea más abajo (mismo sitio que antes).
  const recargar = crearBotonIcono({ icono: 'recargar', etiqueta: 'Recargar workspace' });
  busqueda.append(input, recargar);
  const arbol = el('div', 'files-arbol');
  arbol.setAttribute('role', 'tree');
  explorador.append(busqueda, arbol);

  const visor = el('div', 'files-visor');
  const visorCabecera = el('div', 'files-visor-cabecera');
  /* Botón para ocultar/mostrar la lista de archivos (mismo patrón que el
   * toggle del sidebar: iconos `panel-izq-cerrar/abrir`). `files-visor-lista`
   * es solo hook posicional (margen); el estilo lo pone `crearBotonIcono`. */
  const alternarLista = crearBotonIcono({
    icono: 'panel-izq-cerrar',
    etiqueta: 'Ocultar la lista de archivos',
    claseExtra: 'files-visor-lista',
  });
  alternarLista.setAttribute('aria-expanded', 'true');
  const visorRuta = el('span', 'files-visor-ruta');
  visorRuta.textContent = 'Selecciona un archivo para verlo';
  visorRuta.title = 'Selecciona un archivo para verlo';
  const abrirCon = crearBotonIcono({
    icono: 'abrir',
    etiqueta: 'Abrir archivo con…',
    deshabilitado: true,
  });
  visorCabecera.append(alternarLista, visorRuta, abrirCon);
  const codigo = el('div', 'files-visor-codigo');
  const vacio = el('div', 'files-visor-vacio');
  vacio.textContent = 'Selecciona un archivo del árbol para previsualizarlo.';
  codigo.appendChild(vacio);
  visor.append(visorCabecera, codigo);
  /* Divisor arrastrable entre explorador y visor (mismo patrón que el
   * grip del panel derecho: `seguirPuntero` + cursor global). El
   * explorador está anclado al borde izquierdo: su ancho es la
   * distancia del cursor hasta ese borde. */
  const ANCHO_MIN_EXPLORADOR = 150;
  const FRACCION_MAX_EXPLORADOR = 0.6;
  const grip = el('div', 'files-grip');
  grip.setAttribute('aria-hidden', 'true');
  function acotarAnchoExplorador(px: number): number {
    const max = Math.round(contenido.getBoundingClientRect().width * FRACCION_MAX_EXPLORADOR);
    return Math.min(max, Math.max(ANCHO_MIN_EXPLORADOR, Math.round(px)));
  }
  let arrastrandoGrip = false;
  grip.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrandoGrip = true;
    marcarCuerpo('redimensionando-lateral', true);
  });
  seguirPuntero(
    (e) => {
      if (!arrastrandoGrip) return;
      const rect = contenido.getBoundingClientRect();
      const px = acotarAnchoExplorador(e.clientX - rect.left);
      explorador.style.flex = `0 0 ${px}px`;
    },
    () => {
      if (!arrastrandoGrip) return;
      arrastrandoGrip = false;
      marcarCuerpo('redimensionando-lateral', false);
    },
  );
  contenido.append(explorador, grip, visor);
  raiz.appendChild(contenido);

  const directoriosExpandidos = new Set<string>();
  const cambios = new Map<string, CambioArchivoFiles>();
  let rutaSeleccionada: string | null = null;
  let secuencia = 0;
  let secuenciaLectura = 0;

  function errorTexto(error: unknown): string {
    if (typeof error === 'object' && error !== null && 'mensaje' in error) {
      return String((error as ErrorFilesystem).mensaje);
    }
    return String(error);
  }

  function notificarError(texto: string, error: unknown): void {
    opts.onError?.(texto, errorTexto(error));
  }

  function pintarCodigo(contenidoArchivo: string): void {
    codigo.replaceChildren();
    const lineas = contenidoArchivo.split('\n');
    if (lineas.length > 2000) {
      const pre = el('pre', 'files-visor-plano');
      pre.textContent = contenidoArchivo;
      codigo.appendChild(pre);
      return;
    }
    const frag = document.createDocumentFragment();
    lineas.forEach((texto, i) => {
      const fila = el('div', 'files-visor-linea');
      const numero = el('span', 'files-visor-numero');
      numero.textContent = String(i + 1);
      const textoNodo = el('span', 'files-visor-texto');
      textoNodo.textContent = texto === '' ? ' ' : texto;
      fila.append(numero, textoNodo);
      frag.appendChild(fila);
    });
    codigo.appendChild(frag);
  }

  /* Compara rutas para localizar filas: separadores unificados y sin
   * importar mayúsculas (el agente puede escribir `test/x` y el workspace
   * tener `Test/`; en Windows son el mismo archivo). Para listar/leer se
   * usa siempre la ruta canónica de la fila (`dataset.ruta`), no esta. */
  function normalizarRuta(ruta: string): string {
    return ruta.replace(/\\/g, '/').toLowerCase();
  }

  function botonPara(ruta: string): HTMLButtonElement | null {
    const objetivo = normalizarRuta(ruta);
    const botones = arbol.querySelectorAll<HTMLButtonElement>('.files-nombre[data-ruta]');
    for (const boton of botones) {
      if (normalizarRuta(boton.dataset.ruta || '') === objetivo) return boton;
    }
    return null;
  }

  /** Marca en el árbol el archivo abierto en el visor (misma marca
   * `seleccionado` que las filas del panel Git). */
  function marcarSeleccionado(ruta: string): void {
    arbol.querySelectorAll('.files-nombre.seleccionado').forEach((nodo) => {
      nodo.classList.remove('seleccionado');
      nodo.closest('.files-entrada')?.removeAttribute('aria-selected');
    });
    const boton = botonPara(ruta);
    if (!boton) return;
    boton.classList.add('seleccionado');
    boton.closest('.files-entrada')?.setAttribute('aria-selected', 'true');
    boton.scrollIntoView({ block: 'nearest' });
  }

  async function expandirDirectorio(rutaDir: string, filaNodo: HTMLElement): Promise<void> {
    const nivel = Number(filaNodo.style.getPropertyValue('--files-nivel') || 0);
    directoriosExpandidos.add(rutaDir);
    const contenedor = el('div', 'files-hijos');
    contenedor.dataset.ruta = rutaDir;
    filaNodo.appendChild(contenedor);
    filaNodo.querySelector<HTMLButtonElement>(':scope > .files-nombre')?.setAttribute('aria-expanded', 'true');
    await cargarDirectorio(rutaDir, contenedor, nivel + 1);
  }

  /** Revela una ruta en el árbol: espera a la raíz (puede estar
   * recargándose en el mismo tick), expande sus carpetas padre de arriba
   * abajo y marca el archivo. Si otro archivo tomó el visor mientras
   * tanto, no pisa su marca. */
  async function revelarEnArbol(ruta: string): Promise<void> {
    await raizLista;
    if (rutaSeleccionada !== ruta) return;
    const partes = normalizarRuta(ruta).split('/').filter((p) => p.length > 0);
    let acumulado = '';
    for (let i = 0; i < partes.length - 1; i++) {
      acumulado = acumulado ? `${acumulado}/${partes[i]}` : partes[i];
      const boton = botonPara(acumulado);
      const filaNodo = boton?.closest<HTMLElement>('.files-entrada');
      if (!boton || !filaNodo) break;
      if (!filaNodo.querySelector(':scope > .files-hijos')) {
        await expandirDirectorio(boton.dataset.ruta || acumulado, filaNodo);
      }
      if (rutaSeleccionada !== ruta) return;
    }
    if (rutaSeleccionada === ruta) marcarSeleccionado(ruta);
  }

  async function abrirArchivo(ruta: string): Promise<void> {
    const id = ++secuenciaLectura;
    rutaSeleccionada = ruta;
    void revelarEnArbol(ruta);
    abrirCon.disabled = false;
    visorRuta.textContent = ruta;
    visorRuta.title = ruta;
    codigo.replaceChildren();
    const cargando = el('div', 'files-visor-vacio');
    cargando.textContent = 'cargando…';
    codigo.appendChild(cargando);
    try {
      const resultado = await opts.transporte.leer(ruta);
      if (id !== secuenciaLectura) return;
      visorRuta.textContent = `${resultado.ruta} — ${resultado.lineas} líneas`;
      visorRuta.title = resultado.ruta;
      pintarCodigo(resultado.contenido);
    } catch (error: unknown) {
      if (id !== secuenciaLectura) return;
      codigo.replaceChildren();
      const vacioError = el('div', 'files-visor-vacio');
      vacioError.textContent = 'No se pudo previsualizar el archivo.';
      codigo.appendChild(vacioError);
      notificarError('no se pudo leer el archivo', error);
    }
  }

  async function abrirArchivoConSeleccion(): Promise<void> {
    if (!rutaSeleccionada) return;
    try {
      await opts.transporte.abrirCon(rutaSeleccionada);
    } catch (error: unknown) {
      notificarError('no se pudo abrir el archivo', error);
    }
  }

  function fila(entrada: EntradaWorkspace, nivel: number): HTMLElement {
    const fila = el('div', `files-entrada files-${entrada.tipo}`);
    fila.style.setProperty('--files-nivel', String(nivel));
    fila.setAttribute('role', 'treeitem');
    fila.title = entrada.ruta || entrada.nombre;
    const boton = el('button', 'files-nombre') as HTMLButtonElement;
    boton.type = 'button';
    boton.dataset.ruta = entrada.ruta;
    if (entrada.tipo === 'directorio') {
      boton.appendChild(icono('carpeta'));
      boton.setAttribute('aria-expanded', String(directoriosExpandidos.has(entrada.ruta)));
      boton.addEventListener('click', () => void alternarDirectorio(entrada, fila, nivel));
    } else {
      boton.appendChild(icono('archivo'));
      boton.addEventListener('click', () => void abrirArchivo(entrada.ruta));
    }
    const texto = el('span', 'files-nombre-texto');
    texto.textContent = entrada.nombre;
    boton.appendChild(texto);
    fila.appendChild(boton);
    if (entrada.ignorado) {
      const marca = el('span', 'files-ignorado');
      marca.textContent = 'excluido';
      fila.appendChild(marca);
    }
    return fila;
  }

  function pintarEntradas(entradas: EntradaWorkspace[], contenedor: HTMLElement, nivel: number): void {
    contenedor.replaceChildren();
    for (const entrada of entradas) {
      const nodo = fila(entrada, nivel);
      contenedor.appendChild(nodo);
    }
    if (entradas.length === 0) {
      const vacioArbol = el('div', 'files-vacio');
      vacioArbol.textContent = 'carpeta vacía';
      contenedor.appendChild(vacioArbol);
    }
  }

  /* Promesa de la última carga de la raíz: el revelado la espera porque
   * `abrirFilesEn` recarga el árbol y revela en el mismo tick (el árbol
   * aún está vacío o con contenido viejo cuando llega la ruta). */
  let raizLista: Promise<void> = Promise.resolve();

  function cargarRaiz(): void {
    directoriosExpandidos.clear();
    raizLista = cargarDirectorio('', arbol, 0);
  }

  async function cargarDirectorio(ruta: string, objetivo: HTMLElement, nivel: number): Promise<void> {
    const id = ++secuencia;
    try {
      const resultado = await opts.transporte.listar(ruta, nivel === 0 ? 1 : 0);
      if (id !== secuencia || (ruta !== '' && !directoriosExpandidos.has(ruta))) return;
      pintarEntradas(resultado.entradas, objetivo, nivel);
      /* El árbol se repinta: si hay un archivo abierto en el visor, la
       * raíz re-revela (re-expande padres + marca); una expansión solo
       * re-marca si su fila ya es visible. */
      if (rutaSeleccionada) {
        if (ruta === '' && nivel === 0) void revelarEnArbol(rutaSeleccionada);
        else marcarSeleccionado(rutaSeleccionada);
      }
    } catch (error: unknown) {
      if (id !== secuencia) return;
      objetivo.replaceChildren();
      notificarError('no se pudo listar el workspace', error);
    }
  }

  async function alternarDirectorio(entrada: EntradaWorkspace, filaNodo: HTMLElement, nivel: number): Promise<void> {
    const hijos = filaNodo.querySelector<HTMLElement>(':scope > .files-hijos');
    if (hijos) {
      hijos.remove();
      directoriosExpandidos.delete(entrada.ruta);
      secuencia += 1;
      filaNodo.querySelector<HTMLButtonElement>('.files-nombre')?.setAttribute('aria-expanded', 'false');
      return;
    }
    directoriosExpandidos.add(entrada.ruta);
    const contenedor = el('div', 'files-hijos');
    contenedor.dataset.ruta = entrada.ruta;
    filaNodo.appendChild(contenedor);
    filaNodo.querySelector<HTMLButtonElement>('.files-nombre')?.setAttribute('aria-expanded', 'true');
    await cargarDirectorio(entrada.ruta, contenedor, nivel + 1);
  }

  async function buscar(): Promise<void> {
    const consulta = input.value.trim();
    if (!consulta) {
      cargarRaiz();
      await raizLista;
      return;
    }
    const id = ++secuencia;
    try {
      const resultado = await opts.transporte.buscar(consulta);
      if (id !== secuencia) return;
      arbol.replaceChildren();
      pintarEntradas(resultado.entradas, arbol, 0);
    } catch (error: unknown) {
      if (id !== secuencia) return;
      arbol.replaceChildren();
      notificarError('no se pudo buscar en el workspace', error);
    }
  }

  function pintarBotonLista(visible: boolean): void {
    alternarLista.replaceChildren(icono(visible ? 'panel-izq-cerrar' : 'panel-izq-abrir'));
    const etiqueta = visible ? 'ocultar lista de archivos' : 'mostrar lista de archivos';
    alternarLista.title = etiqueta;
    alternarLista.setAttribute('aria-label', etiqueta);
    alternarLista.setAttribute('aria-expanded', String(visible));
  }

  pintarBotonLista(true);
  alternarLista.addEventListener('click', () => {
    /* `toggle` devuelve si la clase QUEDÓ: oculta = lista escondida. */
    const oculta = raiz.classList.toggle('files-sin-explorador');
    pintarBotonLista(!oculta);
  });

  abrirCon.addEventListener('click', () => void abrirArchivoConSeleccion());
  input.addEventListener('input', () => {
    window.clearTimeout(Number(input.dataset.timer || 0));
    input.dataset.timer = String(window.setTimeout(() => void buscar(), 220));
  });
  recargar.addEventListener('click', () => {
    input.value = '';
    cargarRaiz();
  });

  function registrarCambio(cambio: CambioArchivoFiles): void {
    cambios.set(cambio.ruta, cambio);
    if (cambio.ruta) void abrirArchivo(cambio.ruta);
  }

  function sincronizarCambios(anteriores: CambioArchivoFiles[]): void {
    cambios.clear();
    anteriores.forEach((cambio) => cambios.set(cambio.ruta, cambio));
  }

  return {
    raiz,
    recargar: () => {
      cargarRaiz();
    },
    registrarCambio,
    sincronizarCambios,
    mostrarArchivo: (ruta) => {
      if (ruta) void abrirArchivo(ruta);
    },
  };
}
