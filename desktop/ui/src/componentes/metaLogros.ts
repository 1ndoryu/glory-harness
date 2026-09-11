/* [109A-5 F3] Ciclo de vida de la meta: tiempo de persecución (neto de
 * pausas), cierre declarado e historial de logros.
 *
 * Vive fuera de `panelMeta.ts` porque el panel está en su límite de líneas y
 * porque son dos responsabilidades distintas: la fila del panel captura la
 * meta y refleja el turno; este bloque refleja la meta como OBJETIVO con su
 * reloj y su historial durable.
 *
 * El reloj es del front solo en apariencia: el backend desplaza `iniciada_en`
 * al reanudar para excluir el intervalo pausado, así que `ahora - iniciada_en`
 * es el tiempo de persecución real y en pausa queda congelado por
 * `pausada_en`. El front no acumula tiempo por su cuenta. */

import type { EstadoMetaVisible } from '../dominio/tipos';
import { el, vaciar } from '../util/dom';
import { formatearDuracion, formatearSegundos } from '../util/duracion';
import { icono, ponerIcono } from './iconos';
import '../estilos/metaLogros.css';

/** Contexto de turno que decide si se puede cerrar la meta. */
export interface ContextoMeta {
  /** Hay un turno en curso (M1): no se cierra la meta con trabajo a medias. */
  turnoActivo: boolean;
  /** ¿Existe un turno cerrado que pueda respaldar el logro? Sin él el backend
   * rechaza `lograr` (`turno_requerido`), así que el botón se deshabilita en
   * vez de ofrecer una acción que va a fallar. */
  hayTurno: boolean;
}

export interface MetaLogros {
  raiz: HTMLElement;
  /** Repinta con el estado durable. `null` = sin fila de conversación todavía
   * (borrador): no hay reloj ni historial que mostrar. */
  actualizar(estado: EstadoMetaVisible | null): void;
  setContexto(contexto: ContextoMeta): void;
  /** ¿Hay meta activa? El panel decide con esto si pausar/reanudar la meta. */
  activa(): boolean;
  /** ¿La meta activa está pausada? `reanudar` sobre una meta que corre es un
   * error del dominio, así que el llamador no debe emitirlo sin comprobar. */
  pausada(): boolean;
}

export interface MetaLogrosOpciones {
  /** Declara lograda la meta activa. El logro queda respaldado por el último
   * turno cerrado (lo resuelve el llamador). */
  onLograr: () => void;
}

/** Fecha corta local de un timestamp ISO; cadena vacía si no es parseable. */
function fechaCorta(iso: string): string {
  const fecha = new Date(iso);
  if (Number.isNaN(fecha.getTime())) return '';
  return fecha.toLocaleString('es', { day: '2-digit', month: '2-digit', hour: '2-digit', minute: '2-digit' });
}

