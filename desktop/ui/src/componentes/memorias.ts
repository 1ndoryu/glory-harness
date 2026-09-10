/* [109A-3] Panel "Memorias" del modal de configuración.
 *
 * Muestra y mantiene los recuerdos del PROYECTO ABIERTO: el backend resuelve
 * el ámbito con el área activa de la sesión, así que aquí no se elige ni
 * viaja ningún identificador de proyecto. Cuando el área activa no está
 * registrada el ámbito real es el global y el panel lo advierte para no
 * atribuir al proyecto recuerdos que comparte todo el usuario.
 *
 * El esquema declarativo de `dominio/opciones.ts` no admite listas gestoras,
 * así que este panel es custom y se monta como sección del modal.
 */

import '../estilos/memorias.css';

import type {
  ListadoMemoria,
  RecuerdoMemoria,
  ResultadoCarpetaMemoria,
} from '../tauri/real';
import type { IconoNombre } from '../dominio/tipos';
import { el, vaciar } from '../util/dom';
import { copiarAlPortapapeles } from '../util/portapapeles';
import { icono } from './iconos';

/** Acciones del backend que el panel consume (todas del ámbito activo). */
export interface MemoriasDeps {
  listar(): Promise<ListadoMemoria>;
  borrar(clave: string): Promise<ListadoMemoria>;
  curar(): Promise<string>;
  exportar(): Promise<ResultadoCarpetaMemoria>;
  importar(): Promise<ResultadoCarpetaMemoria>;
  /** Aviso global del orquestador (toast + registro). */
  avisar(texto: string, meta: string, detalle: string): void;
}

export interface MemoriasPanel {
  raiz: HTMLElement;
  /** Recarga desde el backend (al abrir el modal o la sección). */
  refrescar(): void;
}

/** Botón de la barra de acciones con icono + etiqueta. */
function botonAccion(
  etiqueta: string,
  nombre: IconoNombre,
  alPulsar: () => void,
): HTMLButtonElement {
  const b = el('button', 'memorias-accion') as HTMLButtonElement;
  b.type = 'button';
  b.title = etiqueta;
  b.appendChild(icono(nombre, true));
  const texto = el('span');
  texto.textContent = etiqueta;
  b.appendChild(texto);
  b.addEventListener('click', alPulsar);
  return b;
}

/** Recorta texto largo para la lista (el detalle muestra el original). */
function recorte(texto: string, max: number): string {
  const plano = texto.replace(/\s+/g, ' ').trim();
  return plano.length > max ? `${plano.slice(0, max)}…` : plano;
}

/** Fecha ISO → "YYYY-MM-DD HH:MM" (sin depender del locale del sistema). */
function fechaCorta(iso: string): string {
  return iso.slice(0, 16).replace('T', ' ');
}

