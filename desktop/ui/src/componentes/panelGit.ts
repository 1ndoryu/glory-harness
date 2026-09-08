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

export function montarPanelGit(opts: { transporte: GitTransport }): PanelGit {
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

  const estado = el('div', 'git-estado');
  const lista = el('div', 'git-lista');
  lista.setAttribute('role', 'list');
  const diff = el('pre', 'git-diff');
  raiz.append(cabecera, estado, lista, diff);

  let secuencia = 0;

  function pintarEstado(texto: string, clase = ''): void {
    estado.textContent = texto;
    estado.className = `git-estado${clase ? ` ${clase}` : ''}`;
  }

  function pintar(resultado: EstadoGit): void {
    lista.replaceChildren();
    diff.textContent = resultado.diff;
    if (!resultado.aplicable) {
      pintarEstado(resultado.mensaje ?? 'Git no aplicable', 'vacio');
      diff.textContent = '';
      return;
    }
    for (const entrada of resultado.entradas) {
      const fila = el('div', 'git-entrada');
      fila.setAttribute('role', 'listitem');
      const codigo = el('span', 'git-codigo');
      codigo.textContent = entrada.estado;
      const ruta = el('span', 'git-ruta');
      ruta.textContent = entrada.ruta;
      fila.append(codigo, ruta);
      lista.appendChild(fila);
    }
    const cambios = resultado.entradas.length === 1 ? 'cambio' : 'cambios';
    pintarEstado(
      resultado.truncado
        ? `${resultado.entradas.length} ${cambios}; salida truncada`
        : `${resultado.entradas.length} ${cambios}`,
      resultado.entradas.length === 0 ? 'vacio' : '',
    );
  }

  async function cargar(): Promise<void> {
    const id = ++secuencia;
    pintarEstado('consultando Git…', 'cargando');
    try {
      const resultado = await opts.transporte.estado();
      if (id !== secuencia) return;
      pintar(resultado);
    } catch (error: unknown) {
      if (id !== secuencia) return;
      lista.replaceChildren();
      diff.textContent = '';
      pintarEstado(`no se pudo consultar Git: ${String(error)}`, 'error');
    }
  }

  recargar.addEventListener('click', () => void cargar());
  return { raiz, recargar: () => void cargar() };
}
