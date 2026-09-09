// Selector de modelo reutilizable (menú contextual doble:
// proveedor → modelo). Lo usan la barra de entrada (#entrada)
// y el modal de configuración para editar "Proveedor / modelo".
// El menú se ancla a document.body (posición fixed), igual que
// los demás .menu-ctx, para no quedar recortado por contenedores
// con overflow. Variante "campo" = caja con borde (modal);
// variante "barra" = botón sin borde de la entrada.

import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import { icono } from './iconos';
import { el } from '../util/dom';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu } from './menu';
import { altoVentana, anchoVentana } from '../plataforma/ventana';

export type VarianteSelector = 'barra' | 'modal';

export interface SelectorModeloApi {
  raiz: HTMLElement;
  /** Reemplaza el modelo mostrado (sin notificar). */
  setModelo(modelo: ModeloSeleccionado): void;
  /** Estado actual del selector. */
  getModelo(): ModeloSeleccionado;
  /** Habilita/deshabilita abrir el menú (y refleja :disabled). */
  setDeshabilitado(deshabilitado: boolean): void;
}

export interface SelectorModeloOpciones {
  proveedores: ProveedorModelo[];
  modelo: ModeloSeleccionado;
  /** 'barra' = estilo de la entrada; 'modal' = caja con borde del modal. */
  variante: VarianteSelector;
  /** true bloquea abrir el menú (p. ej. durante un turno). */
  deshabilitado?: boolean;
  onCambio: (modelo: ModeloSeleccionado) => void;
}

export function montarSelectorModelo(opts: SelectorModeloOpciones): SelectorModeloApi {
  // 'barra' → botón sin borde de la entrada (#entrada .controles .control);
  // 'modal' → caja con borde a lo ancho dentro del formulario.
  const raiz = el('button', 'selector-modelo') as HTMLButtonElement;
  raiz.type = 'button';
  raiz.title = 'elegir modelo';
  if (opts.variante === 'barra') {
    raiz.classList.add('control');
  } else {
    // NOTA: clase propia 'selector-modal' (no 'modal'): 'modal' es también la
    // clase del diálogo en modal.css y estiraría el botón (height/column).
    raiz.classList.add('selector-modal');
  }
  if (opts.deshabilitado) raiz.disabled = true;

  const spanNombre = el('span', 'nombre');
  spanNombre.textContent = opts.modelo.nombre;
  raiz.appendChild(spanNombre);
  raiz.appendChild(icono('chevron-abajo', true));

  let modeloActual: ModeloSeleccionado = opts.modelo;

  // ---------- menú contextual doble (proveedor → modelo) ----------
  // La mecánica del menú (anclado con vuelco, cierre por click-fuera/Escape/
  // resize/blur) es la compartida de menu.ts; aquí solo se construye el
  // contenido (proveedores + submenús hover) sobre ella.

  function seleccionarModelo(proveedor: string, modelo: string, nombre: string): void {
    modeloActual = { proveedor, modelo, nombre };
    spanNombre.textContent = nombre;
    cerrarMenuActual();
    opts.onCambio(modeloActual);
  }

  /** Abre el menú doble (proveedores → submenús de modelos) bajo el selector. */
  function abrirMenu(): void {
    if (opts.deshabilitado) return;
    const rect = raiz.getBoundingClientRect();
    abrirMenuContextual({
      rect,
      construir(m) {
        const grupos: { g: HTMLElement; sub: HTMLElement }[] = [];
        opts.proveedores.forEach((prov) => {
          const g = el('div', 'menu-grupo');
          const item = crearItemMenu({
            texto: prov.etiqueta,
            marcado: prov.id === modeloActual.proveedor,
            conFlecha: true,
          });
          const sub = el('div', 'menu-sub');
          prov.modelos.forEach((mod) => {
            sub.appendChild(
              crearItemMenu({
                texto: mod.nombre,
                marcado: mod.modelo === modeloActual.modelo,
                onClick() {
                  seleccionarModelo(prov.id, mod.modelo, mod.nombre);
                },
              }),
            );
          });
          g.appendChild(item);
          g.appendChild(sub);
          m.appendChild(g);
          grupos.push({ g, sub });
        });

        // ---- anclar cada submenú a la fila de su proveedor (con vuelco) ----
        const margen = 8;
        const vw = anchoVentana();
        const vh = altoVentana();
        const ocultarSub = (sub: HTMLElement) => {
          sub.style.display = 'none';
          sub.style.top = '';
          sub.style.bottom = '';
          sub.classList.remove('voltear');
        };
        grupos.forEach(({ g, sub }) => {
          g.style.position = 'relative';
          g.addEventListener('mouseenter', () => {
            grupos.forEach(({ sub: s }) => ocultarSub(s));
            sub.style.display = 'block';
            const fila = g.getBoundingClientRect();
            const subAlto = sub.offsetHeight;
            const subAncho = sub.offsetWidth;
            if (fila.left + fila.width + subAncho > vw - margen) {
              sub.classList.add('voltear');
            }
            if (fila.bottom + subAlto > vh - margen && fila.top - subAlto > margen) {
              sub.style.top = 'auto';
              sub.style.bottom = '0';
            }
          });
          g.addEventListener('mouseleave', () => ocultarSub(sub));
        });
      },
    });
  }

  raiz.addEventListener('click', (e) => {
    e.stopPropagation();
    abrirMenu();
  });

  return {
    raiz,
    setModelo(modelo: ModeloSeleccionado) {
      modeloActual = modelo;
      spanNombre.textContent = modelo.nombre;
    },
    getModelo() {
      return modeloActual;
    },
    setDeshabilitado(deshabilitado: boolean) {
      opts.deshabilitado = deshabilitado;
      raiz.disabled = deshabilitado;
      if (deshabilitado) cerrarMenuActual();
    },
  };
}
