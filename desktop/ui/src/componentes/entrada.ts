// ============================================================
// Entrada: compositor con textarea autoexpandible (máx 5 líneas),
// barra de controles (modelo con menú contextual doble
// proveedor → modelo, razonamiento, modo) y botón único
// enviar/detener anclado a la derecha. Port 1:1 del mockup.
// [039A-3 P5] La entrada es duplicable (clase `.entrada`, no id). El
// panel principal usa la variante completa; el panel lateral usa la
// variante mínima (solo textarea + enviar/detener) porque hereda el
// modelo/modo del runtime compartido M1.
// ============================================================

import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import { icono, iconoHtml } from './iconos';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { montarSelectorModelo } from './selectorModelo';
import { el } from '../util/dom';

export type ModoEjecucion = 'predeterminado' | 'meta' | 'autonomo';

/** [039A-3 P5] Variante 'completa' (controles: modelo/razonamiento/modo) o
 * 'minima' (solo textarea + enviar/detener; hereda el runtime M1). */
export type VarianteEntrada = 'completa' | 'minima';

/** Modos reales del core (permiso.rs) con su etiqueta visible. */
export const MODOS_EJECUCION: Array<{ valor: ModoEjecucion; etiqueta: string }> = [
  { valor: 'predeterminado', etiqueta: 'Predeterminado' },
  { valor: 'meta', etiqueta: 'Meta' },
  { valor: 'autonomo', etiqueta: 'Autónomo' },
];

export const ETIQUETA_MODO: Record<ModoEjecucion, string> = {
  predeterminado: 'Predeterminado',
  meta: 'Meta',
  autonomo: 'Autónomo',
};

/** [039A-1 04-09 H7] Niveles de razonamiento (nivel_razonamiento del core). */
export const VALORES_RAZONAMIENTO: Array<{ valor: string; etiqueta: string }> = [
  { valor: 'low', etiqueta: 'Bajo' },
  { valor: 'medium', etiqueta: 'Medio' },
  { valor: 'high', etiqueta: 'Alto' },
];

/** Etiqueta visible de un nivel ('low' → 'Bajo'). */
export const ETIQUETA_RAZONAMIENTO: Record<string, string> = {
  low: 'Bajo',
  medium: 'Medio',
  high: 'Alto',
};

export interface Entrada {
  raiz: HTMLElement;
  /** Recalcula la altura del textarea (llamar tras montar al DOM). */
  medir(): void;
  /** Estado del botón enviar/detener. */
  setCorriendo(corriendo: boolean): void;
  /** Nombre del modelo mostrado. No-op en la variante mínima. */
  setModeloNombre(nombre: string): void;
  /** Modelo mostrado (reemplaza proveedor/modelo/nombre). No-op mínima. */
  setModelo(modelo: ModeloSeleccionado): void;
  /** [039A-1 04-09 H7] Nivel de razonamiento activo ('low'|'medium'|'high').
   * No-op en la variante mínima (lo fija el panel principal). */
  setRazonamientoValor(valor: string): void;
  /** Nivel de razonamiento activo. */
  getRazonamiento(): string;
  /** Etiqueta del modo. No-op en la variante mínima (lo fija el principal). */
  setModo(modo: ModoEjecucion): void;
  /** Pide foco al textarea (tras enviar). */
  enfocar(): void;
  /** Permite a la app conocer el estado interno del textarea. */
  getCorriendo(): boolean;
  getModo(): ModoEjecucion;
  /** [039A-3 P2] Pone el textarea en modo edición de un mensaje de usuario:
   * rellena su texto y muestra la barra "editando…" con cancelar. Al enviar
   * en este modo, `main.ts` hace rewind(`editar=true`) + reenvío. */
  ponerEnEdicion(id: string, texto: string): void;
  /** [039A-3 P2] Sale del modo edición (limpia barra) sin tocar el texto. */
  cancelarEnEdicion(): void;
  /** [039A-3 P2] `true` si hay una edición pendiente (mensaje objetivo). */
  enEdicion(): boolean;
  /** [039A-3 P2] Id del mensaje en edición (`null` si no). */
  edicionId(): string | null;
  /** [039A-3 P2] Texto actual del textarea (sin recortar). */
  getTexto(): string;
  /** [039A-3 P6] Actualiza el indicador circular de contexto (0-100). Con
   * `maxVentana` se muestra el % sobre el total; `null` deja el círculo vacío
   * (sin dato de contexto del `ContextoDetalle`). */
  setContexto(pct: number | null, maxVentana: number | null): void;
}

