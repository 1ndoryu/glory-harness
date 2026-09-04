// ============================================================
// Entrada: compositor con textarea autoexpandible (máx 5 líneas),
// barra de controles (modelo con menú contextual doble
// proveedor → modelo, razonamiento, modo) y botón único
// enviar/detener anclado a la derecha. Port 1:1 del mockup.
// ============================================================

import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import { icono, iconoHtml } from './iconos';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { montarSelectorModelo } from './selectorModelo';
import { el } from '../util/dom';

export type ModoEjecucion = 'predeterminado' | 'meta' | 'autonomo';

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

export interface Entrada {
  raiz: HTMLElement;
  /** Recalcula la altura del textarea (llamar tras montar al DOM). */
  medir(): void;
  /** Estado del botón enviar/detener. */
  setCorriendo(corriendo: boolean): void;
  /** Nombre del modelo mostrado. */
  setModeloNombre(nombre: string): void;
  /** Modelo mostrado (reemplaza proveedor/modelo/nombre). */
  setModelo(modelo: ModeloSeleccionado): void;
  /** Etiqueta del nivel de razonamiento mostrado. */
  setRazonamiento(etiqueta: string): void;
  /** Etiqueta del modo. */
  setModo(modo: ModoEjecucion): void;
  /** Pide foco al textarea (tras enviar). */
  enfocar(): void;
  /** Permite a la app conocer el estado interno del textarea. */
  getCorriendo(): boolean;
  getModo(): ModoEjecucion;
}

export interface EntradaOpciones {
  proveedores: ProveedorModelo[];
  modeloActual: ModeloSeleccionado;
  modo: ModoEjecucion;
  /** Se invoca al enviar un mensaje. */
  onEnviar: (texto: string) => void;
  /** Se invoca al pulsar detener durante un turno. */
  onDetener: () => void;
  /** Se invoca al elegir un modelo del menú. */
  onModeloCambiado: (modelo: ModeloSeleccionado) => void;
  /** Se invoca al cambiar el modo de ejecución desde la barra. */
  onModoCambiado?: (modo: ModoEjecucion) => void;
}

export function montarEntrada(opts: EntradaOpciones): Entrada {
  const raiz = el('div');
  raiz.id = 'entrada';

  const caja = el('div', 'caja');
  const textarea = el('textarea') as HTMLTextAreaElement;
  textarea.id = 'input';
  textarea.rows = 1;
  textarea.placeholder = 'escribe un mensaje…';
  textarea.autocomplete = 'off';

  // ---- barra de controles ----
  const controles = el('div', 'controles');

  // control: modelo (selector compartido con el modal: menú doble)
  // El selector y el menú de modo comparten la mecánica de menu.ts
  // (solo hay un menú abierto a la vez), así que no hace falta cerrar
  // el otro antes de abrir: abrirMenuContextual cierra el previo solo.
  const selectorModelo = montarSelectorModelo({
    proveedores: opts.proveedores,
    modelo: opts.modeloActual,
    variante: 'barra',
    onCambio(modelo) {
      opts.onModeloCambiado(modelo);
    },
  });
  const btnModelo = selectorModelo.raiz as HTMLButtonElement;
  btnModelo.id = 'control-modelo';

  // control: razonamiento (estático en el boceto)
  const spanRazonamiento = el('span', 'control');
  spanRazonamiento.id = 'control-razonamiento';
  spanRazonamiento.title = 'nivel de razonamiento';
  spanRazonamiento.textContent = 'Medio';

  // control: modo (menú contextual: predeterminado / meta / autónomo)
  const btnModo = el('button', 'control') as HTMLButtonElement;
  btnModo.id = 'modo-control';
  btnModo.type = 'button';
  btnModo.title = 'modo de ejecución';
  const spanModo = el('span');
  spanModo.textContent = ETIQUETA_MODO[opts.modo];
  btnModo.appendChild(spanModo);
  btnModo.appendChild(icono('chevron-abajo', true));

  // botón único: enviar / detener
  const btnEnviar = el('button', 'btn-enviar') as HTMLButtonElement;
  btnEnviar.id = 'btn-enviar';
  btnEnviar.type = 'button';

  controles.appendChild(btnModelo);
  controles.appendChild(spanRazonamiento);
  controles.appendChild(btnModo);
  controles.appendChild(btnEnviar);

  caja.appendChild(textarea);
  caja.appendChild(controles);
  raiz.appendChild(caja);

  // ---------- estado interno ----------
  let corriendo = false;
  let modo: ModoEjecucion = opts.modo;

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
    opts.onEnviar(texto);
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

  /** Cambia el modo activo (desde el menú o desde fuera) y notifica. */
  function seleccionarModo(nuevo: ModoEjecucion): void {
    cerrarMenuActual();
    if (modo === nuevo) return;
    modo = nuevo;
    spanModo.textContent = ETIQUETA_MODO[modo];
    opts.onModoCambiado?.(modo);
  }

  /** Abre el menú de modo bajo el botón, con check en el modo activo. */
  function abrirMenuModo(): void {
    if (corriendo) return;
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

  btnModo.addEventListener('click', (e) => {
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
      btnModo.disabled = v;
      selectorModelo.setDeshabilitado(v);
      pintarBotonEnviar();
    },
    setModeloNombre(nombre: string) {
      const m = selectorModelo.getModelo();
      selectorModelo.setModelo({ ...m, nombre });
    },
    setModelo(modelo: ModeloSeleccionado) {
      selectorModelo.setModelo(modelo);
    },
    setRazonamiento(etiqueta: string) {
      spanRazonamiento.textContent = etiqueta;
    },
    setModo(nuevo: ModoEjecucion) {
      modo = nuevo;
      spanModo.textContent = ETIQUETA_MODO[modo];
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
  };
}
