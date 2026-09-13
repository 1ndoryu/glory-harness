import '../estilos/cambios.css';
import { el } from '../util/dom';
import { montarPanelGit, type EstadoGit, type GitTransport, type PanelGit } from './panelGit';
import {
  pintarDiff,
  separarEntradas,
  sumarCambios,
  type ArchivoGit,
} from './gitDiff';
import type { CambioArchivoTurno, RestauracionArchivo } from '../tauri/realTipos';

/** Transporte del panel Cambios: vault por turno + rechazo puntual. */
export interface CambiosTransport {
  listar(conversacionId: string): Promise<CambioArchivoTurno[]>;
  rechazar(conversacionId: string, turnoId: string, ruta: string): Promise<RestauracionArchivo>;
}

export interface PanelCambios {
  raiz: HTMLElement;
  recargar(): void;
  /** [129A-7 F3] Refresco en vivo tras una escritura del agente (el vault ya
   * la tiene: re-consulta con antirrebote y conserva la selección por ruta). */
  registrarCambioVivo(ruta: string, diff: string | null): void;
  /** [129A-8] Revela el archivo en el próximo pintado: lo selecciona, muestra
   * su diff y lo desplaza a la vista (en ambos modos: git o vault). */
  revelar(ruta: string): void;
}

/* [139A-1] Panel "Cambios": con git aplicable muestra SOLO el estado git (la
 * lista separada del vault era redundante); sin git muestra los cambios del
 * vault con las MISMAS clases git (sección, filas, caja de diff) en vez de
 * reinventar la lista (`cambios-lista` eliminada). */