export interface EntradaOpciones {
  /** Prefijo de los ids internos (una instancia por panel). */
  idPrefijo: string;
  /** [039A-3 P5] 'completa' (default) o 'minima'. */
  variante?: VarianteEntrada;
  /** Solo variante 'completa': catálogo de proveedores del selector. */
  proveedores?: ProveedorModelo[];
  /** Modelo inicial (obligatorio en 'completa'; se ignora en 'minima'). */
  modeloActual?: ModeloSeleccionado;
  modo: ModoEjecucion;
  /** [039A-1 04-09 H7] Nivel de razonamiento inicial ('low'|'medium'|'high'). */
  razonamiento?: string;
  /** Se invoca al enviar un mensaje.
   *  [039A-3 P2] `editandoId` trae el id del mensaje de usuario reescrito
   *  (edición), o `null` para un mensaje nuevo. El consumidor decide si
   *  ejecutar rewind(editar=true) antes de enviar el texto. */
  onEnviar: (texto: string, editandoId?: string | null) => void;
  /** Se invoca al pulsar detener durante un turno. */
  onDetener: () => void;
  /** Se invoca al elegir un modelo del menú (solo variante 'completa'). */
  onModeloCambiado?: (modelo: ModeloSeleccionado) => void;
  /** Se invoca al cambiar el modo de ejecución desde la barra (completa). */
  onModoCambiado?: (modo: ModoEjecucion) => void;
  /** [039A-1 04-09 H7] Se invoca al elegir un nivel de razonamiento (completa). */
  onRazonamientoCambiado?: (razonamiento: string) => void;
}

