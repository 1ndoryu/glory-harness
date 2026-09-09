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

import type {
  ElementoSeleccionado,
  ModeloSeleccionado,
  ProveedorModelo,
  Workspace,
} from '../dominio/tipos';
import { icono, ponerIcono } from './iconos';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { montarSelectorModelo } from './selectorModelo';
import { montarSelectorWorkspace } from './selectorWorkspace';
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

/** [039A-3 P6+] Estado de contexto del indicador circular y su menú hover.
 * Campos en `null` = sin dato (la UI los omite o muestra "sin datos"). */
export interface EstadoContexto {
  /** Ocupación 0-100 de la ventana efectiva (o `null` sin dato). */
  pct: number | null;
  /** Ventana máxima configurada (p. ej. 150000). */
  maxVentana: number | null;
  /** Reserva de salida que se descuenta de la ventana para el cálculo. */
  reservaSalida: number | null;
  /** Tokens totales de entrada del último desglose de contexto. */
  totalEntrada: number | null;
}

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
  /** [039A-3 P6] Actualiza el indicador circular de contexto y su detalle de
   * hover. Con `estado.pct` se pinta el arco (0-100); `null` deja el círculo
   * vacío. El resto de campos alimenta el pequeño menú `.ctx-detalle` que
   * aparece al poner el cursor sobre el círculo (uso de la ventana). */
  setContexto(estado: EstadoContexto): void;
  /** Actualiza las áreas disponibles sin reconstruir el composer. */
  setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null): void;
  /** Oculta/muestra el selector de workspace (conversación nueva vs existente). */
  setConversaId(id: string | null): void;
  /** [seleccionar] Adjunta un elemento del navegador como badge pendiente:
   * se muestra sobre el textarea y se antepone al próximo mensaje enviado. */
  adjuntarElemento(elem: ElementoSeleccionado): void;
  /** [seleccionar] Badge pendiente actual (`null` si no hay). */
  getElementoPendiente(): ElementoSeleccionado | null;
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
  /** Áreas disponibles para la conversación nueva. */
  workspaces?: Workspace[];
  /** Área destino seleccionada; `null` = conversación sin proyecto. */
  workspaceSeleccionadoId?: string | null;
  /** Se invoca al cambiar el área destino de la conversación nueva. */
  onWorkspaceCambiado?: (workspaceId: string | null) => void;
}

