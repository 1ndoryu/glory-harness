/* [129A-11 F2] Tarjeta flotante de subagentes: una sola tarjeta agrupada,
 * minimizable, arriba a la derecha dentro del panel principal (el `.chat` es
 * su marco: `position: relative` en `layout.css`). Minimalista y pequeña.
 *
 * Vista PURA del stream: `SubagenteInicio { perfil, instruccion }` añade una
 * fila en estado `trabajando`; `SubagenteFin { resumen, ok, parcial }` cierra
 * la más antigua sin cerrar (FIFO — los eventos no traen id, ver F1 del
 * plan). Al terminar la fila queda colapsada con su resumen; el click abre
 * la ficha de solo lectura en el lateral (F3, vía `onAbrir`).
 *
 * Límites conocidos (F1): sin transcript persistido del hijo, la ficha solo
 * muestra instrucción+resumen; con 2 subagentes concurrentes el emparejamiento
 * inicio→fin por orden puede cruzarse.
 */
import { el, vaciar } from '../util/dom';
import { icono } from './iconos';
import '../estilos/subagentes.css';

/** Estado visible de un subagente (espejo del ciclo inicio→fin del núcleo). */
export type EstadoSubagente = 'trabajando' | 'fin' | 'parcial' | 'fallido';

/** Ficha de solo lectura (F3): lo único recuperable del hijo (F1). */
export interface FichaSubagente {
  seq: number;
  perfil: string;
  instruccion: string;
  estado: EstadoSubagente;
  resumen: string;
}

interface EntradaSubagente extends FichaSubagente {
  fila: HTMLButtonElement;
  iconoEstado: HTMLElement;
  texto: HTMLElement;
}

/** Tarjeta viva de subagentes: nodo + altas por inicio y cierres por fin. */
export interface SubagentesViva {
  raiz: HTMLElement;
  /** Alta por `subagente_inicio`; devuelve el nº de secuencia (tab F3). */
  inicio(perfil: string, instruccion: string): number;
  /** Cierre por `subagente_fin` (FIFO: la más antigua sin cerrar). */
  fin(resumen: string, ok: boolean, parcial: boolean): void;
  /** ¿Sigue montada en el documento? (el contenedor cambia de conversación). */
  montado(): boolean;
  /** Click en una fila → ficha de solo lectura en el lateral (F3). */
  onAbrir: ((ficha: FichaSubagente) => void) | null;
}

/** Instrucción recortada para la fila (la completa vive en la ficha F3). */
const MAX_INSTR_FILA = 120;

function recortar(texto: string, max: number): string {
  const limpio = texto.replace(/\s+/g, ' ').trim();
  return limpio.length > max ? `${limpio.slice(0, max)}…` : limpio;
}

function iconoDeEstado(estado: EstadoSubagente): SVGSVGElement {
  if (estado === 'trabajando') return icono('spin', true, 'ic-spin');
  if (estado === 'fin') return icono('check', true);
  return icono('reloj', true);
}

function textoEstado(estado: EstadoSubagente): string {
  if (estado === 'trabajando') return 'trabajando';
  if (estado === 'fin') return 'fin';
  if (estado === 'parcial') return 'parcial';
  return 'sin resumen';
}

