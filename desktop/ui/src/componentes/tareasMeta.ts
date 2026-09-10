/* [109A-5 F2] Plan visible de la conversación (tool `todo`): bloque con las
 * tareas del turno y su estado en vivo.
 *
 * Es una vista PURA del contrato `tareas_actualizadas`: cada evento trae la
 * lista completa, así que el componente no acumula deltas ni estado propio que
 * pueda divergir del backend. Las marcas textuales `[ ]`/`[/]`/`[x]` son el
 * espejo exacto de `EstadoTodo::marca` del núcleo, para que el usuario vea el
 * mismo plan que ve el modelo en su contexto. */
import type { EstadoTareaVisible, TareaVisible } from '../dominio/tipos';
import { el, vaciar } from '../util/dom';
import { icono } from './iconos';
import '../estilos/tareasMeta.css';

/** Marca textual por estado (espejo de `EstadoTodo::marca` en el núcleo). */
const MARCAS: Record<EstadoTareaVisible, string> = {
  pendiente: '[ ]',
  en_curso: '[/]',
  completada: '[x]',
};

/** Clase de fila por estado; los valores visuales viven en el CSS. */
const CLASES: Record<EstadoTareaVisible, string> = {
  pendiente: 'tareaPendiente',
  en_curso: 'tareaEnCurso',
  completada: 'tareaCompletada',
};

/** Bloque vivo del plan: nodo + pintado de listas completas. */
export interface TareasViva {
  raiz: HTMLElement;
  /** Pinta la lista COMPLETA recibida (el backend nunca manda deltas). */
  actualizar(items: TareaVisible[]): void;
  /** ¿Sigue montado en el documento? (el contenedor cambia de conversación). */
  montado(): boolean;
}

/** Icono Lucide de cada estado (`check` hecho, `spin` en curso, `reloj`
 * pendiente). La marca textual ya distingue el estado: el icono solo refuerza. */
function iconoDeEstado(estado: EstadoTareaVisible): SVGSVGElement {
  if (estado === 'completada') return icono('check', true);
  if (estado === 'en_curso') return icono('spin', true, 'ic-spin');
  return icono('reloj', true);
}

/** Crea el bloque (vacío y oculto hasta el primer `actualizar` con tareas). */
export function crearTareasViva(): TareasViva {
  const raiz = el('section', 'tareas');
  raiz.setAttribute('aria-label', 'Tareas de la conversación');
  const cabecera = el('div', 'tareasCabecera');
  cabecera.appendChild(icono('flujo', true, 'tareasIcono'));
  const titulo = el('span', 'tareasTitulo');
  titulo.textContent = 'Tareas';
  const cuenta = el('span', 'tareasCuenta');
  cuenta.setAttribute('aria-live', 'polite');
  cabecera.appendChild(titulo);
  cabecera.appendChild(cuenta);
  const lista = el('ol', 'tareasLista');
  raiz.appendChild(cabecera);
  raiz.appendChild(lista);

  function fila(tarea: TareaVisible): HTMLLIElement {
    const nodoFila = el('li', `tarea ${CLASES[tarea.estado]}`);
    const nodoIcono = el('span', 'tareaIcono');
    nodoIcono.appendChild(iconoDeEstado(tarea.estado));
    const marca = el('span', 'tareaMarca');
    marca.textContent = MARCAS[tarea.estado];
    const texto = el('span', 'tareaTexto');
    texto.textContent = tarea.texto;
    nodoFila.appendChild(nodoIcono);
    nodoFila.appendChild(marca);
    nodoFila.appendChild(texto);
    return nodoFila;
  }

  function actualizar(items: TareaVisible[]): void {
    /* Repintado completo: la lista es corta (el plan de un turno) y el evento
     * ya trae el estado final, así que un diff por id añadiría código sin
     * ahorrar nada medible. */
    vaciar(lista);
    for (const tarea of items) lista.appendChild(fila(tarea));
    const hechas = items.filter((t) => t.estado === 'completada').length;
    cuenta.textContent = `${hechas}/${items.length}`;
    /* Lista vacía = el backend cerró la meta: se oculta en vez de mostrar un
     * encabezado "Tareas 0/0" que no significa nada. */
    raiz.hidden = items.length === 0;
  }

  actualizar([]);
  return {
    raiz,
    actualizar,
    montado: () => raiz.isConnected,
  };
}
