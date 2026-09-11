// [119A-2 F1] Menú contextual del grupo proyecto (Copy Path / Edit name /
// Remove con confirmación; F2 añade Open in Finder primero). Mecánica
// compartida de menu.ts (la misma que las celdas de conversación): sin
// menús paralelos ni HTML inyectado.
import type { Workspace } from '../dominio/tipos';
import {
  abrirMenuContextual,
  cerrarMenuActual,
  crearItemMenu,
  crearSeparadorMenu,
} from './menu';
import { el } from '../util/dom';
import { copiarAlPortapapeles } from '../util/portapapeles';

export interface MenuProyectoDeps {
  onRenombrar: (id: string, nombre: string) => void;
  onEliminar: (id: string) => void;
  /** [119A-2 F2] Abre la carpeta del proyecto en el Explorador. */
  onRevelar: (id: string) => void;
}

/** Convierte el nombre del grupo en un input inline para renombrar. */
export function empezarRenombrarProyecto(
  proyecto: Workspace,
  boton: HTMLButtonElement,
  nombreEl: HTMLElement,
  onRenombrar: (id: string, nombre: string) => void,
): void {
  cerrarMenuActual();
  const input = el('input') as HTMLInputElement;
  input.type = 'text';
  input.className = 'proyecto-grupo-nombre renombrar';
  input.value = proyecto.nombre;
  input.setAttribute('aria-label', 'nuevo nombre del proyecto');
  boton.replaceChild(input, nombreEl);

  const fin = (guardar: boolean) => {
    const nuevo = input.value.trim();
    if (guardar && nuevo && nuevo !== proyecto.nombre) {
      proyecto.nombre = nuevo;
      onRenombrar(proyecto.id, nuevo);
    }
    if (input.parentNode === boton) boton.replaceChild(nombreEl, input);
    nombreEl.textContent = proyecto.nombre;
  };
  input.addEventListener('click', (e) => e.stopPropagation());
  input.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter') fin(true);
    else if (e.key === 'Escape') fin(false);
  });
  input.addEventListener('blur', () => fin(true));
  input.focus();
  input.select();
}

export function abrirMenuProyecto(opts: {
  proyecto: Workspace;
  evento: MouseEvent;
  boton: HTMLButtonElement;
  nombreEl: HTMLElement;
  deps: MenuProyectoDeps;
  /** Segundo paso del borrado: pide confirmación explícita. */
  confirmar?: boolean;
}): void {
  const { proyecto, evento, boton, nombreEl, deps, confirmar } = opts;
  abrirMenuContextual({
    rect: new DOMRect(evento.clientX, evento.clientY, 0, 0),
    construir(m) {
      if (confirmar) {
        m.appendChild(
          crearItemMenu({
            texto: `Quitar «${proyecto.nombre}» del área`,
            onClick() {
              deps.onEliminar(proyecto.id);
            },
          }),
        );
        m.appendChild(
          crearItemMenu({
            texto: 'Cancelar',
            onClick() {
              cerrarMenuActual();
            },
          }),
        );
        return;
      }
      m.appendChild(
        crearItemMenu({
          texto: 'Copy Path',
          onClick() {
            cerrarMenuActual();
            void copiarAlPortapapeles(proyecto.ruta);
          },
        }),
      );
      // [119A-2 F2] Primero el atajo más usado; cierra el menú y delega al
      // orquestador (en web el transporte rechaza con aviso visible).
      m.appendChild(
        crearItemMenu({
          texto: 'Open in Finder',
          onClick() {
            cerrarMenuActual();
            deps.onRevelar(proyecto.id);
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: 'Edit name',
          onClick() {
            empezarRenombrarProyecto(proyecto, boton, nombreEl, deps.onRenombrar);
          },
        }),
      );
      m.appendChild(crearSeparadorMenu());
      m.appendChild(
        crearItemMenu({
          texto: 'Remove',
          onClick() {
            // Segundo paso: reabre el menú en modo confirmación (las
            // conversaciones huérfanas pasan a "Sin proyecto").
            cerrarMenuActual();
            abrirMenuProyecto({ proyecto, evento, boton, nombreEl, deps, confirmar: true });
          },
        }),
      );
    },
  });
}