export function crearMetaLogros(opts: MetaLogrosOpciones): MetaLogros {
  const raiz = el('div', 'metaLogros');
  raiz.hidden = true;

  const cabecera = el('div', 'metaLogrosCabecera');
  const titulo = el('span', 'metaLogrosTitulo');
  titulo.appendChild(icono('meta', true));
  titulo.appendChild(el('span')).textContent = 'meta';

  const persecucion = el('span', 'metaPersecucion');

  const btnLograr = el('button', 'metaBtnLograr') as HTMLButtonElement;
  btnLograr.type = 'button';
  btnLograr.title = 'declarar lograda la meta activa';
  btnLograr.setAttribute('aria-label', 'declarar lograda la meta activa');
  ponerIcono(btnLograr, 'check', true);
  btnLograr.appendChild(el('span')).textContent = 'lograr';
  btnLograr.addEventListener('click', () => opts.onLograr());

  cabecera.appendChild(titulo);
  cabecera.appendChild(persecucion);
  cabecera.appendChild(btnLograr);

  const historial = el('ol', 'metaHistorial');
  const tituloHistorial = el('div', 'metaHistorialTitulo');
  tituloHistorial.textContent = 'logros';

  raiz.appendChild(cabecera);
  raiz.appendChild(tituloHistorial);
  raiz.appendChild(historial);

  let estado: EstadoMetaVisible | null = null;
  let contexto: ContextoMeta = { turnoActivo: false, hayTurno: false };
  let timer: number | null = null;

  /** Meta vigente, o `null` si no hay (borrador, sin meta o meta ya lograda).
   * Centraliza la comprobación: repetirla en cada pintado invita a confundir
   * "sin estado" con "estado sin meta activa". */
  function metaActiva() {
    return estado?.activa ?? null;
  }

  /** Segundos de persecución: congelados si la meta está pausada. */
  function segundosPersecucion(): number | null {
    const activa = metaActiva();
    if (!activa) return null;
    const inicio = Date.parse(activa.iniciada_en);
    if (Number.isNaN(inicio)) return null;
    const fin = activa.pausada_en === null ? Date.now() : Date.parse(activa.pausada_en);
    if (Number.isNaN(fin)) return null;
    return Math.max(0, (fin - inicio) / 1000);
  }

  function pintarPersecucion(): void {
    const segundos = segundosPersecucion();
    const activa = metaActiva();
    const pausada = activa !== null && activa.pausada_en !== null;
    raiz.classList.toggle('metaLogrosEnPausa', pausada);
    if (segundos === null) {
      persecucion.textContent = 'sin meta activa';
      persecucion.title = 'fija una meta para empezar a medir la persecución';
      return;
    }
    persecucion.textContent = `${pausada ? 'en pausa' : 'persecución'} ${formatearSegundos(segundos)}`;
    persecucion.title = pausada
      ? 'el reloj de la meta está congelado'
      : 'tiempo dedicado a la meta, sin contar las pausas';
  }

  /** El intervalo solo existe mientras hay una meta activa y sin pausar: sin
   * él el reloj sería una animación sin dato detrás. */
  function ajustarReloj(): void {
    const activa = metaActiva();
    const corre = activa !== null && activa.pausada_en === null;
    if (corre && timer === null) timer = window.setInterval(pintarPersecucion, 1000);
    else if (!corre && timer !== null) {
      window.clearInterval(timer);
      timer = null;
    }
  }

  function pintarLogros(): void {
    vaciar(historial);
    const logros = estado?.logros ?? [];
    tituloHistorial.hidden = logros.length === 0;
    historial.hidden = logros.length === 0;
    // Más reciente arriba: el último cierre es el que se consulta.
    for (const logro of [...logros].reverse()) {
      const fila = el('li', 'metaLogro');
      fila.appendChild(icono('check', true));
      const texto = el('span', 'metaLogroTexto');
      texto.textContent = logro.meta;
      texto.title = logro.meta;
      fila.appendChild(texto);
      const tiempo = el('span', 'metaLogroTiempo');
      tiempo.textContent = formatearDuracion(logro.elapsed_ms);
      tiempo.title = 'tiempo de persecución neto de pausas';
      fila.appendChild(tiempo);
      const fecha = fechaCorta(logro.lograda_en);
      if (fecha !== '') {
        const nodo = el('span', 'metaLogroFecha');
        nodo.textContent = fecha;
        fila.appendChild(nodo);
      }
      historial.appendChild(fila);
    }
  }

  function pintarBoton(): void {
    btnLograr.disabled = metaActiva() === null || contexto.turnoActivo || !contexto.hayTurno;
  }

  function pintarVisibilidad(): void {
    const hayAlgo = estado !== null && (estado.activa !== null || estado.logros.length > 0);
    raiz.hidden = !hayAlgo;
  }

  return {
    raiz,
    actualizar(nuevo: EstadoMetaVisible | null): void {
      estado = nuevo;
      pintarLogros();
      pintarPersecucion();
      pintarBoton();
      pintarVisibilidad();
      ajustarReloj();
    },
    setContexto(nuevo: ContextoMeta): void {
      contexto = nuevo;
      pintarBoton();
    },
    activa(): boolean {
      return metaActiva() !== null;
    },
    pausada(): boolean {
      const activa = metaActiva();
      return activa !== null && activa.pausada_en !== null;
    },
  };
}
