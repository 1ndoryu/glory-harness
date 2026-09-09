// Visor de archivo y cambios (089A-2): vive como tab del panel
// derecho. Pestaña Archivo: contenido real del workspace vía
// comando `leer_archivo` (Tauri). Pestaña Cambios: diffs de las
// tools file_write/file_patch de la conversación (los registra el
// orquestador con `registrarCambio`). Sin backend disponible los
// errores se muestran en el propio visor, nunca en silencio.

import '../estilos/visor.css';
import { el } from '../util/dom';

export interface CambioArchivo {
  origen: 'tool';
  tool: 'file_write' | 'file_patch';
  ruta: string;
  titulo: string;
  resumen: string;
  diff: string | null;
}

export interface PanelVisor {
  raiz: HTMLElement;
  /** Acumula un cambio (vivo o del historial); evita duplicados por ruta. */
  registrarCambio(cambio: CambioArchivo): void;
  /** Vacía la lista de cambios (al cambiar de conversación). */
  limpiarCambios(): void;
  /** Fija el área de trabajo para resolver rutas relativas. */
  fijarBase(ruta: string | null): void;
  /** Carga un archivo en la pestaña Archivo. */
  abrirArchivo(ruta: string): void;
}

type SubTab = 'archivo' | 'cambios';

export function montarPanelVisor(opts: {
  leerArchivo?: (ruta: string) => Promise<{ ruta: string; lineas: number; contenido: string }>;
} = {}): PanelVisor {
  const raiz = el('div', 'panel-visor');

  const barra = el('div', 'visor-barra');
  barra.setAttribute('role', 'tablist');
  const btnArchivo = subTabBtn('Archivo');
  const btnCambios = subTabBtn('Cambios');
  barra.appendChild(btnArchivo);
  barra.appendChild(btnCambios);

  // ---- pestaña Archivo ----
  const tabArchivo = el('div', 'visor-tab');
  const filaRuta = el('div', 'visor-ruta-fila');
  const inputRuta = el('input', 'visor-ruta') as HTMLInputElement;
  inputRuta.type = 'text';
  inputRuta.placeholder = 'ruta del archivo (relativa al área de trabajo)…';
  inputRuta.setAttribute('aria-label', 'ruta del archivo a ver');
  const btnCargar = el('button', 'btn') as HTMLButtonElement;
  btnCargar.type = 'button';
  btnCargar.textContent = 'cargar';
  filaRuta.appendChild(inputRuta);
  filaRuta.appendChild(btnCargar);
  // [089A-2] Barra meta estilo Paseo (FilePanelBar): nombre + ruta completa
  // en el tooltip + nº de líneas. Monocromo, sin colores.
  const meta = el('div', 'visor-meta');
  meta.hidden = true;
  const aviso = el('div', 'visor-aviso');
  aviso.hidden = true;
  const codigo = el('div', 'visor-codigo');
  codigo.setAttribute('aria-label', 'contenido del archivo');
  tabArchivo.appendChild(filaRuta);
  tabArchivo.appendChild(meta);
  tabArchivo.appendChild(aviso);
  tabArchivo.appendChild(codigo);

  // ---- pestaña Cambios ----
  const tabCambios = el('div', 'visor-tab');
  tabCambios.hidden = true;
  const listaCambios = el('div', 'visor-cambios');
  const vacio = el('div', 'visor-vacio');
  vacio.textContent = 'sin cambios todavía: aparecen aquí los archivos que el agente modifique (file_write / file_patch).';
  listaCambios.appendChild(vacio);
  tabCambios.appendChild(listaCambios);

  raiz.appendChild(barra);
  raiz.appendChild(tabArchivo);
  raiz.appendChild(tabCambios);

  let sub: SubTab = 'archivo';
  const vistos = new Map<string, HTMLElement>();

  function pintarSub(): void {
    btnArchivo.classList.toggle('sel', sub === 'archivo');
    btnCambios.classList.toggle('sel', sub === 'cambios');
    tabArchivo.hidden = sub !== 'archivo';
    tabCambios.hidden = sub !== 'cambios';
  }
  btnArchivo.addEventListener('click', () => {
    sub = 'archivo';
    pintarSub();
  });
  btnCambios.addEventListener('click', () => {
    sub = 'cambios';
    pintarSub();
  });

  function avisar(texto: string): void {
    aviso.textContent = texto;
    aviso.hidden = false;
  }

  async function cargar(ruta: string): Promise<void> {
    const r = ruta.trim();
    aviso.hidden = true;
    if (!r) {
      avisar('escribe una ruta primero.');
      return;
    }
    if (!opts.leerArchivo) {
      avisar('la lectura local solo está disponible en la app de escritorio.');
      return;
    }
    inputRuta.value = r;
    try {
      const res = await opts.leerArchivo(r);
      const nombre = res.ruta.split(/[/\\]/).pop() || res.ruta;
      meta.textContent = `${nombre} — ${res.lineas} líneas`;
      meta.title = res.ruta;
      meta.hidden = false;
      pintarCodigo(res.contenido);
    } catch (e: unknown) {
      avisar(`no se pudo leer: ${String(e)}`);
    }
  }

  function pintarCodigo(contenido: string): void {
    codigo.replaceChildren();
    const lineas = contenido.split('\n');
    // Hasta 2000 líneas numeradas; más allá, texto plano (rendimiento).
    if (lineas.length > 2000) {
      const pre = el('pre', 'visor-plano');
      pre.textContent = contenido;
      codigo.appendChild(pre);
      return;
    }
    const frag = document.createDocumentFragment();
    lineas.forEach((texto, i) => {
      const fila = el('div', 'visor-linea');
      const num = el('span', 'visor-num');
      num.textContent = String(i + 1);
      const txt = el('span', 'visor-texto');
      txt.textContent = texto === '' ? ' ' : texto;
      fila.appendChild(num);
      fila.appendChild(txt);
      frag.appendChild(fila);
    });
    codigo.appendChild(frag);
  }

  btnCargar.addEventListener('click', () => {
    void cargar(inputRuta.value);
  });
  inputRuta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter') void cargar(inputRuta.value);
  });

  pintarSub();

  return {
    raiz,
    registrarCambio(cambio) {
      const clave = `${cambio.origen}:${cambio.tool}:${cambio.ruta || cambio.titulo}`;
      vistos.get(clave)?.remove();
      if (vacio.parentNode === listaCambios) vacio.remove();
      const tarjeta = el('div', 'visor-cambio');
      const cab = el('div', 'visor-cambio-cab');
      const btnRuta = el('button', 'visor-cambio-ruta') as HTMLButtonElement;
      btnRuta.type = 'button';
      btnRuta.textContent = cambio.ruta || '(sin ruta)';
      btnRuta.title = 'ver archivo actual';
      btnRuta.addEventListener('click', () => {
        sub = 'archivo';
        pintarSub();
        void cargar(cambio.ruta);
      });
      const meta = el('span', 'visor-cambio-titulo');
      meta.textContent = cambio.titulo;
      cab.appendChild(btnRuta);
      cab.appendChild(meta);
      const diff = el('div', 'visor-cambio-diff');
      pintarDiffSeguro(diff, cambio.resumen, cambio.diff);
      tarjeta.appendChild(cab);
      tarjeta.appendChild(diff);
      listaCambios.prepend(tarjeta);
      vistos.set(clave, tarjeta);
    },
    fijarBase(_ruta) {
      // La ruta activa se resuelve en el backend; se conserva el método para
      // mantener la API del visor y los callers existentes.
    },
    limpiarCambios() {
      vistos.clear();
      listaCambios.replaceChildren(vacio);
    },
    abrirArchivo(ruta) {
      sub = 'archivo';
      pintarSub();
      void cargar(ruta);
    },
  };
}

function pintarDiffSeguro(contenedor: HTMLElement, resumen: string, diff: string | null): void {
  const resumenNodo = el('span', 'resumen');
  resumenNodo.textContent = resumen.trim();
  contenedor.appendChild(resumenNodo);
  if (!diff?.trim()) return;

  for (const linea of diff.split('\\n')) {
    if (linea.startsWith('@@')) continue;
    const fila = el('span', 'visor-diff-linea');
    const clase = linea.startsWith('+') ? 'add' : linea.startsWith('-') ? 'del' : 'ctx';
    fila.classList.add(clase);
    fila.textContent = linea.startsWith(' ') ? linea.slice(1) : linea;
    contenedor.appendChild(fila);
  }
}

function subTabBtn(etiqueta: string): HTMLButtonElement {
  const b = el('button', 'visor-subtab') as HTMLButtonElement;
  b.type = 'button';
  b.setAttribute('role', 'tab');
  b.textContent = etiqueta;
  return b;
}