export function montarMemorias(deps: MemoriasDeps): MemoriasPanel {
  const raiz = el('div', 'memorias');

  const titulo = el('div', 'memorias-titulo');
  const subtitulo = el('div', 'memorias-subtitulo');
  const identidad = el('div', 'memorias-identidad');
  identidad.append(titulo, subtitulo);

  const acciones = el('div', 'memorias-acciones');
  const cabecera = el('div', 'memorias-cabecera');
  cabecera.append(identidad, acciones);

  const busqueda = el('input', 'input-texto memorias-busqueda') as HTMLInputElement;
  busqueda.type = 'search';
  busqueda.placeholder = 'buscar por clave o contenido';
  busqueda.spellcheck = false;
  busqueda.addEventListener('input', () => pintarFilas());

  const lista = el('div', 'memorias-lista');
  const estado = el('div', 'memorias-estado');

  const detalleTitulo = el('div', 'memorias-detalle-titulo');
  const detalleMeta = el('div', 'memorias-detalle-meta');
  const detalleCuerpo = el('pre', 'memorias-detalle-cuerpo');
  const detalleAcciones = el('div', 'memorias-detalle-acciones');
  const detalle = el('div', 'memorias-detalle');
  detalle.hidden = true;
  detalle.append(detalleTitulo, detalleMeta, detalleCuerpo, detalleAcciones);

  raiz.append(cabecera, busqueda, estado, lista, detalle);

  /** Último listado recibido (el filtro local trabaja sobre él). */
  let ultimo: ListadoMemoria | null = null;
  /** Clave pendiente de confirmar borrado (borrar es irreversible). */
  let confirmando: string | null = null;
  let ocupado = false;

  function textoEstado(texto: string, clase = ''): void {
    vaciar(estado);
    if (!texto) return;
    const linea = el('div', clase ? `memorias-nota ${clase}` : 'memorias-nota');
    linea.textContent = texto;
    estado.appendChild(linea);
  }

  /** Cabecera de identidad: de qué ámbito son los recuerdos que se ven. */
  function pintarIdentidad(l: ListadoMemoria): void {
    titulo.textContent = l.proyecto ? `Memorias de ${l.proyecto}` : 'Memorias globales';
    vaciar(subtitulo);
    const distintivo = el('span', l.global ? 'memorias-chip global' : 'memorias-chip');
    distintivo.textContent = l.global ? 'global' : 'proyecto';
    const ruta = el('span', 'memorias-ruta');
    ruta.textContent = l.global
      ? 'sin proyecto activo: estos recuerdos se comparten con todo el usuario'
      : (l.ruta ?? '');
    subtitulo.append(distintivo, ruta);
  }

  function pintarFilas(): void {
    const l = ultimo;
    vaciar(lista);
    if (!l) return;
    const filtro = busqueda.value.trim().toLowerCase();
    const visibles = l.recuerdos.filter(
      (r) =>
        !filtro ||
        r.clave.toLowerCase().includes(filtro) ||
        r.contenido.toLowerCase().includes(filtro),
    );
    if (l.recuerdos.length === 0) {
      textoEstado(
        'sin recuerdos: el agente guarda con `memoria_guardar` o con el subcomando `memoria guardar`',
      );
      return;
    }
    if (visibles.length === 0) {
      textoEstado(`${l.recuerdos.length} recuerdo(s), ninguno coincide con el filtro`);
      return;
    }
    textoEstado(
      filtro
        ? `${visibles.length} de ${l.recuerdos.length} recuerdo(s)`
        : `${l.recuerdos.length} recuerdo(s)`,
    );
    visibles.forEach((r) => lista.appendChild(fila(r)));
  }

  function fila(r: RecuerdoMemoria): HTMLElement {
    const contenedor = el('div', 'memoria-fila');

    const principal = el('button', 'memoria-principal') as HTMLButtonElement;
    principal.type = 'button';
    const clave = el('span', 'memoria-clave');
    clave.textContent = r.clave;
    const resumen = el('span', 'memoria-resumen');
    resumen.textContent = recorte(r.contenido, 80);
    const meta = el('span', 'memoria-meta');
    meta.textContent =
      `usos ${r.usos} · ${r.origen}` + (r.archivada ? ' · archivada (no se inyecta)' : '');
    principal.append(clave, resumen, meta);
    principal.addEventListener('click', () => mostrarDetalle(r));
    contenedor.appendChild(principal);

    const olvidar = el('button', 'memoria-borrar') as HTMLButtonElement;
    olvidar.type = 'button';
    olvidar.addEventListener('click', () => {
      if (confirmando !== r.clave) {
        // Primer click: confirma (borrar un recuerdo no se puede deshacer).
        confirmando = r.clave;
        olvidar.textContent = '¿olvidar?';
        olvidar.classList.add('confirmar');
        pintarFilas();
        return;
      }
      confirmando = null;
      void borrar(r.clave);
    });
    if (confirmando === r.clave) {
      olvidar.textContent = '¿olvidar?';
      olvidar.classList.add('confirmar');
      olvidar.title = 'pulsa otra vez para olvidar este recuerdo';
    } else {
      olvidar.appendChild(icono('x', true));
      olvidar.title = `olvidar '${r.clave}'`;
    }
    contenedor.appendChild(olvidar);
    return contenedor;
  }

  function mostrarDetalle(r: RecuerdoMemoria): void {
    detalleTitulo.textContent = r.clave;
    detalleMeta.textContent =
      `actualizada ${fechaCorta(r.actualizada_en)}` +
      (r.ultimo_uso ? ` · último uso ${fechaCorta(r.ultimo_uso)}` : '') +
      ` · origen ${r.origen}`;
    detalleCuerpo.textContent = r.contenido;
    vaciar(detalleAcciones);
    detalleAcciones.append(
      botonAccion('copiar', 'copiar', () => {
        void copiarAlPortapapeles(r.contenido)
          .then(() => deps.avisar(`recuerdo '${r.clave}' copiado`, '', ''))
          .catch((e: unknown) => deps.avisar(`no se pudo copiar: ${String(e)}`, '', ''));
      }),
      botonAccion('cerrar', 'x', () => {
        detalle.hidden = true;
      }),
    );
    detalle.hidden = false;
  }

  function pintar(l: ListadoMemoria): void {
    ultimo = l;
    confirmando = null;
    detalle.hidden = true;
    pintarIdentidad(l);
    pintarFilas();
  }

  /** Envuelve una acción del backend con el estado de ocupado y el error. */
  function ejecutar(etiqueta: string, tarea: () => Promise<void>): void {
    if (ocupado) return;
    ocupado = true;
    textoEstado(`${etiqueta}…`);
    void tarea()
      .catch((e: unknown) => {
        const motivo = String(e);
        textoEstado(`${etiqueta}: ${motivo}`, 'error');
        deps.avisar(`${etiqueta}: ${motivo}`, '', '');
      })
      .finally(() => {
        ocupado = false;
      });
  }

  function cargar(): void {
    ejecutar('cargando', async () => {
      pintar(await deps.listar());
    });
  }

  function borrar(clave: string): void {
    ejecutar('olvidando', async () => {
      pintar(await deps.borrar(clave));
      deps.avisar(`recuerdo '${clave}' olvidado`, '', '');
    });
  }

  function curar(): void {
    ejecutar('curando', async () => {
      const reporte = await deps.curar();
      deps.avisar('pasada del curador terminada', '', reporte.trim());
      pintar(await deps.listar());
    });
  }

  function moverCarpeta(accion: 'exportar' | 'importar'): void {
    ejecutar(accion, async () => {
      const res = accion === 'exportar' ? await deps.exportar() : await deps.importar();
      const omitidos = res.omitidos.length
        ? `\n${res.omitidos.length} archivo(s) omitidos:\n- ${res.omitidos.join('\n- ')}`
        : '';
      deps.avisar(
        `${res.recuerdos} recuerdo(s) ${accion === 'exportar' ? 'exportados' : 'importados'}`,
        res.carpeta,
        omitidos.trim(),
      );
      pintar(await deps.listar());
    });
  }

  acciones.append(
    botonAccion('curar', 'cerebro', curar),
    botonAccion('exportar', 'flecha-arriba', () => moverCarpeta('exportar')),
    botonAccion('importar', 'flecha-izq', () => moverCarpeta('importar')),
    botonAccion('recargar', 'recargar', cargar),
  );

  return {
    raiz,
    refrescar: cargar,
  };
}