export function montarEntrada(opts: EntradaOpciones): Entrada {
  const raiz = el('div', 'entrada');
  const variante: VarianteEntrada = opts.variante ?? 'completa';

  const caja = el('div', 'caja');
  const textarea = el('textarea') as HTMLTextAreaElement;
  textarea.id = `${opts.idPrefijo}-input`;
  textarea.rows = 1;
  textarea.placeholder = 'escribe un mensaje…';
  textarea.autocomplete = 'off';

  // ---- estado interno ----
  let corriendo = false;
  let modo: ModoEjecucion = opts.modo;

  // [039A-3 P5] En la variante mínima no se construyen controles de
  // modelo/razonamiento/modo: el runtime M1 es compartido y el panel
  // principal es la fuente de esos valores. Solo existe el botón
  // enviar/detener, dentro de `.controles`.
  const controles = el('div', 'controles');
  let btnModelo: HTMLButtonElement | null = null;
  let btnRazonamiento: HTMLButtonElement | null = null;
  let btnModo: HTMLButtonElement | null = null;
  let razonamiento = 'medium';
  const spanRazonamiento = el('span');
  const spanModo = el('span');
  let selectorModelo: ReturnType<typeof montarSelectorModelo> | null = null;

  if (variante === 'completa') {
    // control: modelo (selector compartido con el modal: menú doble)
    // El selector y el menú de modo comparten la mecánica de menu.ts
    // (solo hay un menú abierto a la vez), así que no hace falta cerrar
    // el otro antes de abrir: abrirMenuContextual cierra el previo solo.
    selectorModelo = montarSelectorModelo({
      proveedores: opts.proveedores ?? [],
      modelo: opts.modeloActual ?? { proveedor: '', modelo: '', nombre: '' },
      variante: 'barra',
      onCambio(modelo) {
        opts.onModeloCambiado?.(modelo);
      },
    });
    btnModelo = selectorModelo.raiz as HTMLButtonElement;
    btnModelo.id = `${opts.idPrefijo}-control-modelo`;

    // [039A-1 04-09 H7] control: razonamiento (menú Bajo/Medio/Alto, igual que
    // el de modo; el nivel elegido viaja al turno y se persiste en config).
    const razonamientoInicial = VALORES_RAZONAMIENTO.some(
      (r) => r.valor === opts.razonamiento,
    )
      ? (opts.razonamiento as string)
      : 'medium';
    razonamiento = razonamientoInicial;
    btnRazonamiento = el('button', 'control') as HTMLButtonElement;
    btnRazonamiento.id = `${opts.idPrefijo}-control-razonamiento`;
    btnRazonamiento.type = 'button';
    btnRazonamiento.title = 'nivel de razonamiento';
    spanRazonamiento.textContent = ETIQUETA_RAZONAMIENTO[razonamiento] ?? 'Medio';
    btnRazonamiento.appendChild(spanRazonamiento);
    btnRazonamiento.appendChild(icono('chevron-abajo', true));

    /** Cambia el nivel de razonamiento activo y notifica. */
    function seleccionarRazonamiento(valor: string): void {
      cerrarMenuActual();
      if (razonamiento === valor) return;
      razonamiento = valor;
      spanRazonamiento.textContent = ETIQUETA_RAZONAMIENTO[valor] ?? valor;
      opts.onRazonamientoCambiado?.(valor);
    }

    /** Abre el menú de razonamiento bajo el botón (check en el nivel activo). */
    function abrirMenuRazonamiento(): void {
      if (corriendo) return;
      if (!btnRazonamiento) return;
      const rect = btnRazonamiento.getBoundingClientRect();
      abrirMenuContextual({
        rect,
        construir(m) {
          VALORES_RAZONAMIENTO.forEach(({ valor, etiqueta }) => {
            m.appendChild(
              crearItemMenu({
                texto: etiqueta,
                marcado: valor === razonamiento,
                onClick() {
                  seleccionarRazonamiento(valor);
                },
              }),
            );
          });
        },
      });
    }

    btnRazonamiento.addEventListener('click', (e) => {
      e.stopPropagation();
      abrirMenuRazonamiento();
    });

    // control: modo (menú contextual: predeterminado / meta / autónomo)
    btnModo = el('button', 'control') as HTMLButtonElement;
    btnModo.id = `${opts.idPrefijo}-modo-control`;
    btnModo.type = 'button';
    btnModo.title = 'modo de ejecución';
    spanModo.textContent = ETIQUETA_MODO[opts.modo];
    btnModo.appendChild(spanModo);
    btnModo.appendChild(icono('chevron-abajo', true));

    controles.appendChild(btnModelo);
    controles.appendChild(btnRazonamiento);
    controles.appendChild(btnModo);
  }

  // botón único: enviar / detener (en completa es el 4º control de la barra;
  // en mínima es el único hijo de .controles, anclado a la derecha).
  const btnEnviar = el('button', 'btn-enviar') as HTMLButtonElement;
  btnEnviar.id = `${opts.idPrefijo}-btn-enviar`;
  btnEnviar.type = 'button';
  // [039A-3 P6] Indicador circular de contexto, justo antes del botón enviar.
  // Círculo sin relleno (stroke) cuya circunferencia se rellena según el
  // `ocupacion_pct` del `ContextoDetalle` (fuente única, decisión §2.10).
  // Estética monocromo: solo trazo, sin relleno.
  const indicador = el('button', 'ctx-indicador') as HTMLButtonElement;
  indicador.type = 'button';
  indicador.title = 'contexto 0%';
  indicador.setAttribute('aria-label', 'contexto 0%');
  const NS = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '0 0 24 24');
  svg.setAttribute('aria-hidden', 'true');
  const radio = 9;
  const perimetro = 2 * Math.PI * radio;
  const circuloFondo = document.createElementNS(NS, 'circle');
  circuloFondo.setAttribute('cx', '12');
  circuloFondo.setAttribute('cy', '12');
  circuloFondo.setAttribute('r', String(radio));
  circuloFondo.setAttribute('class', 'ctx-pista');
  const circulo = document.createElementNS(NS, 'circle');
  circulo.setAttribute('cx', '12');
  circulo.setAttribute('cy', '12');
  circulo.setAttribute('r', String(radio));
  circulo.setAttribute('class', 'ctx-lleno');
  // Rotado -90° para empezar arriba; dasharray = perimetro * pct.
  circulo.setAttribute('transform', 'rotate(-90 12 12)');
  svg.appendChild(circuloFondo);
  svg.appendChild(circulo);
  indicador.appendChild(svg);
  function pintarContexto(pct: number | null, maxVentana: number | null): void {
    // `pctF` (0-100) es la fuente del arco; sin dato queda 0 (círculo vacío).
    const pctF = pct === null ? 0 : Math.min(100, Math.max(0, pct));
    const off = perimetro * (1 - pctF / 100);
    circulo.setAttribute('stroke-dasharray', `${perimetro} ${perimetro}`);
    circulo.setAttribute('stroke-dashoffset', String(off));
    // Etiqueta: "0%" o "N%" (sin datos → título genérico con el total si hay).
    const rotulo =
      pct === null
        ? maxVentana !== null
          ? `contexto · hasta ${Math.round(maxVentana / 1000)}k`
          : 'contexto'
        : `contexto ${Math.round(pct)}%`;
    indicador.title = rotulo;
    indicador.setAttribute('aria-label', rotulo);
  }
  pintarContexto(null, null);
  // Sin acción: es informativo. El clic no debe enfocar el chat por error.
  indicador.addEventListener('click', (e) => e.stopPropagation());
  controles.appendChild(indicador);
  controles.appendChild(btnEnviar);

  caja.appendChild(textarea);
  caja.appendChild(controles);
  raiz.appendChild(caja);

  // ---------- [039A-3 P2] modo edición de mensaje ----------
  // Al editar un mensaje de usuario del historial, el textarea rellena su
  // texto y se muestra una barra "editando mensaje… [cancelar]". Al enviar
  // en este modo, main.ts hace rewind(editar=true) + reenvío (P2 §2.5).
  let edicion: { id: string } | null = null;

  const barraEdicion = el('div', 'editando-msg');
  barraEdicion.hidden = true;
  const edicionTexto = el('span', 'editando-msg-texto');
  const btnCancelarEdicion = el('button', 'editando-msg-cancelar') as HTMLButtonElement;
  btnCancelarEdicion.type = 'button';
  btnCancelarEdicion.textContent = 'cancelar';
  btnCancelarEdicion.title = 'cancelar edición';
  barraEdicion.appendChild(edicionTexto);
  barraEdicion.appendChild(btnCancelarEdicion);
  // La barra va entre el textarea y los controles.
  caja.insertBefore(barraEdicion, textarea);

  function pintarBarraEdicion(): void {
    if (edicion) {
      edicionTexto.textContent = 'editando mensaje';
      barraEdicion.hidden = false;
    } else {
      barraEdicion.hidden = true;
    }
  }

  function cancelarEnEdicion(): void {
    if (!edicion) return;
    edicion = null;
    pintarBarraEdicion();
    textarea.focus();
  }

  btnCancelarEdicion.addEventListener('click', (e) => {
    e.stopPropagation();
    cancelarEnEdicion();
  });

  // ---------- textarea autoexpandible (máx 5 líneas) ----------
  function ajustarEntrada(): void {
    textarea.style.height = 'auto';
    const lh = getComputedStyle(textarea).lineHeight;
    const linea = lh === 'normal' ? 18 : parseFloat(lh);
    const max = linea * 5;
    if (textarea.scrollHeight > max) {
      textarea.style.height = max + 'px';
      textarea.style.overflowY = 'auto';
    } else {
      textarea.style.height = textarea.scrollHeight + 'px';
      textarea.style.overflowY = 'hidden';
    }
  }
  textarea.addEventListener('input', ajustarEntrada);
  ajustarEntrada();

  // ---------- botón enviar/detener ----------
  function pintarBotonEnviar(): void {
    if (corriendo) {
      btnEnviar.innerHTML = iconoHtml('detener', true);
      btnEnviar.title = 'detener';
      btnEnviar.setAttribute('aria-label', 'detener');
    } else {
      btnEnviar.innerHTML = iconoHtml('flecha-arriba', true);
      btnEnviar.title = 'enviar';
      btnEnviar.setAttribute('aria-label', 'enviar');
    }
  }
  pintarBotonEnviar();

  function enviar(): void {
    if (corriendo) {
      opts.onDetener();
      return;
    }
    const texto = textarea.value.trim();
    if (!texto) return;
    textarea.value = '';
    ajustarEntrada();
    // [039A-3 P2] Al enviar desde modo edición, el destino queda marcado para
    // que main.ts ejecute rewind(editar=true)+reenvío antes de limpiarlo.
    const editandoId = edicion ? edicion.id : null;
    edicion = null;
    pintarBarraEdicion();
    opts.onEnviar(texto, editandoId);
  }

  btnEnviar.addEventListener('click', enviar);
  textarea.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault();
      enviar();
    }
  });

  // ---------- menú de modo (predeterminado / meta / autónomo) ----------
  // La mecánica del menú es la compartida de menu.ts; aquí solo el contenido.
  // [039A-3 P5] Solo existe en la variante completa: el modo es compartido
  // (M1) y la entrada mínima no puede cambiarlo.

  /** Cambia el modo activo (desde el menú o desde fuera) y notifica. */
  function seleccionarModo(nuevo: ModoEjecucion): void {
    cerrarMenuActual();
    if (modo === nuevo) return;
    modo = nuevo;
    if (spanModo) spanModo.textContent = ETIQUETA_MODO[modo];
    opts.onModoCambiado?.(modo);
  }

  /** Abre el menú de modo bajo el botón, con check en el modo activo. */
  function abrirMenuModo(): void {
    if (corriendo) return;
    if (!btnModo) return;
    const rect = btnModo.getBoundingClientRect();
    abrirMenuContextual({
      rect,
      construir(m) {
        MODOS_EJECUCION.forEach(({ valor, etiqueta }) => {
          m.appendChild(
            crearItemMenu({
              texto: etiqueta,
              marcado: valor === modo,
              onClick() {
                seleccionarModo(valor);
              },
            }),
          );
        });
      },
    });
  }

  btnModo?.addEventListener('click', (e) => {
    e.stopPropagation();
    abrirMenuModo();
  });

  // ---------- API pública ----------
  return {
    raiz,
    medir() {
      // La medición debe hacerse con el elemento en el DOM (scrollHeight
      // es 0 fuera de él); el constructor no puede hacerla aún.
      ajustarEntrada();
    },
    setCorriendo(v: boolean) {
      corriendo = v;
      if (btnModo) btnModo.disabled = v;
      if (btnRazonamiento) btnRazonamiento.disabled = v;
      selectorModelo?.setDeshabilitado(v);
      pintarBotonEnviar();
    },
    setModeloNombre(nombre: string) {
      if (!selectorModelo) return;
      const m = selectorModelo.getModelo();
      selectorModelo.setModelo({ ...m, nombre });
    },
    setModelo(modelo: ModeloSeleccionado) {
      selectorModelo?.setModelo(modelo);
    },
    setRazonamientoValor(valor: string) {
      const valido = VALORES_RAZONAMIENTO.some((r) => r.valor === valor)
        ? valor
        : 'medium';
      razonamiento = valido;
      spanRazonamiento.textContent = ETIQUETA_RAZONAMIENTO[valido] ?? valido;
    },
    getRazonamiento() {
      return razonamiento;
    },
    setModo(nuevo: ModoEjecucion) {
      modo = nuevo;
      if (spanModo) spanModo.textContent = ETIQUETA_MODO[modo];
    },
    enfocar() {
      textarea.focus();
    },
    getCorriendo() {
      return corriendo;
    },
    getModo() {
      return modo;
    },
    // [039A-3 P2] edición de mensaje: el textarea entra en modo edición
    // mostrando la barra; al enviar, main.ts decide rewind+reenvío.
    ponerEnEdicion(id: string, texto: string) {
      edicion = { id };
      textarea.value = texto;
      ajustarEntrada();
      pintarBarraEdicion();
      textarea.focus();
    },
    cancelarEnEdicion() {
      cancelarEnEdicion();
    },
    enEdicion() {
      return edicion !== null;
    },
    edicionId() {
      return edicion ? edicion.id : null;
    },
    getTexto() {
      return textarea.value;
    },
    setContexto(pct: number | null, maxVentana: number | null) {
      pintarContexto(pct, maxVentana);
    },
  };
}
