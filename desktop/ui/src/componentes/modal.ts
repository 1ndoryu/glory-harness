// Página de ajustes a pantalla completa (119A-4 F1; antes modal).
// Las opciones se definen de forma centralizada en
// dominio/opciones.ts y se renderizan con componentes/formulario.ts.
// Los cambios se aplican al vuelo (guardado automático): no hay
// botones "guardar cambios"/"cancelar"; se vuelve con «← Volver a la
// app» o con Escape. La interfaz ModalConfiguracion no cambia: el
// cableado (vistaModal, crearPanel, main) sigue intacto.

import { FORMULARIO_CONFIGURACION, OPCIONES_MODELO } from '../dominio/opciones';
import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import type { ModoEjecucion } from './entrada';
import { montarFormulario } from './formulario';
import { montarMemorias, type MemoriasDeps, type MemoriasPanel } from './memorias';
import { montarSelectorModelo, type SelectorModeloApi } from './selectorModelo';
import { el } from '../util/dom';

export interface ModalConfiguracion {
  raiz: HTMLElement;
  abrir(): void;
  cerrar(): void;
  /** Actualiza el valor de una opción por id y refresca su control. */
  asignarValor(id: string, valor: string | boolean): void;
  /** Reemplaza el modelo mostrado por el selector (sin notificar). */
  setModelo(modelo: ModeloSeleccionado): void;
}

export interface ModalOpciones {
  modelo: ModeloSeleccionado;
  /** Catálogo de proveedores para el selector de modelo del panel Modelo. */
  proveedores: ProveedorModelo[];
  /** Modo activo actual (se presiembra en el formulario). */
  modo: ModoEjecucion;
  /** Nivel de razonamiento actual ('low' | 'medium' | 'high'). */
  razonamiento: string;
  /** Se invoca al cambiar cualquier opción (guardado automático). */
  onCambio?: (id: string, valor: string | boolean) => void;
  /** Se invoca al elegir un modelo en el selector (panel Modelo). */
  onModeloCambiado?: (modelo: ModeloSeleccionado) => void;
  /** [109A-3] Acciones del panel "Memorias" (proyecto activo). */
  memoria: MemoriasDeps;
}

type PanelId = (typeof FORMULARIO_CONFIGURACION)[number]['id'];

