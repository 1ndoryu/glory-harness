import '../estilos/git.css';
import { el } from '../util/dom';
import {
  pintarDiff,
  separarEntradas,
  sumarCambios,
  type ArchivoGit,
  type GrupoGit,
} from './gitDiff';

export interface EntradaGit {
  estado: string;
  ruta: string;
}

export interface EstadoGit {
  aplicable: boolean;
  raiz: string | null;
  entradas: EntradaGit[];
  /** Compatibilidad con el endpoint anterior: diff unstaged combinado. */
  diff: string;
  truncado: boolean;
  mensaje: string | null;
  diff_unstaged?: string | null;
  diff_staged?: string | null;
}

export interface GitTransport {
  estado(): Promise<EstadoGit>;
}

export interface PanelGit {
  raiz: HTMLElement;
  recargar(): void;
}

export function montarPanelGit(opts: {
  transporte: GitTransport;
  onError?: (texto: string, detalle?: string) => void;
}): PanelGit {
  const raiz = el('div', 'panel-git');
  const recargar = () => void cargar();

  const contenido = el('div', 'git-contenido');
  const lista = el('div', 'git-lista');
  lista.setAttribute('role', 'list');
  const diff = el('div', 'git-diff');
  diff.hidden = true;
  contenido.append(lista, diff);
  raiz.append(contenido);

  let secuencia = 0;
  let seleccion: { grupo: GrupoGit; ruta: string } | null = null;

  function pintar(resultado: EstadoGit): void {
    lista.replaceChildren();
    diff.replaceChildren();
    diff.hidden = true;

    if (!resultado.aplicable) {
      const vacio = el('div', 'git-vacio');
      vacio.textContent = resultado.mensaje ?? 'Git no aplicable en este workspace';
      lista.appendChild(vacio);
      return;
    }

    const datos = separarEntradas(
      resultado.entradas,
      resultado.diff_staged ?? '',
      resultado.diff_unstaged ?? resultado.diff ?? '',
    );
    const seleccionAnterior = seleccion;
    const archivoAnterior = seleccionAnterior
      ? datos[seleccionAnterior.grupo].find((archivo) => archivo.ruta === seleccionAnterior.ruta)
      : undefined;

    /* Sin cambios no se pinta estado vacío: el panel queda limpio como en
       Synara. Las secciones solo aparecen cuando contienen archivos. */
    if (datos.staged.length === 0 && datos.changes.length === 0) return;

    if (datos.staged.length > 0) {
      lista.appendChild(crearSeccion('Staged', 'staged', datos.staged));
    }
    if (datos.changes.length > 0) {
      lista.appendChild(crearSeccion('Changes', 'changes', datos.changes));
    }

    if (archivoAnterior && seleccionAnterior) {
      pintarSeleccion(archivoAnterior, seleccionAnterior.grupo);
    } else {
      seleccion = null;
    }
  }

  function crearSeccion(titulo: string, grupo: GrupoGit, archivos: ArchivoGit[]): HTMLElement {
    const seccion = el('section', `git-seccion git-seccion-${grupo}`);
    const cabeceraSeccion = el('div', 'git-seccion-cabecera');
    const nombre = el('span', 'git-seccion-titulo');
    nombre.textContent = titulo;
    const contador = el('span', 'git-seccion-contador');
    contador.textContent = String(archivos.length);
    const estadistica = sumarCambios(archivos);
    const stat = el('span', 'git-seccion-estadistica');
    stat.append(
      crearStat('+', estadistica.adiciones, 'git-adiciones'),
      crearStat('−', estadistica.eliminaciones, 'git-eliminaciones'),
    );
    cabeceraSeccion.append(nombre, contador, stat);
    seccion.appendChild(cabeceraSeccion);

    if (archivos.length === 0) {
      const vacio = el('div', 'git-seccion-vacia');
      vacio.textContent = grupo === 'staged' ? 'sin cambios preparados' : 'sin cambios';
      seccion.appendChild(vacio);
      return seccion;
    }

    const filas = el('div', 'git-seccion-lista');
    for (const archivo of archivos) {
      const fila = el('button', 'git-entrada') as HTMLButtonElement;
      fila.type = 'button';
      fila.setAttribute('role', 'listitem');
      fila.dataset.grupo = grupo;
      fila.dataset.ruta = archivo.ruta;
      fila.classList.toggle('seleccionada', seleccion?.grupo === grupo && seleccion.ruta === archivo.ruta);
      fila.title = archivo.ruta;
      const codigo = el('span', 'git-codigo');
      codigo.textContent = estadoVisible(archivo.estado, grupo);
      const ruta = el('span', 'git-ruta');
      ruta.textContent = archivo.ruta;
      const statArchivo = el('span', 'git-entrada-estadistica');
      statArchivo.append(
        crearStat('+', archivo.adiciones, 'git-adiciones'),
        crearStat('−', archivo.eliminaciones, 'git-eliminaciones'),
      );
      fila.append(codigo, ruta, statArchivo);
      fila.addEventListener('click', () => pintarSeleccion(archivo, grupo));
      filas.appendChild(fila);
    }
    seccion.appendChild(filas);
    return seccion;
  }

  function pintarSeleccion(archivo: ArchivoGit, grupo: GrupoGit): void {
    seleccion = { grupo, ruta: archivo.ruta };
    pintarDiff(diff, archivo);
    diff.hidden = false;
    lista.querySelectorAll<HTMLButtonElement>('.git-entrada').forEach((fila) => {
      fila.classList.toggle(
        'seleccionada',
        fila.dataset.grupo === grupo && fila.dataset.ruta === archivo.ruta,
      );
    });
  }

  async function cargar(): Promise<void> {
    const id = ++secuencia;
    try {
      const resultado = await opts.transporte.estado();
      if (id !== secuencia) return;
      pintar(resultado);
    } catch (error: unknown) {
      if (id !== secuencia) return;
      lista.replaceChildren();
      diff.replaceChildren();
      diff.hidden = true;
      opts.onError?.('no se pudo consultar Git', String(error));
    }
  }

  return { raiz, recargar };
}

function crearStat(marca: string, cantidad: number, clase: string): HTMLElement {
  const nodo = el('span', clase);
  nodo.textContent = `${marca}${cantidad}`;
  return nodo;
}

function estadoVisible(estado: string, grupo: GrupoGit): string {
  const indice = estado[0] ?? ' ';
  const trabajo = estado[1] ?? ' ';
  if (estado === '??') return '?';
  return grupo === 'staged' ? indice : trabajo;
}
