// ponytail: sin watcher nativo. recargar() en cada apertura + botón manual
// cubren detección de cambios externos. Añadir `notify` (crate) + invalidación
// diferida solo si hay quejas de datos obsoletos sin recarga manual.

import '../estilos/files.css';
import { icono } from './iconos';
import { el } from '../util/dom';
import type { EntradaWorkspace, ErrorFilesystem, ListadoWorkspace, ResultadoBusqueda } from '../dominio/tipos';

export interface FilesTransport {
  listar(ruta: string, profundidad?: number): Promise<ListadoWorkspace>;
  buscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda>;
  leer(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }>;
}

export interface PanelFiles {
  raiz: HTMLElement;
  recargar(): void;
}

export function montarPanelFiles(opts: {
  transporte: FilesTransport;
  abrirArchivo(ruta: string): void;
}): PanelFiles {
  const raiz = el('div', 'panel-files');
  const cabecera = el('div', 'files-cabecera');
  const titulo = el('span', 'files-titulo');
  titulo.textContent = 'Files';
  const recargar = el('button', 'files-accion') as HTMLButtonElement;
  recargar.type = 'button';
  recargar.title = 'Recargar workspace';
  recargar.setAttribute('aria-label', 'Recargar workspace');
  recargar.appendChild(icono('recargar'));
  cabecera.append(titulo, recargar);

  const busqueda = el('div', 'files-busqueda');
  const input = el('input', 'files-input') as HTMLInputElement;
  input.type = 'search';
  input.placeholder = 'buscar archivos…';
  input.setAttribute('aria-label', 'buscar archivos por nombre');
  busqueda.appendChild(input);

  const estado = el('div', 'files-estado');
  const arbol = el('div', 'files-arbol');
  arbol.setAttribute('role', 'tree');
  raiz.append(cabecera, busqueda, estado, arbol);

  const cargadas = new Map<string, EntradaWorkspace[]>();
  let secuencia = 0;

  function errorTexto(error: unknown): string {
    if (typeof error === 'object' && error !== null && 'mensaje' in error) {
      return String((error as ErrorFilesystem).mensaje);
    }
    return String(error);
  }

  function pintarEstado(texto: string, clase = ''): void {
    estado.textContent = texto;
    estado.className = `files-estado${clase ? ` ${clase}` : ''}`;
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
      boton.setAttribute('aria-expanded', String(cargadas.has(entrada.ruta)));
      boton.addEventListener('click', () => void alternarDirectorio(entrada, fila, nivel));
    } else {
      boton.appendChild(icono('archivo'));
      boton.addEventListener('click', () => opts.abrirArchivo(entrada.ruta));
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
      if (entrada.tipo === 'directorio' && entrada.hijos) {
        const hijos = el('div', 'files-hijos');
        hijos.dataset.ruta = entrada.ruta;
        pintarEntradas(entrada.hijos, hijos, nivel + 1);
        nodo.appendChild(hijos);
      }
    }
  }

  async function cargarDirectorio(ruta: string, objetivo: HTMLElement, nivel: number): Promise<void> {
    const id = ++secuencia;
    pintarEstado('cargando…', 'cargando');
    try {
      const resultado = await opts.transporte.listar(ruta, nivel === 0 ? 1 : 0);
      if (id !== secuencia) return;
      cargadas.set(ruta, resultado.entradas);
      pintarEntradas(resultado.entradas, objetivo, nivel);
      pintarEstado(resultado.truncado ? 'lista truncada; usa búsqueda para más resultados' : `${resultado.entradas.length} entradas`);
      if (resultado.entradas.length === 0) pintarEstado('carpeta vacía', 'vacio');
    } catch (error: unknown) {
      if (id !== secuencia) return;
      objetivo.replaceChildren();
      pintarEstado(`no se pudo listar: ${errorTexto(error)}`, 'error');
    }
  }

  async function alternarDirectorio(entrada: EntradaWorkspace, filaNodo: HTMLElement, nivel: number): Promise<void> {
    const hijos = filaNodo.querySelector<HTMLElement>(':scope > .files-hijos');
    if (hijos) {
      hijos.remove();
      const boton = filaNodo.querySelector<HTMLButtonElement>('.files-nombre');
      boton?.setAttribute('aria-expanded', 'false');
      return;
    }
    const contenedor = el('div', 'files-hijos');
    contenedor.dataset.ruta = entrada.ruta;
    filaNodo.appendChild(contenedor);
    filaNodo.querySelector<HTMLButtonElement>('.files-nombre')?.setAttribute('aria-expanded', 'true');
    await cargarDirectorio(entrada.ruta, contenedor, nivel + 1);
  }

  async function buscar(): Promise<void> {
    const consulta = input.value.trim();
    if (!consulta) {
      await cargarDirectorio('', arbol, 0);
      return;
    }
    const id = ++secuencia;
    pintarEstado('buscando…', 'cargando');
    try {
      const resultado = await opts.transporte.buscar(consulta);
      if (id !== secuencia) return;
      arbol.replaceChildren();
      const entradas = resultado.entradas;
      pintarEntradas(entradas, arbol, 0);
      pintarEstado(resultado.truncado ? `${entradas.length} resultados; búsqueda truncada` : `${entradas.length} resultados`);
      if (!entradas.length) pintarEstado('sin resultados', 'vacio');
    } catch (error: unknown) {
      if (id !== secuencia) return;
      arbol.replaceChildren();
      pintarEstado(`no se pudo buscar: ${errorTexto(error)}`, 'error');
    }
  }

  input.addEventListener('input', () => {
    window.clearTimeout(Number(input.dataset.timer || 0));
    input.dataset.timer = String(window.setTimeout(() => void buscar(), 220));
  });
  recargar.addEventListener('click', () => {
    cargadas.clear();
    input.value = '';
    void cargarDirectorio('', arbol, 0);
  });

  return { raiz, recargar: () => { cargadas.clear(); void cargarDirectorio('', arbol, 0); } };
}
