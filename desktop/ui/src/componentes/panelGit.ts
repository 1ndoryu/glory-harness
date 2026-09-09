import '../estilos/git.css';
import { el } from '../util/dom';
import { icono } from './iconos';

export interface EntradaGit {
  estado: string;
  ruta: string;
}

export interface EstadoGit {
  aplicable: boolean;
  raiz: string | null;
  entradas: EntradaGit[];
  diff: string;
  truncado: boolean;
  mensaje: string | null;
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
  const cabecera = el('div', 'git-cabecera');
  const titulo = el('span', 'git-titulo');
  titulo.textContent = 'Git local';
  const recargar = el('button', 'git-accion') as HTMLButtonElement;
  recargar.type = 'button';
  recargar.title = 'Recargar estado Git';
  recargar.setAttribute('aria-label', 'Recargar estado Git');
  recargar.appendChild(icono('recargar'));
  cabecera.append(titulo, recargar);

  const lista = el('div', 'git-lista');
  lista.setAttribute('role', 'list');
  const diff = el('pre', 'git-diff');
  raiz.append(cabecera, lista, diff);

  let secuencia = 0;

  function pintar(resultado: EstadoGit): void {
    lista.replaceChildren();
    diff.textContent = resultado.diff;
    if (!resultado.aplicable) {
      diff.textContent = '';
      const vacio = el('div', 'git-vacio');
      vacio.textContent = resultado.mensaje ?? 'Git no aplicable en este workspace';
      lista.appendChild(vacio);
      return;
    }
    if (resultado.entradas.length === 0) {
      const vacio = el('div', 'git-vacio');
      vacio.textContent = 'sin cambios';
      lista.appendChild(vacio);
    }
    for (const entrada of resultado.entradas) {
      const fila = el('div', 'git-entrada');
      const codigo = el('span', 'git-codigo');
      codigo.textContent = entrada.estado;
      const ruta = el('span', 'git-ruta');
      ruta.textContent = entrada.ruta;
      fila.append(codigo, ruta);
      lista.appendChild(fila);
    }
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
      diff.textContent = '';
      opts.onError?.('no se pudo consultar Git', String(error));
    }
  }

  recargar.addEventListener('click', () => void cargar());
  return { raiz, recargar: () => void cargar() };
}