/** Crea la tarjeta (oculta hasta el primer `inicio`). */
export function crearSubagentesViva(): SubagentesViva {
  const raiz = el('section', 'subagentes');
  raiz.setAttribute('aria-label', 'Subagentes en ejecución');
  const cabecera = el('button', 'subagentesCabecera') as HTMLButtonElement;
  cabecera.type = 'button';
  cabecera.title = 'minimizar / mostrar';
  cabecera.appendChild(icono('cerebro', true, 'subagentesIcono'));
  const titulo = el('span', 'subagentesTitulo');
  titulo.textContent = 'Subagentes';
  const cuenta = el('span', 'subagentesCuenta');
  cuenta.setAttribute('aria-live', 'polite');
  const plegar = el('span', 'subagentesPlegar');
  plegar.appendChild(icono('chevron-abajo', true));
  cabecera.appendChild(titulo);
  cabecera.appendChild(cuenta);
  cabecera.appendChild(plegar);
  const lista = el('div', 'subagentesLista');
  raiz.appendChild(cabecera);
  raiz.appendChild(lista);

  let seq = 0;
  const entradas: EntradaSubagente[] = [];
  const viva: SubagentesViva = {
    raiz,
    inicio,
    fin,
    montado: () => raiz.isConnected,
    onAbrir: null,
  };

  cabecera.addEventListener('click', () => {
    const plegada = raiz.classList.toggle('subagentesPlegada');
    vaciar(plegar);
    plegar.appendChild(icono(plegada ? 'chevron-derecha' : 'chevron-abajo', true));
  });

  function refrescarCuenta(): void {
    const activos = entradas.filter((e) => e.estado === 'trabajando').length;
    cuenta.textContent = activos > 0 ? `${activos} activos` : `${entradas.length}`;
  }

  function fila(entrada: EntradaSubagente): HTMLButtonElement {
    const nodo = el('button', 'subagente') as HTMLButtonElement;
    nodo.type = 'button';
    nodo.title = 'abrir ficha de solo lectura en el lateral';
    entrada.fila = nodo;
    entrada.iconoEstado = el('span', 'subagenteIcono');
    const cuerpo = el('span', 'subagenteCuerpo');
    const perfil = el('span', 'subagentePerfil');
    perfil.textContent = entrada.perfil;
    entrada.texto = el('span', 'subagenteTexto');
    cuerpo.appendChild(perfil);
    cuerpo.appendChild(entrada.texto);
    nodo.appendChild(entrada.iconoEstado);
    nodo.appendChild(cuerpo);
    pintarFila(entrada);
    nodo.addEventListener('click', () => {
      viva.onAbrir?.({
        seq: entrada.seq,
        perfil: entrada.perfil,
        instruccion: entrada.instruccion,
        estado: entrada.estado,
        resumen: entrada.resumen,
      });
    });
    return nodo;
  }

  function pintarFila(entrada: EntradaSubagente): void {
    vaciar(entrada.iconoEstado);
    entrada.iconoEstado.appendChild(iconoDeEstado(entrada.estado));
    // Trabajando: perfil + instrucción recortada. Terminado: colapsada con
    // su resumen (la ficha F3 guarda el texto completo).
    entrada.texto.textContent = entrada.estado === 'trabajando'
      ? recortar(entrada.instruccion, MAX_INSTR_FILA)
      : recortar(entrada.resumen || textoEstado(entrada.estado), MAX_INSTR_FILA);
    entrada.fila.dataset.estado = entrada.estado;
    entrada.fila.setAttribute(
      'aria-label',
      `subagente ${entrada.perfil}: ${textoEstado(entrada.estado)}`,
    );
  }

  function inicio(perfil: string, instruccion: string): number {
    seq += 1;
    const entrada: EntradaSubagente = {
      seq,
      perfil,
      instruccion,
      estado: 'trabajando',
      resumen: '',
      fila: el('button', 'subagente') as HTMLButtonElement,
      iconoEstado: el('span', 'subagenteIcono'),
      texto: el('span', 'subagenteTexto'),
    };
    // `fila()` sustituye los nodos provisionales por los definitivos.
    entrada.fila = fila(entrada);
    entradas.push(entrada);
    lista.appendChild(entrada.fila);
    raiz.hidden = false;
    refrescarCuenta();
    return seq;
  }

  function fin(resumen: string, ok: boolean, parcial: boolean): void {
    const abierta = entradas.find((e) => e.estado === 'trabajando');
    if (!abierta) return;
    abierta.estado = !ok ? 'fallido' : parcial ? 'parcial' : 'fin';
    abierta.resumen = resumen;
    pintarFila(abierta);
    refrescarCuenta();
  }

  raiz.hidden = true;
  return viva;
}

/** Ficha de solo lectura (F3): tab `subagente:<seq>` sin caja de escritura
 * por construcción (solo texto: ningún input, ningún botón de envío). */
export function crearFichaSubagente(ficha: FichaSubagente): HTMLElement {
  const raiz = el('section', 'fichaSubagente');
  const cabecera = el('div', 'fichaSubagenteCabecera');
  cabecera.appendChild(icono('cerebro', true));
  const titulo = el('h3', 'fichaSubagenteTitulo');
  titulo.textContent = `Subagente ${ficha.perfil}`;
  const estado = el('span', 'fichaSubagenteEstado');
  estado.textContent = textoEstado(ficha.estado);
  estado.dataset.estado = ficha.estado;
  cabecera.appendChild(titulo);
  cabecera.appendChild(estado);
  raiz.appendChild(cabecera);
  const objetivo = el('h4', 'fichaSubagenteApartado');
  objetivo.textContent = 'Objetivo delegado';
  const instr = el('p', 'fichaSubagenteTexto');
  instr.textContent = ficha.instruccion || '(sin instrucción registrada)';
  raiz.appendChild(objetivo);
  raiz.appendChild(instr);
  const apartadoResumen = el('h4', 'fichaSubagenteApartado');
  apartadoResumen.textContent = 'Resumen devuelto al padre';
  const resumen = el('p', 'fichaSubagenteTexto');
  resumen.textContent = ficha.estado === 'trabajando'
    ? '(todavía trabajando…)'
    : ficha.resumen || '(sin resumen)';
  raiz.appendChild(apartadoResumen);
  raiz.appendChild(resumen);
  return raiz;
}
