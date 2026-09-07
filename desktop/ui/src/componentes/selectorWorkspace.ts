// ============================================================
// Selector de área de trabajo (menú contextual, mismo estilo que
// .menu-ctx). Botón con el nombre del proyecto activo; al
// pulsarlo abre un menú contextual con cada workspace registrado.
// SIEMPRE hay un workspace activo (el agente trabaja siempre en
// algún área). Se usa dentro del cuadro centrado del composer
// cuando la conversación es nueva.
// ============================================================

import type { Workspace } from '../dominio/tipos';
import { icono } from './iconos';
import { el } from '../util/dom';
import { abrirMenuContextual, crearItemMenu } from './menu';

export interface SelectorWorkspaceApi {
  raiz: HTMLElement;
  setWorkspaces(workspaces: Workspace[], seleccionadoId: string | null): void;
  getSeleccionado(): string | null;
}

export interface SelectorWorkspaceOpciones {
  workspaces: Workspace[];
  seleccionadoId?: string | null;
  onCambio: (workspaceId: string | null) => void;
}

export function montarSelectorWorkspace(opts: SelectorWorkspaceOpciones): SelectorWorkspaceApi {
  let seleccionadoId: string | null = opts.seleccionadoId ?? null;
  let workspaces: Workspace[] = opts.workspaces;

  // [069A-8] El backend es la fuente de verdad del workspace activo
  // (`area_activa` auto-registra la ruta activa si falta). No hay fallback
  // a `workspaces[0]`: si `activa` es null, el selector muestra
  // "Seleccionar…" y el usuario debe elegir explícitamente.

  const raiz = el('button', 'selector-workspace-boton') as HTMLButtonElement;
  raiz.type = 'button';
  raiz.title = 'elegir área de trabajo';

  const spanNombre = el('span', 'nombre');
  spanNombre.textContent = labelDeId(seleccionadoId);
  raiz.appendChild(spanNombre);
  raiz.appendChild(icono('chevron-abajo', true));

  function labelDeId(id: string | null): string {
    if (id === null) return 'Seleccionar…';
    const ws = workspaces.find((w) => w.id === id);
    return ws ? ws.nombre : 'Seleccionar…';
  }

  function abrirMenu(): void {
    const rect = raiz.getBoundingClientRect();
    abrirMenuContextual({
      rect,
      construir(m) {
        workspaces.forEach((ws) => {
          m.appendChild(
            crearItemMenu({
              texto: ws.nombre,
              marcado: ws.id === seleccionadoId,
              onClick() {
                seleccionadoId = ws.id;
                spanNombre.textContent = ws.nombre;
                opts.onCambio(ws.id);
              },
            }),
          );
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
    setWorkspaces(workspacesNuevos: Workspace[], id: string | null) {
      workspaces = workspacesNuevos;
      // [069A-8] Backend es fuente de verdad: respetar `id` tal cual, sin
      // fallback al primero. Si el usuario eliminó el workspace activo, el
      // selector mostrará "Seleccionar…".
      seleccionadoId = id;
      spanNombre.textContent = labelDeId(id);
    },
    getSeleccionado() {
      return seleccionadoId;
    },
  };
}