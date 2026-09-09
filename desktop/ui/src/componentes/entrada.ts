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
import { icono } from './iconos';
import { crearBarrasEntrada } from './entradaBarras';
import { crearIndicadorContexto } from './entradaContexto';
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

  // Indicador circular de contexto (arco SVG + detalle hover): módulo propio.
  // El botón enviar/detener y las barras del compositor viven en
  // `entradaBarras.ts` (se crean tras ensamblar la caja, más abajo).
  const ctx = crearIndicadorContexto();



  raiz.appendChild(selectorWorkspaceBox);
  caja.appendChild(textarea);
  caja.appendChild(controles);
  raiz.appendChild(caja);

  // Barras del compositor (edición, badge, autoexpand, botón enviar). Se crean
  // aquí porque insertan nodos en la caja ya ensamblada, en el mismo orden.
  const barras = crearBarrasEntrada({ idPrefijo: opts.idPrefijo, caja, textarea });
  controles.appendChild(ctx.indicador);
  controles.appendChild(barras.btnEnviar);



  function enviar(): void {
    if (corriendo) {
      opts.onDetener();
      return;
    }
    const texto = textarea.value.trim();
    if (!texto) return;
    textarea.value = '';
    barras.ajustarEntrada();
    // [039A-3 P2] Al enviar desde modo edición, el destino queda marcado para
    // que main.ts ejecute rewind(editar=true)+reenvío antes de limpiarlo.
    const editandoId = barras.tomarEdicionId();
    // [seleccionar] El adjunto (elemento del navegador) viaja antepuesto al
    // texto y se limpia tras enviarlo (un uso por mensaje).
    const textoFinal = barras.textoConAdjunto(texto);
    barras.quitarAdjunto();
    opts.onEnviar(textoFinal, editandoId);
  }

  barras.btnEnviar.addEventListener('click', enviar);
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
      barras.ajustarEntrada();
    },
    setCorriendo(v: boolean) {
      corriendo = v;
      if (btnModo) btnModo.disabled = v;
      if (btnRazonamiento) btnRazonamiento.disabled = v;
      selectorModelo?.setDeshabilitado(v);
      barras.pintarBotonEnviar(corriendo);
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
      barras.ponerEnEdicion(id, texto);
    },
    cancelarEnEdicion() {
      barras.cancelarEnEdicion();
    },
    enEdicion() {
      return barras.enEdicion();
    },
    edicionId() {
      return barras.edicionId();
    },
    getTexto() {
      return textarea.value;
    },
    setContexto(estado: EstadoContexto) {
      ctx.pintarContexto(estado);
    },
    setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null) {
      sw.setWorkspaces(workspaces, seleccionadoId);
    },
    setConversaId(id: string | null) {
      conversaId = id;
      pintarVisibilidadWorkspace();
    },
    adjuntarElemento(elem: ElementoSeleccionado) {
      barras.adjuntarElemento(elem);
    },
    getElementoPendiente() {
      return barras.getElementoPendiente();
    },
  };
}
