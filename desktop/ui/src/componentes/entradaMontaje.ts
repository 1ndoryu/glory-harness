// Montaje de la entrada (split 089A-16 F2-resto): la factoría `montarEntrada`
// vive aquí; `entrada.ts` queda como barrel (tipos + montaje). Sin ciclos:
// solo importa de dominio, utilidades y componentes hoja.
//
// Compositor con textarea autoexpandible (máx 5 líneas), barra de controles
// y botón único enviar/detener. [039A-3 P5] Duplicable (clase `.entrada`):
// el panel principal usa la variante completa y el lateral la mínima.

import type {
  ElementoSeleccionado,
  ModeloSeleccionado,
  Workspace,
} from '../dominio/tipos';
import { icono } from './iconos';
import { crearEntradaComandos } from './entradaComandos';
import { crearBarrasEntrada } from './entradaBarras';
import { crearIndicadorContexto } from './entradaContexto';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { montarSelectorModelo } from './selectorModelo';
import { montarSelectorWorkspace } from './selectorWorkspace';
import { el } from '../util/dom';
import {
  ETIQUETA_MODO,
  ETIQUETA_RAZONAMIENTO,
  MODOS_EJECUCION,
  VALORES_RAZONAMIENTO,
} from './entradaTipos';
import type {
  Entrada,
  EntradaOpciones,
  EstadoContexto,
  ModoEjecucion,
  VarianteEntrada,
} from './entradaTipos';
import type { ComandoProyecto } from '../dominio/comandosSlash';

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
  // [109A-4] Comandos `/` del área activa (los integrados son catálogo fijo).
  let comandosProyecto: ComandoProyecto[] = opts.comandosProyecto ?? [];
  // Último estado de contexto pintado (lo lee el comando `/contexto`).
  let ultimoContexto: EstadoContexto | null = null;

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

  // [109A-4 F2] Menú `/`: el compositor solo aporta el ancla y decide qué
  // pasa al elegir. El filtrado, la navegación y el pintado viven en
  // `entradaComandos.ts` + `menuComandos.ts`.
  const comandos = crearEntradaComandos({
    textarea,
    comandosProyecto: () => comandosProyecto,
    onElegir(comando, admiteArgumentos) {
      if (corriendo) return;
      textarea.value = `/${comando.nombre}${admiteArgumentos ? ' ' : ''}`;
      barras.ajustarEntrada();
      textarea.focus();
      // Con argumentos el usuario sigue escribiendo; sin ellos se ejecuta ya
      // por el MISMO canal que un envío normal (el turno resuelve el comando).
      if (!admiteArgumentos) enviar();
    },
  });

  function enviar(): void {
    if (corriendo) {
      opts.onDetener();
      return;
    }
    comandos.cerrar();
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
    // El menú `/` tiene prioridad sobre enviar/navegar mientras está abierto.
    if (comandos.alTecla(e)) return;
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
      ultimoContexto = estado;
      ctx.pintarContexto(estado);
    },
    getContexto() {
      return ultimoContexto;
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
    setComandosProyecto(lista: ComandoProyecto[]) {
      comandosProyecto = lista;
      comandos.cerrar();
    },
  };
}