export function montarPanelCambios(opts: {
  git: GitTransport;
  cambios: CambiosTransport;
  convId: () => string | null;
  onError?: (texto: string, detalle?: string) => void;
  onToast?: (texto: string, detalle?: string) => void;
}): PanelCambios {
  const raiz = el('div', 'panel-cambios');
  const vault = el('div');
  const git: PanelGit = montarPanelGit({ transporte: opts.git, onError: opts.onError });
  git.raiz.hidden = true;
  raiz.append(vault, git.raiz);

  let secuencia = 0;
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let diffsVivos = new Map<string, string>();
  // [129A-8] Revelado pendiente (el listado es asíncrono: se aplica al pintar).
  let revelarPendiente: string | null = null;
  // Vista vault: una fila por ruta (el cambio más reciente) + selección viva.
  let vaultTurnos = new Map<string, CambioArchivoTurno>();
  let vaultArchivos: ArchivoGit[] = [];
  let seleccionVault: string | null = null;

  function claveRevisados(conv: string): string {
    return `cambios-revisados:${conv}`;
  }

  function leerRevisados(conv: string): Set<string> {
    try {
      const crudo = localStorage.getItem(claveRevisados(conv));
      const arr: unknown = crudo ? JSON.parse(crudo) : [];
      return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : []);
    } catch {
      return new Set();
    }
  }

  function guardarRevisados(conv: string, revisados: Set<string>): void {
    try {
      localStorage.setItem(claveRevisados(conv), JSON.stringify([...revisados]));
    } catch {
      /* sin persistencia: la marca vive en memoria esta sesión */
    }
  }

  function normalizarRuta(ruta: string): string {
    return ruta.replaceAll('\\', '/').trim();
  }

  function crearStat(marca: '+' | '−', cantidad: number): HTMLElement {
    const nodo = el('span', marca === '+' ? 'git-adiciones' : 'git-eliminaciones');
    nodo.textContent = `${marca}${cantidad}`;
    return nodo;
  }

  function pintarVaultVacio(texto: string): void {
    vault.replaceChildren();
    vaultArchivos = [];
    vaultTurnos = new Map();
    seleccionVault = null;
    const vacio = el('div', 'git-vacio');
    vacio.textContent = texto;
    vault.appendChild(vacio);
  }

  function pintarVault(cambios: CambioArchivoTurno[], conv: string): void {
    vault.replaceChildren();
    // Una fila por ruta (el turno más reciente si varios la tocaron).
    vaultTurnos = new Map();
    for (const c of cambios) {
      const clave = normalizarRuta(c.ruta);
      const previo = vaultTurnos.get(clave);
      if (!previo || c.en_ms >= previo.en_ms) vaultTurnos.set(clave, c);
    }
    const rutas = [...vaultTurnos.keys()].sort((a, b) => a.localeCompare(b));
    if (rutas.length === 0) {
      revelarPendiente = null;
      pintarVaultVacio('el agente aún no tocó archivos en esta conversación');
      return;
    }
    // Diffs vivos envueltos en cabecera `diff --git` para que
    // `separarEntradas` los atribuya por ruta igual que con git real.
    const vivos = new Map<string, string>();
    for (const [ruta, diff] of diffsVivos) vivos.set(normalizarRuta(ruta), diff);
    const diffUnstaged = rutas
      .map((ruta) => {
        const vivo = vivos.get(ruta)?.trim();
        if (!vivo) return '';
        return `diff --git a/${ruta} b/${ruta}\n--- a/${ruta}\n+++ b/${ruta}\n${vivo}`;
      })
      .filter((bloque) => bloque !== '')
      .join('\n');
    const datos = separarEntradas(
      rutas.map((ruta) => ({ estado: ' M', ruta })),
      '',
      diffUnstaged,
    );
    vaultArchivos = datos.changes;

    const contenido = el('div', 'git-contenido');
    const lista = el('div', 'git-lista');
    lista.setAttribute('role', 'list');
    const cajaDiff = el('div', 'git-diff');
    cajaDiff.hidden = true;
    const seccion = el('section', 'git-seccion');
    const cab = el('div', 'git-seccion-cabecera');
    const titulo = el('span', 'git-seccion-titulo');
    titulo.textContent = 'Cambios';
    const contador = el('span', 'git-seccion-contador');
    contador.textContent = String(vaultArchivos.length);
    const stat = el('span', 'git-seccion-estadistica');
    const totales = sumarCambios(vaultArchivos);
    stat.append(crearStat('+', totales.adiciones), crearStat('−', totales.eliminaciones));
    cab.append(titulo, contador, stat);
    seccion.appendChild(cab);
    const filas = el('div', 'git-seccion-lista');
    const revisados = leerRevisados(conv);
    for (const archivo of vaultArchivos) {
      const fila = el('button', 'git-entrada');
      fila.type = 'button';
      fila.setAttribute('role', 'listitem');
      fila.dataset.ruta = archivo.ruta;
      fila.title = archivo.ruta;
      fila.classList.toggle('revisado', revisados.has(archivo.ruta));
      const codigo = el('span', 'git-codigo');
      codigo.textContent = 'M';
      const rutaNodo = el('span', 'git-ruta');
      rutaNodo.textContent = archivo.ruta;
      const statArchivo = el('span', 'git-entrada-estadistica');
      statArchivo.append(crearStat('+', archivo.adiciones), crearStat('−', archivo.eliminaciones));
      fila.append(codigo, rutaNodo, statArchivo);
      fila.addEventListener('click', () => seleccionarVault(archivo, conv, lista, cajaDiff));
      filas.appendChild(fila);
    }
    seccion.appendChild(filas);
    lista.appendChild(seccion);
    contenido.append(lista, cajaDiff);
    vault.appendChild(contenido);

    // Restaura la selección o aplica el revelado pendiente (una sola vez).
    const objetivo = revelarPendiente;
    revelarPendiente = null;
    if (objetivo) {
      const archivo = vaultArchivos.find(
        (a) => a.ruta === objetivo || a.ruta === normalizarRuta(objetivo),
      );
      if (archivo) {
        seleccionarVault(archivo, conv, lista, cajaDiff);
        lista
          .querySelector(`.git-entrada[data-ruta="${CSS.escape(archivo.ruta)}"]`)
          ?.scrollIntoView({ block: 'nearest' });
      }
    } else if (seleccionVault) {
      const archivo = vaultArchivos.find((a) => a.ruta === seleccionVault);
      if (archivo) seleccionarVault(archivo, conv, lista, cajaDiff);
      else seleccionVault = null;
    }
  }

  function seleccionarVault(
    archivo: ArchivoGit,
    conv: string,
    lista: HTMLElement,
    cajaDiff: HTMLElement,
  ): void {
    seleccionVault = archivo.ruta;
    cajaDiff.hidden = false;
    pintarDiff(cajaDiff, archivo, accionesVault(archivo, conv, lista));
    lista.querySelectorAll('.git-entrada').forEach((fila) => {
      fila.classList.toggle(
        'seleccionada',
        fila instanceof HTMLElement && fila.dataset.ruta === archivo.ruta,
      );
    });
  }

  function accionesVault(archivo: ArchivoGit, conv: string, lista: HTMLElement): HTMLElement[] {
    const cambio = vaultTurnos.get(archivo.ruta);
    const revisados = leerRevisados(conv);
    const estado = el('span', 'cambios-estado');
    const btnAceptar = el('button', 'btn cambios-boton');
    btnAceptar.type = 'button';
    btnAceptar.textContent = 'Aceptar';
    const btnRechazar = el('button', 'btn cambios-boton cambios-boton-rechazar');
    btnRechazar.type = 'button';
    btnRechazar.textContent = 'Rechazar';
    const refrescar = () => {
      const ok = revisados.has(archivo.ruta);
      estado.textContent = ok ? 'revisado' : '';
      btnAceptar.disabled = ok;
      lista.querySelectorAll('.git-entrada').forEach((fila) => {
        if (fila instanceof HTMLElement && fila.dataset.ruta === archivo.ruta) {
          fila.classList.toggle('revisado', ok);
        }
      });
    };
    refrescar();
    // Aceptar = solo marca revisado (localStorage por conversación).
    btnAceptar.addEventListener('click', () => {
      revisados.add(archivo.ruta);
      guardarRevisados(conv, revisados);
      refrescar();
    });
    // Rechazar = restaura el previo del vault, directo.
    btnRechazar.addEventListener('click', () => {
      if (!cambio) return;
      btnAceptar.disabled = true;
      btnRechazar.disabled = true;
      estado.textContent = 'restaurando…';
      opts.cambios
        .rechazar(conv, cambio.turno_id, cambio.ruta)
        .then((r) => {
          if (r.estado === 'restaurado') {
            opts.onToast?.(`rechazado: ${cambio.ruta}`, 'restaurado al estado previo del turno');
            recargar();
          } else {
            opts.onToast?.(
              `no se tocó ${cambio.ruta}`,
              r.detalle ?? r.estado,
            );
            recargar();
          }
        })
        .catch((e: unknown) => {
          opts.onToast?.(`no se pudo rechazar ${cambio.ruta}`, String(e));
          recargar();
        });
    });
    return [estado, btnAceptar, btnRechazar];
  }

  async function cargar(): Promise<void> {
    const id = ++secuencia;
    // Una sola consulta a git por recarga: decide el modo y pinta sin refetch.
    let gitEstado: EstadoGit | null = null;
    let gitError: string | null = null;
    try {
      gitEstado = await opts.git.estado();
    } catch (error: unknown) {
      gitError = String(error);
    }
    if (id !== secuencia) return;
    if (gitEstado?.aplicable) {
      vault.replaceChildren();
      vault.hidden = true;
      git.raiz.hidden = false;
      git.fijar(gitEstado);
      const objetivo = revelarPendiente;
      revelarPendiente = null;
      if (objetivo && !git.seleccionar(objetivo)) git.seleccionar(normalizarRuta(objetivo));
      return;
    }
    if (gitError) opts.onError?.('no se pudo consultar Git', gitError);
    git.raiz.hidden = true;
    vault.hidden = false;
    const conv = opts.convId();
    if (!conv) {
      pintarVaultVacio('abre una conversación para ver sus cambios');
      return;
    }
    try {
      const cambios = await opts.cambios.listar(conv);
      if (id !== secuencia) return;
      pintarVault(cambios, conv);
    } catch (error: unknown) {
      if (id !== secuencia) return;
      vault.replaceChildren();
      opts.onError?.('no se pudieron listar los cambios', String(error));
    }
  }

  function recargar(): void {
    void cargar();
  }

  function registrarCambioVivo(ruta: string, diff: string | null): void {
    if (diff) diffsVivos.set(ruta, diff);
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(recargar, 800);
  }

  function revelar(ruta: string): void {
    revelarPendiente = ruta;
    recargar();
  }

  return { raiz, recargar, registrarCambioVivo, revelar };
}
