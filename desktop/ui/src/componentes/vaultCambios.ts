import { el } from '../util/dom';
import { normalizarRutaArea } from '../util/reposUtil';
import { crearSeccionColapsable, type SeccionColapsable } from '../util/seccionColapsable';
import {
  pintarDiff,
  separarEntradas,
  sumarCambios,
  type ArchivoGit,
} from './gitDiff';
import type { CambioArchivoTurno, RestauracionArchivo } from '../tauri/realTipos';

/** Transporte del vault: rechazo puntual (el listado lo orquesta el padre). */
export interface VaultCambiosTransport {
  rechazar(conversacionId: string, turnoId: string, ruta: string): Promise<RestauracionArchivo>;
}

export interface VaultCambios {
  raiz: HTMLElement;
  pintar(cambios: CambioArchivoTurno[], conv: string, nombreSeccion?: string, revelado?: string | null): void;
  pintarVacio(texto: string): void;
  ocultar(): void;
  /** [139A-2] Selección huérfana (ruta fuera de todo repo). */
  revelar(ruta: string): void;
}

/* [139A-2] Vista vault del panel Cambios: una fila por ruta con las MISMAS
 * clases git en vez de reinventar la lista. */
export function crearVaultCambios(ctx: {
  cambios: VaultCambiosTransport;
  diffsVivos: Map<string, string>;
  tomarRevelado: () => string | null;
  recargar: () => void;
  onToast?: (texto: string, detalle?: string) => void;
}): VaultCambios {
  const raiz = el('div');
  // Una fila por ruta (el cambio más reciente) + selección viva.
  let turnos = new Map<string, CambioArchivoTurno>();
  let archivos: ArchivoGit[] = [];
  let seleccion: string | null = null;
  // Referencias del último pintado (para el revelado huérfano).
  let lista: HTMLElement | null = null;
  let cajaDiff: HTMLElement | null = null;
  let conv: string | null = null;
  // [139A-6] El vault repinta desde cero en cada recarga: el plegado se
  // conserva aquí (nace minimizado, igual que los repos).
  let colapsadoVault = true;
  let seccionVault: SeccionColapsable | null = null;

  function claveRevisados(conversacion: string): string {
    return `cambios-revisados:${conversacion}`;
  }

  function leerRevisados(conversacion: string): Set<string> {
    try {
      const crudo = localStorage.getItem(claveRevisados(conversacion));
      const arr: unknown = crudo ? JSON.parse(crudo) : [];
      return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : []);
    } catch {
      return new Set();
    }
  }

  function guardarRevisados(conversacion: string, revisados: Set<string>): void {
    try {
      localStorage.setItem(claveRevisados(conversacion), JSON.stringify([...revisados]));
    } catch {
      /* sin persistencia: la marca vive en memoria esta sesión */
    }
  }

  function crearStat(marca: '+' | '−', cantidad: number): HTMLElement {
    const nodo = el('span', marca === '+' ? 'git-adiciones' : 'git-eliminaciones');
    nodo.textContent = `${marca}${cantidad}`;
    return nodo;
  }

  function pintarVacio(texto: string): void {
    raiz.hidden = false;
    raiz.replaceChildren();
    archivos = [];
    turnos = new Map();
    seleccion = null;
    lista = null;
    cajaDiff = null;
    seccionVault = null;
    const vacio = el('div', 'git-vacio');
    vacio.textContent = texto;
    raiz.appendChild(vacio);
  }

  /* `revelado` (multi-repo) es el objetivo ya consumido por el dueño:
   * cuando se pasa manda sobre `tomarRevelado` (no comerse el de un repo). */
  function pintar(
    cambios: CambioArchivoTurno[],
    conversacion: string,
    nombreSeccion = 'Cambios',
    revelado?: string | null,
  ): void {
    raiz.hidden = false;
    raiz.replaceChildren();
    // Una fila por ruta (el turno más reciente si varios la tocaron).
    turnos = new Map();
    for (const c of cambios) {
      const clave = normalizarRutaArea(c.ruta);
      const previo = turnos.get(clave);
      if (!previo || c.en_ms >= previo.en_ms) turnos.set(clave, c);
    }
    const rutas = [...turnos.keys()].sort((a, b) => a.localeCompare(b));
    if (rutas.length === 0) {
      if (revelado === undefined) ctx.tomarRevelado();
      pintarVacio('el agente aún no tocó archivos en esta conversación');
      return;
    }
    // Diffs vivos envueltos en cabecera `diff --git` para que
    // `separarEntradas` los atribuya por ruta igual que con git real.
    const vivos = new Map<string, string>();
    for (const [ruta, diff] of ctx.diffsVivos) vivos.set(normalizarRutaArea(ruta), diff);
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
    archivos = datos.changes;

    const contenido = el('div', 'git-contenido');
    const listaNodos = el('div', 'git-lista');
    listaNodos.setAttribute('role', 'list');
    const caja = el('div', 'git-diff');
    caja.hidden = true;
    // [139A-6] La misma sección colapsable de los repos (minimizada por
    // defecto); el diff vive fuera y se coordina aparte.
    const sec = crearSeccionColapsable();
    sec.titulo.textContent = nombreSeccion;
    sec.contador.textContent = String(archivos.length);
    const stat = el('span', 'git-seccion-estadistica');
    const totales = sumarCambios(archivos);
    stat.append(crearStat('+', totales.adiciones), crearStat('−', totales.eliminaciones));
    sec.cab.append(stat);
    const filas = el('div', 'git-seccion-lista seccion-cuerpo');
    const revisados = leerRevisados(conversacion);
    for (const archivo of archivos) {
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
      fila.addEventListener('click', () => seleccionar(archivo, conversacion, listaNodos, caja, true));
      filas.appendChild(fila);
    }
    sec.seccion.appendChild(filas);
    listaNodos.appendChild(sec.seccion);
    contenido.append(listaNodos, caja);
    raiz.appendChild(contenido);
    lista = listaNodos;
    cajaDiff = caja;
    conv = conversacion;
    seccionVault = sec;
    sec.fijarColapso(colapsadoVault);
    // [139A-6] Plegada, la lista vacía pegaría su borde al de la cabecera
    // (2px): `.lista-plegada` lo anula coordinada con el colapso.
    listaNodos.classList.toggle('lista-plegada', sec.colapsado);
    sec.cab.addEventListener('click', () => {
      // El helper ya plegó/desplegó: espejar el gesto y coordinar el diff.
      colapsadoVault = sec.colapsado;
      listaNodos.classList.toggle('lista-plegada', sec.colapsado);
      caja.hidden = sec.colapsado || seleccion === null;
    });

    // Restaura la selección o aplica el revelado pendiente (una sola vez).
    const objetivo = revelado !== undefined ? revelado : ctx.tomarRevelado();
    if (objetivo) {
      const archivo = archivos.find(
        (a) => a.ruta === objetivo || a.ruta === normalizarRutaArea(objetivo),
      );
      if (archivo) {
        // El revelado expande la sección (nace minimizada).
        sec.fijarColapso(false);
        colapsadoVault = false;
        listaNodos.classList.toggle('lista-plegada', false);
        seleccionar(archivo, conversacion, listaNodos, caja, true);
        listaNodos
          .querySelector(`.git-entrada[data-ruta="${CSS.escape(archivo.ruta)}"]`)
          ?.scrollIntoView({ block: 'nearest' });
      }
    } else if (seleccion && !sec.colapsado) {
      const archivo = archivos.find((a) => a.ruta === seleccion);
      if (archivo) seleccionar(archivo, conversacion, listaNodos, caja, true);
      else seleccion = null;
    } else if (seleccion && sec.colapsado && !archivos.some((a) => a.ruta === seleccion)) {
      seleccion = null;
    }
  }

  function seleccionar(
    archivo: ArchivoGit,
    conversacion: string,
    listaNodos: HTMLElement,
    caja: HTMLElement,
    mostrarDiff: boolean,
  ): void {
    seleccion = archivo.ruta;
    caja.hidden = !mostrarDiff;
    pintarDiff(caja, archivo, acciones(archivo, conversacion, listaNodos));
    listaNodos.querySelectorAll('.git-entrada').forEach((fila) => {
      fila.classList.toggle(
        'seleccionada',
        fila instanceof HTMLElement && fila.dataset.ruta === archivo.ruta,
      );
    });
  }

  function acciones(archivo: ArchivoGit, conversacion: string, listaNodos: HTMLElement): HTMLElement[] {
    const cambio = turnos.get(archivo.ruta);
    const revisados = leerRevisados(conversacion);
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
      listaNodos.querySelectorAll('.git-entrada').forEach((fila) => {
        if (fila instanceof HTMLElement && fila.dataset.ruta === archivo.ruta) fila.classList.toggle('revisado', ok);
      });
    };
    refrescar();
    // Aceptar = solo marca revisado (localStorage por conversación).
    btnAceptar.addEventListener('click', () => {
      revisados.add(archivo.ruta);
      guardarRevisados(conversacion, revisados);
      refrescar();
    });
    // Rechazar = restaura el previo del vault, directo.
    btnRechazar.addEventListener('click', () => {
      if (!cambio) return;
      btnAceptar.disabled = true;
      btnRechazar.disabled = true;
      estado.textContent = 'restaurando…';
      ctx.cambios
        .rechazar(conversacion, cambio.turno_id, cambio.ruta)
        .then((r) => {
          if (r.estado === 'restaurado') {
            ctx.onToast?.(`rechazado: ${cambio.ruta}`, 'restaurado al estado previo del turno');
          } else ctx.onToast?.(`no se tocó ${cambio.ruta}`, r.detalle ?? r.estado);
          ctx.recargar();
        })
        .catch((e: unknown) => {
          ctx.onToast?.(`no se pudo rechazar ${cambio.ruta}`, String(e));
          ctx.recargar();
        });
    });
    return [estado, btnAceptar, btnRechazar];
  }

  function ocultar(): void {
    raiz.replaceChildren();
    raiz.hidden = true;
    seccionVault = null;
  }

  function revelar(ruta: string): void {
    if (!lista || !cajaDiff || !conv || !seccionVault) return;
    const archivo = archivos.find((a) => a.ruta === ruta);
    if (!archivo) return;
    // [139A-6] El revelado expande la sección (nace minimizada).
    seccionVault.fijarColapso(false);
    colapsadoVault = false;
    lista.classList.toggle('lista-plegada', false);
    seleccionar(archivo, conv, lista, cajaDiff, true);
    lista
      .querySelector(`.git-entrada[data-ruta="${CSS.escape(archivo.ruta)}"]`)
      ?.scrollIntoView({ block: 'nearest' });
  }

  return { raiz, pintar, pintarVacio, ocultar, revelar };
}
