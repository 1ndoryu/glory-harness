// Modal de configuración — sin cabecera ni pie.
// Las opciones se definen de forma centralizada en
// dominio/opciones.ts y se renderizan con componentes/formulario.ts.
// Los cambios se aplican al vuelo (guardado automático): no hay
// botones "guardar cambios"/"cancelar"; se cierra con click fuera
// del diálogo o con Escape.

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

  const fondo = el('div', 'modal-fondo');
  fondo.id = 'modal-fondo';
  fondo.hidden = true;

  const modal = el('div', 'modal');
  modal.setAttribute('role', 'dialog');
  modal.setAttribute('aria-modal', 'true');
  modal.setAttribute('aria-label', 'configuración');

  // ---- cuerpo: nav (paneles) + formularios centralizados ----
  const cuerpo = el('div', 'modal-cuerpo');
  const nav = el('nav', 'config-nav');
  nav.setAttribute('aria-label', 'categorías de configuración');

  const formularios = new Map<PanelId, ReturnType<typeof montarFormulario>>();
  const items = new Map<PanelId, HTMLDivElement>();
  /** Índice opción id → formulario (para asignarValor desde fuera). */
  const formularioDeOpcion = new Map<string, ReturnType<typeof montarFormulario>>();
  /** Selector de modelo del panel Modelo (se reemplaza la opción de lectura). */
  let selectorModeloApi: SelectorModeloApi | null = null;
  /** [109A-3] Panel "Memorias" (se consulta al backend al mostrarlo). */
  let panelMemorias: MemoriasPanel | null = null;
  /** Panel visible ahora mismo (para recargar al reabrir el modal). */
  let panelActivo: PanelId = FORMULARIO_CONFIGURACION[0].id as PanelId;

  function cambiarPanel(objetivo: PanelId): void {
    panelActivo = objetivo;
    items.forEach((item, clave) => item.classList.toggle('sel', clave === objetivo));
    formularios.forEach((f, clave) => {
      f.raiz.hidden = clave !== objetivo;
    });
    // Las memorias viven en el backend: se leen al abrir la sección (no al
    // construir el modal) y otra vez al reabrir, para ver lo que cambió.
    if (objetivo === 'memorias') panelMemorias?.refrescar();
  }

  // guardado automático: todo cambio se notifica al vuelo
  function notificarCambio(id: string, valor: string | boolean): void {
    // Persistencia real: el dueño (main.ts) guarda vía `configGuardar` del
    // adaptador. Aquí el valor queda en memoria, en el DOM (control
    // actualizado) y se propaga al dueño para sincronizar la vista.
    opts.onCambio?.(id, valor);
  }

  FORMULARIO_CONFIGURACION.forEach((seccion) => {
    const item = el('div', 'config-item');
    item.dataset.panel = seccion.id;
    item.textContent = seccion.etiqueta;
    item.addEventListener('click', () => cambiarPanel(seccion.id as PanelId));
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

  const contenedor = el('div', 'config-paneles');
  formularios.forEach((f) => contenedor.appendChild(f.raiz));

  cuerpo.appendChild(nav);
  cuerpo.appendChild(contenedor);
  modal.appendChild(cuerpo);
  fondo.appendChild(modal);

  // ---------- comportamiento ----------
  function abrir(): void {
    fondo.hidden = false;
    // Si el modal se cierra en "Memorias" y se reabre, se relista el ámbito
    // activo (pudo cambiar el proyecto o curar el agente entre medias).
    if (panelActivo === 'memorias') panelMemorias?.refrescar();
  }
  function cerrar(): void {
    fondo.hidden = true;
  }

  fondo.addEventListener('click', (e) => {
    if (e.target === fondo) cerrar();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !fondo.hidden) cerrar();
  });

  // panel inicial: primero visible
  const primera = FORMULARIO_CONFIGURACION[0].id as PanelId;
  cambiarPanel(primera);
  return {
    raiz: fondo,
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