export function montarModalConfiguracion(opts: ModalOpciones): ModalConfiguracion {
  // Presiembra el estado real de la app en las opciones (antes de construir).
  // 'proveedorModelo' no se presiembra: su control se sustituye por el
  // selector de modelo (montarSelectorModelo recibe opts.modelo directo).
  OPCIONES_MODELO.opciones.forEach((o) => {
    if (o.id === 'nivelRazonamiento') o.valor = opts.razonamiento;
  });
  FORMULARIO_CONFIGURACION.forEach((sec) => {
    sec.grupos.forEach((g) => {
      g.opciones.forEach((o) => {
        if (o.id === 'modo') o.valor = opts.modo;
      });
    });
  });

  const pagina = el('div', 'ajustes-pagina');
  pagina.id = 'ajustes-pagina';
  pagina.hidden = true;
  pagina.setAttribute('aria-label', 'ajustes');

  // ---- cabecera: volver + título + buscador ----
  const cabecera = el('header', 'ajustes-cabecera');
  const btnVolver = el('button', 'ajustes-volver') as HTMLButtonElement;
  btnVolver.type = 'button';
  btnVolver.textContent = '← Volver a la app';
  btnVolver.addEventListener('click', () => cerrar());
  const titulo = el('div', 'ajustes-titulo');
  titulo.textContent = 'Ajustes';
  const buscar = el('input', 'ajustes-buscar input-texto') as HTMLInputElement;
  buscar.type = 'search';
  buscar.placeholder = 'Buscar ajustes';
  buscar.setAttribute('aria-label', 'buscar ajustes');
  buscar.spellcheck = false;
  buscar.addEventListener('input', () => filtrarBusqueda(buscar.value));
  cabecera.appendChild(btnVolver);
  cabecera.appendChild(titulo);
  cabecera.appendChild(buscar);

  // ---- cuerpo: nav (secciones) + paneles ----
  const cuerpo = el('div', 'ajustes-cuerpo');
  const nav = el('nav', 'ajustes-nav');
  nav.setAttribute('aria-label', 'secciones de ajustes');

  const formularios = new Map<PanelId, ReturnType<typeof montarFormulario>>();
  const items = new Map<PanelId, HTMLDivElement>();
  /** Índice opción id → formulario (para asignarValor desde fuera). */
  const formularioDeOpcion = new Map<string, ReturnType<typeof montarFormulario>>();
  /** Selector de modelo del panel Modelo (se reemplaza la opción de lectura). */
  let selectorModeloApi: SelectorModeloApi | null = null;
  /** [109A-3] Panel "Memorias" (se consulta al backend al mostrarlo). */
  let panelMemorias: MemoriasPanel | null = null;
  /** Panel visible ahora mismo (para recargar al reabrir la página). */
  let panelActivo: PanelId = FORMULARIO_CONFIGURACION[0].id as PanelId;

  function cambiarPanel(objetivo: PanelId): void {
    panelActivo = objetivo;
    items.forEach((item, clave) => item.classList.toggle('sel', clave === objetivo));
    formularios.forEach((f, clave) => {
      f.raiz.hidden = clave !== objetivo;
    });
    // Las memorias viven en el backend: se leen al abrir la sección (no al
    // construir la página) y otra vez al reabrir, para ver lo que cambió.
    if (objetivo === 'memorias') panelMemorias?.refrescar();
  }

  /** Texto buscable por opción (etiqueta + nota + id + grupo + sección). */
  const indiceBusqueda = new Map<string, string>();
  FORMULARIO_CONFIGURACION.forEach((sec) => {
    if (sec.grupos.length === 0) {
      indiceBusqueda.set(`seccion:${sec.id}`, normalizar(sec.etiqueta));
    }
    sec.grupos.forEach((g) => {
      g.opciones.forEach((o) => {
        indiceBusqueda.set(
          o.id,
          normalizar(`${o.etiqueta} ${o.nota ?? ''} ${o.id} ${g.titulo} ${sec.etiqueta}`),
        );
      });
    });
  });

  function normalizar(s: string): string {
    return s
      .toLowerCase()
      .normalize('NFD')
      .replace(/[\u0300-\u036f]/g, '');
  }

  /** Fila visible que contiene un nodo con data-opcion (campo o fila-dato). */
  function filaDe(nodo: HTMLElement): HTMLElement {
    if (nodo.classList.contains('fila-dato')) return nodo;
    return (nodo.closest('.campo') ?? nodo) as HTMLElement;
  }

  /** Filtra opciones por texto; con texto vacío restaura la vista por panel. */
  function filtrarBusqueda(texto: string): void {
    const consulta = normalizar(texto.trim());
    if (!consulta) {
      items.forEach((item) => {
        item.hidden = false;
      });
      formularios.forEach((f, clave) => {
        f.raiz.hidden = clave !== panelActivo;
        f.raiz
          .querySelectorAll('.campo[hidden], .fila-dato[hidden], section.grupo[hidden]')
          .forEach((n) => {
            (n as HTMLElement).hidden = false;
          });
      });
      return;
    }
    formularios.forEach((f, clave) => {
      // El control reemplazado del panel Modelo conserva data-opcion.
      f.raiz.querySelectorAll('[data-opcion]').forEach((n) => {
        const nodo = n as HTMLElement;
        const id = nodo.dataset.opcion ?? '';
        filaDe(nodo).hidden = !(indiceBusqueda.get(id) ?? '').includes(consulta);
      });
      let visible = false;
      const grupos = f.raiz.querySelectorAll('section.grupo');
      grupos.forEach((g) => {
        const gs = g as HTMLElement;
        const conResultados =
          gs.querySelector('.campo:not([hidden]), .fila-dato:not([hidden])') !== null;
        gs.hidden = !conResultados;
        if (conResultados) visible = true;
      });
      // Sección sin grupos (Memorias): filtra por su propia etiqueta.
      if (grupos.length === 0) {
        visible = (indiceBusqueda.get(`seccion:${clave}`) ?? '').includes(consulta);
      }
      f.raiz.hidden = !visible;
      const item = items.get(clave);
      if (item) item.hidden = !visible;
    });
  }

  // guardado automático: todo cambio se notifica al vuelo
  function notificarCambio(id: string, valor: string | boolean): void {
    // Persistencia real: el dueño (main.ts) guarda vía `configGuardar` del
    // adaptador. Aquí el valor queda en memoria, en el DOM (control
    // actualizado) y se propaga al dueño para sincronizar la vista.
    opts.onCambio?.(id, valor);
  }

  FORMULARIO_CONFIGURACION.forEach((seccion) => {
    const item = el('div', 'ajustes-item');
    item.dataset.panel = seccion.id;
    item.textContent = seccion.etiqueta;
    item.addEventListener('click', () => {
      // Elegir sección sale del modo búsqueda (vista por panel).
      if (buscar.value) {
        buscar.value = '';
        filtrarBusqueda('');
      }
      cambiarPanel(seccion.id as PanelId);
    });
    nav.appendChild(item);
    items.set(seccion.id as PanelId, item);

    const f = montarFormulario({
      grupos: seccion.grupos,
      onCambio: notificarCambio,
    });
    f.raiz.dataset.panel = seccion.id;
    formularios.set(seccion.id as PanelId, f);
    seccion.grupos.forEach((g) => g.opciones.forEach((o) => formularioDeOpcion.set(o.id, f)));

    // El panel Modelo usa el selector de modelo compartido (menú doble) en
    // lugar de la caja de solo lectura del esquema ('proveedorModelo').
    if (seccion.id === 'modelo') {
      const selModelo = montarSelectorModelo({
        proveedores: opts.proveedores,
        modelo: opts.modelo,
        variante: 'modal',
        onCambio: (modelo) => {
          opts.onModeloCambiado?.(modelo);
        },
      });
      f.reemplazarControl('proveedorModelo', selModelo.raiz);
      selectorModeloApi = selModelo;
    }

    // [109A-3] Sección "Memorias": panel custom (no hay formulario que
    // generar; la sección se declaró sin grupos a propósito).
    if (seccion.id === 'memorias') {
      panelMemorias = montarMemorias(opts.memoria);
      f.raiz.appendChild(panelMemorias.raiz);
    }
  });

  const contenedor = el('div', 'ajustes-paneles');
  formularios.forEach((f) => contenedor.appendChild(f.raiz));

  cuerpo.appendChild(nav);
  cuerpo.appendChild(contenedor);
  pagina.appendChild(cabecera);
  pagina.appendChild(cuerpo);

  // ---------- comportamiento ----------
  function abrir(): void {
    pagina.hidden = false;
    // Cada apertura parte de la vista limpia (sin búsqueda residual).
    buscar.value = '';
    filtrarBusqueda('');
    // Si la página se cierra en "Memorias" y se reabre, se relista el ámbito
    // activo (pudo cambiar el proyecto o curar el agente entre medias).
    if (panelActivo === 'memorias') panelMemorias?.refrescar();
  }
  function cerrar(): void {
    pagina.hidden = true;
    buscar.blur();
  }

  document.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape' || pagina.hidden) return;
    // Escape con búsqueda activa la limpia; sin búsqueda vuelve a la app.
    if (document.activeElement === buscar && buscar.value) {
      buscar.value = '';
      filtrarBusqueda('');
    } else {
      cerrar();
    }
  });

  // panel inicial: primero visible
  const primera = FORMULARIO_CONFIGURACION[0].id as PanelId;
  cambiarPanel(primera);
  return {
    raiz: pagina,
    abrir,
    cerrar,
    asignarValor(id: string, valor: string | boolean) {
      const f = formularioDeOpcion.get(id);
      if (f) f.asignarValor(id, valor);
    },
    setModelo(modelo: ModeloSeleccionado) {
      selectorModeloApi?.setModelo(modelo);
    },
  };
}
