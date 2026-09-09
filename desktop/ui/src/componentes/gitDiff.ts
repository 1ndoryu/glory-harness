import { el } from '../util/dom';

export type GrupoGit = 'staged' | 'changes';

export interface ArchivoGit {
  estado: string;
  ruta: string;
  patch: string;
  adiciones: number;
  eliminaciones: number;
}

export function separarEntradas(entradas: Array<{ estado: string; ruta: string }>, diffStaged: string, diffUnstaged: string): {
  staged: ArchivoGit[];
  changes: ArchivoGit[];
} {
  const stagedPatches = indexarPatches(diffStaged);
  const unstagedPatches = indexarPatches(diffUnstaged);
  const staged: ArchivoGit[] = [];
  const changes: ArchivoGit[] = [];

  for (const entrada of entradas) {
    const indice = entrada.estado[0] ?? ' ';
    const trabajo = entrada.estado[1] ?? ' ';
    if (indice !== ' ' && indice !== '?') {
      staged.push(crearArchivo(entrada, stagedPatches.get(entrada.ruta) ?? ''));
    }
    if (trabajo !== ' ' || entrada.estado === '??') {
      changes.push(crearArchivo(entrada, unstagedPatches.get(entrada.ruta) ?? ''));
    }
  }

  return { staged, changes };
}

function crearArchivo(entrada: { estado: string; ruta: string }, patch: string): ArchivoGit {
  const estadistica = contarCambios(patch);
  return {
    estado: entrada.estado,
    ruta: entrada.ruta,
    patch,
    adiciones: estadistica.adiciones,
    eliminaciones: estadistica.eliminaciones,
  };
}

function indexarPatches(diff: string): Map<string, string> {
  const resultado = new Map<string, string>();
  if (!diff.trim()) return resultado;

  const bloques = diff.split(/^diff --git /m).slice(1);
  for (const bloque of bloques) {
    const patch = `diff --git ${bloque}`;
    const ruta = rutaDelPatch(patch);
    if (ruta) resultado.set(ruta, patch.trimEnd());
  }
  return resultado;
}

function rutaDelPatch(patch: string): string | null {
  const nueva = patch.match(/^\+\+\+ b\/(.*)$/m)?.[1];
  if (nueva && nueva !== '/dev/null') return normalizarRuta(nueva);
  const antigua = patch.match(/^--- a\/(.*)$/m)?.[1];
  return antigua && antigua !== '/dev/null' ? normalizarRuta(antigua) : null;
}

function normalizarRuta(ruta: string): string {
  return ruta.replaceAll('\\', '/').trim();
}

function contarCambios(patch: string): { adiciones: number; eliminaciones: number } {
  let adiciones = 0;
  let eliminaciones = 0;
  for (const linea of patch.split(/\r?\n/)) {
    if (linea.startsWith('+++') || linea.startsWith('---')) continue;
    if (linea.startsWith('+')) adiciones += 1;
    else if (linea.startsWith('-')) eliminaciones += 1;
  }
  return { adiciones, eliminaciones };
}

export function sumarCambios(archivos: ArchivoGit[]): { adiciones: number; eliminaciones: number } {
  return archivos.reduce(
    (total, archivo) => ({
      adiciones: total.adiciones + archivo.adiciones,
      eliminaciones: total.eliminaciones + archivo.eliminaciones,
    }),
    { adiciones: 0, eliminaciones: 0 },
  );
}

export function pintarDiff(contenedor: HTMLElement, archivo: ArchivoGit): void {
  contenedor.replaceChildren();
  contenedor.hidden = false;

  const caja = el('div', 'git-diff-caja');
  const cabecera = el('div', 'git-diff-cabecera');
  const ruta = el('span', 'git-diff-ruta');
  ruta.textContent = archivo.ruta;
  ruta.title = archivo.ruta;
  const estadistica = el('span', 'git-diff-estadistica');
  estadistica.append(
    crearEstadistica('+', archivo.adiciones, 'git-diff-adiciones'),
    crearEstadistica('−', archivo.eliminaciones, 'git-diff-eliminaciones'),
  );
  cabecera.append(ruta, estadistica);
  caja.appendChild(cabecera);

  const cuerpo = el('div', 'git-diff-cuerpo');
  if (!archivo.patch.trim()) {
    const vacio = el('div', 'git-diff-vacio');
    vacio.textContent = 'No hay diff disponible para este archivo.';
    cuerpo.appendChild(vacio);
    caja.appendChild(cuerpo);
    contenedor.appendChild(caja);
    return;
  }

  let lineaAntigua = 0;
  let lineaNueva = 0;
  for (const linea of archivo.patch.split(/\r?\n/)) {
    if (linea.startsWith('@@')) {
      const hunk = linea.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
      if (hunk) {
        lineaAntigua = Number(hunk[1]);
        lineaNueva = Number(hunk[2]);
      }
      const encabezado = el('div', 'git-diff-hunk');
      encabezado.textContent = linea;
      cuerpo.appendChild(encabezado);
      continue;
    }
    if (
      linea.startsWith('diff --git ') ||
      linea.startsWith('index ') ||
      linea.startsWith('--- ') ||
      linea.startsWith('+++ ')
    ) {
      continue;
    }
    if (linea === '\\ No newline at end of file') {
      const aviso = el('div', 'git-diff-aviso');
      aviso.textContent = linea;
      cuerpo.appendChild(aviso);
      continue;
    }

    const tipo = linea.startsWith('+') ? 'adicion' : linea.startsWith('-') ? 'eliminacion' : 'contexto';
    const fila = el('div', `git-diff-linea git-diff-${tipo}`);
    const antigua = el('span', 'git-diff-numero');
    const nueva = el('span', 'git-diff-numero');
    const marca = el('span', 'git-diff-marca');
    const texto = el('span', 'git-diff-texto');
    if (tipo === 'adicion') {
      antigua.textContent = '';
      nueva.textContent = String(lineaNueva++);
      marca.textContent = '+';
      texto.textContent = linea.slice(1);
    } else if (tipo === 'eliminacion') {
      antigua.textContent = String(lineaAntigua++);
      nueva.textContent = '';
      marca.textContent = '−';
      texto.textContent = linea.slice(1);
    } else {
      antigua.textContent = String(lineaAntigua++);
      nueva.textContent = String(lineaNueva++);
      marca.textContent = ' ';
      texto.textContent = linea.startsWith(' ') ? linea.slice(1) : linea;
    }
    fila.append(antigua, nueva, marca, texto);
    cuerpo.appendChild(fila);
  }
  caja.appendChild(cuerpo);
  contenedor.appendChild(caja);
}

function crearEstadistica(marca: string, cantidad: number, clase: string): HTMLElement {
  const nodo = el('span', clase);
  nodo.textContent = `${marca}${cantidad}`;
  return nodo;
}