export function montarEntrada(opts: EntradaOpciones): Entrada {
  const raiz = el('div', 'entrada');
  const variante: VarianteEntrada = opts.variante ?? 'completa';

  // [069A-8] Selector de área de trabajo como menú contextual (mismo estilo
  // que .menu-ctx), dentro de un cuadro centrado que SOLO se muestra cuando
  // la conversación es nueva (conversaId === null).
  let conversaId: string | null = null;
  const selectorWorkspaceBox = el('div', 'selector-workspace-box');
  selectorWorkspaceBox.hidden = false; // visible por defecto (nueva)
  const sw = montarSelectorWorkspace({
    workspaces: opts.workspaces ?? [],
    seleccionadoId: opts.workspaceSeleccionadoId ?? null,
    onCambio(id) {
      opts.onWorkspaceCambiado?.(id);
    },
  });
  selectorWorkspaceBox.appendChild(sw.raiz);

  function pintarVisibilidadWorkspace(): void {
    selectorWorkspaceBox.hidden = conversaId !== null;
  }

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
  indicador.setAttribute('aria-label', 'contexto');
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

  // [039A-3 P6+] Estado completo de contexto (fuente única para el arco y el
  // menú hover). El `title` nativo se omite: el detalle lo da el menú custom.
  let estadoCtx: EstadoContexto = {
    pct: null,
    maxVentana: null,
    reservaSalida: null,
    totalEntrada: null,
  };
  let detalleCtx: HTMLElement | null = null;

  function pintarContexto(estado: EstadoContexto): void {
    estadoCtx = estado;
    // `pctF` (0-100) es la fuente del arco; sin dato queda 0 (círculo vacío).
    const pctF = estado.pct === null ? 0 : Math.min(100, Math.max(0, estado.pct));
    const off = perimetro * (1 - pctF / 100);
    circulo.setAttribute('stroke-dasharray', `${perimetro} ${perimetro}`);
    circulo.setAttribute('stroke-dashoffset', String(off));
    // Rótulo accesible: "0%" o "N%" (sin datos → total configurado si hay).
    const rotulo =
      estado.pct === null
        ? estado.maxVentana !== null
          ? `contexto · hasta ${Math.round(estado.maxVentana / 1000)}k`
          : 'contexto'
        : `contexto ${Math.round(estado.pct)}%`;
    indicador.setAttribute('aria-label', rotulo);
    // Si el detalle está visible (cursor sobre el círculo), refrescar datos.
    if (detalleCtx) {
      const d = detalleCtx;
      while (d.firstChild) d.removeChild(d.firstChild);
      construirDetalle(d);
      posicionarDetalle(d, indicador.getBoundingClientRect());
    }
  }

  /** Número exacto con separador de miles (es). */
  function numeroExacto(n: number): string {
    return Number.isFinite(n) ? String(Math.round(n).toLocaleString('es')) : '—';
  }

  /** Filas etiqueta/valor del uso de la ventana (contenido del hover). */
  function filasDetalleContexto(): Array<[string, string]> {
    const filas: Array<[string, string]> = [];
    if (estadoCtx.pct === null) {
      filas.push(
        estadoCtx.maxVentana !== null
          ? ['configurada', numeroExacto(estadoCtx.maxVentana)]
          : ['uso', 'sin datos'],
      );
      return filas;
    }
    if (estadoCtx.maxVentana !== null) {
      const efectiva = Math.max(0, estadoCtx.maxVentana - (estadoCtx.reservaSalida ?? 0));
      const usados = Math.round((estadoCtx.pct / 100) * efectiva);
      filas.push([
        'usados',
        `${numeroExacto(usados)} de ${numeroExacto(estadoCtx.maxVentana)} (${Math.round(estadoCtx.pct)}%)`,
      ]);
    } else {
      filas.push(['usados', `${Math.round(estadoCtx.pct)}%`]);
    }
    if (estadoCtx.reservaSalida !== null) {
      filas.push(['reserva de salida', numeroExacto(estadoCtx.reservaSalida)]);
    }
    if (estadoCtx.totalEntrada !== null) {
      filas.push(['entrada del turno', numeroExacto(estadoCtx.totalEntrada)]);
    }
    return filas;
  }

  /** Rellena el contenido del detalle (título + filas etiqueta/valor). */
  function construirDetalle(d: HTMLElement): void {
    const titulo = el('div', 'ctx-detalle-titulo');
    titulo.textContent = 'ventana de contexto';
    d.appendChild(titulo);
    for (const [etiqueta, valor] of filasDetalleContexto()) {
      const fila = el('div', 'ctx-detalle-fila');
      const e = el('span', 'etiqueta');
      e.textContent = etiqueta;
      const v = el('span', 'valor');
      v.textContent = valor;
      fila.appendChild(e);
      fila.appendChild(v);
      d.appendChild(fila);
    }
  }

  /** Posiciona el detalle junto al indicador, con vuelco al viewport. */
  function posicionarDetalle(d: HTMLElement, rect: DOMRect): void {
    const margen = 8;
    const vw = window.innerWidth;
    const vh = window.innerHeight;
    const altura = d.offsetHeight;
    const ancho = d.offsetWidth;
    const espacioAbajo = vh - rect.bottom - margen;
    let top =
      espacioAbajo < altura && rect.top > altura + margen ? rect.top - altura - 4 : rect.bottom + 4;
    if (top < margen) top = margen;
    if (top + altura > vh - margen) top = vh - altura - margen;
    d.style.top = top + 'px';
    let left = rect.left;
    if (left + ancho > vw - margen) left = Math.max(margen, vw - ancho - margen);
    d.style.left = left + 'px';
  }

  function alClicFueraDetalle(): void {
    cerrarDetalle();
  }
  function alTeclaDetalle(e: KeyboardEvent): void {
    if (e.key === 'Escape') cerrarDetalle();
  }
  function alCambioLayoutDetalle(): void {
    cerrarDetalle();
  }

  function cerrarDetalle(): void {
    if (!detalleCtx) return;
    detalleCtx.remove();
    detalleCtx = null;
    document.removeEventListener('click', alClicFueraDetalle, true);
    document.removeEventListener('keydown', alTeclaDetalle, true);
    window.removeEventListener('resize', alCambioLayoutDetalle);
    window.removeEventListener('scroll', alCambioLayoutDetalle, true);
    window.removeEventListener('blur', alCambioLayoutDetalle);
  }

  /** Abre el pequeño menú de detalle bajo el círculo (hover/enfoque). */
  function abrirDetalle(): void {
    cerrarDetalle();
    const d = el('div', 'ctx-detalle');
    d.style.visibility = 'hidden';
    construirDetalle(d);
    document.body.appendChild(d);
    posicionarDetalle(d, indicador.getBoundingClientRect());
    detalleCtx = d;
    document.addEventListener('click', alClicFueraDetalle, true);
    document.addEventListener('keydown', alTeclaDetalle, true);
    window.addEventListener('resize', alCambioLayoutDetalle);
    window.addEventListener('scroll', alCambioLayoutDetalle, true);
    window.addEventListener('blur', alCambioLayoutDetalle);
    d.style.visibility = '';
  }

  pintarContexto({ pct: null, maxVentana: null, reservaSalida: null, totalEntrada: null });
  // El detalle es informativo: se abre con el cursor encima del círculo (o con
  // el foco por teclado) y se cierra al salir. El clic no enfoca el chat.
  indicador.addEventListener('mouseenter', () => abrirDetalle());
  indicador.addEventListener('mouseleave', () => cerrarDetalle());
  indicador.addEventListener('focus', () => abrirDetalle());
  indicador.addEventListener('blur', () => cerrarDetalle());
  indicador.addEventListener('click', (e) => e.stopPropagation());
  controles.appendChild(indicador);
  controles.appendChild(btnEnviar);

  raiz.appendChild(selectorWorkspaceBox);
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

  // ---------- [seleccionar] badge de elemento del navegador ----------
  // El usuario eligió un elemento de la página (hover+clic en el panel
  // navegador). Se muestra como badge sobre el textarea y, al enviar, se
  // antepone al texto un descriptor para que el modelo lo reciba y pueda
  // actuar sobre él (p. ej. con la tool `navegador_reflejo`).
  let adjunto: ElementoSeleccionado | null = null;

  const barraAdjunto = el('div', 'adjunto-badge');
  barraAdjunto.hidden = true;
  const adjuntoInfo = el('span', 'adjunto-badge-info');
  const btnQuitarAdjunto = el('button', 'adjunto-badge-quitar') as HTMLButtonElement;
  btnQuitarAdjunto.type = 'button';
  btnQuitarAdjunto.textContent = 'quitar';
  btnQuitarAdjunto.title = 'quitar elemento seleccionado';
  barraAdjunto.appendChild(adjuntoInfo);
  barraAdjunto.appendChild(btnQuitarAdjunto);
  // El badge va sobre el textarea (primera fila de la caja del compositor).
  caja.insertBefore(barraAdjunto, caja.firstChild);

  function pintarAdjunto(): void {
    if (!adjunto) {
      barraAdjunto.hidden = true;
      return;
    }
    const textoRecorte = adjunto.texto ? ` · “${adjunto.texto.slice(0, 40)}”` : '';
    adjuntoInfo.textContent = `elemento: ${adjunto.etiqueta}${textoRecorte}`;
    barraAdjunto.title = `selector: ${adjunto.selector}\npágina: ${adjunto.pagina}`;
    barraAdjunto.hidden = false;
  }

  function quitarAdjunto(): void {
    adjunto = null;
    pintarAdjunto();
  }

  btnQuitarAdjunto.addEventListener('click', (e) => {
    e.stopPropagation();
    quitarAdjunto();
  });

  /** Al enviar, antepone el descriptor del elemento adjunto al texto. */
  function textoConAdjunto(texto: string): string {
    if (!adjunto) return texto;
    const base = adjunto.texto ? adjunto.texto.trim().slice(0, 200) : '';
    const contexto = base ? ` ("${base}")` : '';
    const descriptor =
      `[elemento de la página ${adjunto.pagina} — selector CSS: ${adjunto.selector}` +
      ` — etiqueta: ${adjunto.etiqueta}${contexto}]`;
    return `${descriptor}\n\n${texto}`;
  }

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
      ponerIcono(btnEnviar, 'detener', true);
      btnEnviar.title = 'detener';
      btnEnviar.setAttribute('aria-label', 'detener');
    } else {
      ponerIcono(btnEnviar, 'flecha-arriba', true);
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
    // [seleccionar] El adjunto (elemento del navegador) viaja antepuesto al
    // texto y se limpia tras enviarlo (un uso por mensaje).
    const textoFinal = textoConAdjunto(texto);
    quitarAdjunto();
    opts.onEnviar(textoFinal, editandoId);
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
    setContexto(estado: EstadoContexto) {
      pintarContexto(estado);
    },
    setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null) {
      sw.setWorkspaces(workspaces, seleccionadoId);
    },
    setConversaId(id: string | null) {
      conversaId = id;
      pintarVisibilidadWorkspace();
    },
    adjuntarElemento(elem: ElementoSeleccionado) {
      adjunto = elem;
      pintarAdjunto();
      textarea.focus();
    },
    getElementoPendiente() {
      return adjunto;
    },
  };
}
