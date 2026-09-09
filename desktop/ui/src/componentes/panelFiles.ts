// ponytail: sin watcher nativo. recargar() en cada apertura + botón manual
// cubren detección de cambios externos. Files sigue el patrón de Synara:
// un único pane con árbol a la izquierda y preview del archivo a la derecha.

import '../estilos/files.css';
import { icono } from './iconos';
import { el } from '../util/dom';
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
  const recargar = el('button', 'files-accion files-recargar') as HTMLButtonElement;
  recargar.type = 'button';
  recargar.title = 'Recargar workspace';
  recargar.setAttribute('aria-label', 'Recargar workspace');
  recargar.appendChild(icono('recargar'));
  busqueda.append(input, recargar);
  const arbol = el('div', 'files-arbol');
  arbol.setAttribute('role', 'tree');
  explorador.append(busqueda, arbol);

  const visor = el('div', 'files-visor');
  const visorCabecera = el('div', 'files-visor-cabecera');
  const visorRuta = el('span', 'files-visor-ruta');
  visorRuta.textContent = 'Selecciona un archivo para verlo';
  visorRuta.title = 'Selecciona un archivo para verlo';
  const abrirCon = el('button', 'files-accion') as HTMLButtonElement;
  abrirCon.type = 'button';
  abrirCon.title = 'Abrir archivo con…';
  abrirCon.setAttribute('aria-label', 'Abrir archivo con…');
  abrirCon.disabled = true;
  abrirCon.appendChild(icono('abrir'));
  visorCabecera.append(visorRuta, abrirCon);
  const codigo = el('div', 'files-visor-codigo');
  const vacio = el('div', 'files-visor-vacio');
  vacio.textContent = 'Selecciona un archivo del árbol para previsualizarlo.';
  codigo.appendChild(vacio);
  visor.append(visorCabecera, codigo);
  contenido.append(explorador, visor);
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

  async function abrirArchivo(ruta: string): Promise<void> {
    const id = ++secuenciaLectura;
    rutaSeleccionada = ruta;
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

  async function cargarDirectorio(ruta: string, objetivo: HTMLElement, nivel: number): Promise<void> {
    const id = ++secuencia;
    try {
      const resultado = await opts.transporte.listar(ruta, nivel === 0 ? 1 : 0);
      if (id !== secuencia || (ruta !== '' && !directoriosExpandidos.has(ruta))) return;
      pintarEntradas(resultado.entradas, objetivo, nivel);
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
      directoriosExpandidos.clear();
      await cargarDirectorio('', arbol, 0);
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

  abrirCon.addEventListener('click', () => void abrirArchivoConSeleccion());
  input.addEventListener('input', () => {
    window.clearTimeout(Number(input.dataset.timer || 0));
    input.dataset.timer = String(window.setTimeout(() => void buscar(), 220));
  });
  recargar.addEventListener('click', () => {
    directoriosExpandidos.clear();
    input.value = '';
    void cargarDirectorio('', arbol, 0);
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
      directoriosExpandidos.clear();
      void cargarDirectorio('', arbol, 0);
    },
    registrarCambio,
    sincronizarCambios,
  };
}
