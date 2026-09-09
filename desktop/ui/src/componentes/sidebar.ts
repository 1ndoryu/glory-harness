// Sidebar: botones de acción + lista de conversaciones + pie
// (configuración). Port 1:1 del mockup. La selección de
// conversación es visual; las acciones del menú contextual
// (cambiar nombre / archivar / copiar ID / eliminar) mutan el
// estado local y notifican al llamador (sin backend aún). El
// menú contextual REUTILIZA la mecánica compartida de menu.ts
// (la misma del selector de modelo y del modo de ejecución).

import type { Conversacion, Workspace } from '../dominio/tipos';
import { icono } from './iconos';
import { cerrarMenuActual } from './menu';
import { el } from '../util/dom';
import { crearCeldaConv, empezarRenombrarConv, type CeldaConvDeps } from './sidebarCeldas';

export interface Sidebar {
  raiz: HTMLElement;
  /** Marca una conversación como seleccionada (visual). */
  seleccionar(id: string): void;
  /**
   * Sustituye la lista completa (p. ej. tras listar el backend): repinta y
   * conserva la selección si el id sigue existiendo.
   */
  sustituir(conversaciones: Conversacion[]): void;
  /** [069A-Proyectos] Sustituye la lista de proyectos + proyecto activo. */
  sustituirProyectos(proyectos: Workspace[], activo: Workspace | null): void;
  /** [039A-3 P4] Pone el título de una conversación en edición inline (misma
   * mecánica que el ⋯ de su fila). No-op si el id no está en la lista. */
  empezarRenombrar(id: string): void;
  /** [039A-3 P4] Archiva/desarchiva una conversación (misma lógica que el ⋯
   * de su fila: muta, notifica al llamador y repinta). */
  archivarConversacion(id: string): void;
  /** [039A-3 P4] Elimina una conversación (misma lógica que el ⋯ de su fila). */
  eliminarConversacion(id: string): void;
}

/** Acciones de los botones superiores del nav. */
export type AccionNav = 'nueva' | 'agente' | 'flujo' | 'complementos' | 'navegador';

/** Datos que pinta la lista (conversaciones + proyectos). */
export interface SidebarDatos {
  conversaciones: Conversacion[];
  /** [069A-Proyectos] Lista de proyectos registrados. */
  proyectos: Workspace[];
  /** [069A-Proyectos] Proyecto activo actual (el que filtra conversaciones). */
  proyectoActivo: Workspace | null;
}

/** Acciones sobre conversaciones (fila y ⋯). */
export interface SidebarConversacion {
  /** Al pulsar una conversación (activa esa conversación). */
  onSeleccionar: (id: string) => void;
  /** Se invoca tras cambiar el nombre de una conversación. */
  onRenombrar: (id: string, titulo: string) => void;
  /** Se invoca al archivar (id, y si se desarchiva). */
  onArchivar: (id: string, archivada: boolean) => void;
  /** Se invoca al eliminar una conversación. */
  onEliminar: (id: string) => void;
}

/** Acciones de proyectos, panel lateral y nav superior. */
export interface SidebarProyectoNav {
  /** [069A-Proyectos] Se invoca al pulsar el botón + de proyectos. */
  onCrearProyecto?: () => void;
  /** [069A-Proyectos] Se invoca al elegir un proyecto del menú (ruta). */
  onSeleccionarProyecto?: (ruta: string) => void;
  /** [039A-3 P5] Consulta si se puede ofrecer "Abrir en panel lateral"
   * (el orquestador decide: <2 chats abiertos y ancho suficiente). */
  puedeAbrirLateral?: () => boolean;
  /** [039A-3 P5] Abre el id en un segundo panel lateral. */
  onAbrirEnLateral?: (id: string) => void;
  /** Al pulsar un botón superior del nav (nueva/agentes/flujo/complementos). */
  onAccionNav?: (accion: AccionNav) => void;
  abrirConfig: () => void;
}

export interface SidebarOpciones
  extends SidebarDatos, SidebarConversacion, SidebarProyectoNav {}

export function montarSidebar(opts: SidebarOpciones): Sidebar {
  const aside = el('aside');
  aside.id = 'sidebar';
  const conversaciones: Conversacion[] = [...opts.conversaciones];

  const marca = el('h1', 'marcaAplicacion');
  marca.textContent = 'Glory Harness';
  aside.appendChild(marca);

  // ---- botones de acción (nav) ----
  const nav = el('nav', 'nav-botones');
  nav.setAttribute('aria-label', 'crear y tipos de conversación');

  function botonNav(
    tooltip: string,
    etiqueta: string,
    iconoNombre: 'nueva' | 'agente' | 'flujo' | 'complementos' | 'navegador',
    onClick?: () => void,
  ): HTMLButtonElement {
    const b = el('button', 'nav-boton') as HTMLButtonElement;
    b.type = 'button';
    b.title = tooltip;
    b.appendChild(icono(iconoNombre, true));
    const span = el('span');
    span.textContent = etiqueta;
    b.appendChild(span);
    if (onClick) b.addEventListener('click', onClick);
    return b;
  }

  nav.appendChild(
    botonNav('nueva conversación', 'Nueva conversación', 'nueva', () =>
      opts.onAccionNav?.('nueva'),
    ),
  );
  nav.appendChild(
    botonNav('agentes', 'Agentes', 'agente', () => opts.onAccionNav?.('agente')),
  );
  nav.appendChild(
    botonNav('flujo', 'Flujo', 'flujo', () => opts.onAccionNav?.('flujo')),
  );
  nav.appendChild(
    botonNav('complementos', 'Complementos', 'complementos', () =>
      opts.onAccionNav?.('complementos'),
    ),
  );
  nav.appendChild(
    botonNav('navegador interno', 'Navegador', 'navegador', () =>
      opts.onAccionNav?.('navegador'),
    ),
  );

  // ---- [069A-Proyectos] Cabecera y grupos de conversaciones ----
  const proyectos: Workspace[] = [...opts.proyectos];
  let proyectoActivo: Workspace | null = opts.proyectoActivo;

  const progSec = el('div', 'prog-sec');
  const progHeader = el('div', 'prog-header');
  const progNombre = el('span', 'prog-nombre');
  progNombre.textContent = 'Proyectos';
  const progMas = el('button', 'prog-mas') as HTMLButtonElement;
  progMas.type = 'button';
  progMas.title = 'Crear proyecto';
  progMas.setAttribute('aria-label', 'Crear proyecto');
  progMas.appendChild(icono('mas', true));
  progMas.addEventListener('click', (e) => {
    e.stopPropagation();
    opts.onCrearProyecto?.();
  });
  progHeader.append(progNombre, progMas);
  progSec.appendChild(progHeader);

  // ---- lista de conversaciones agrupadas por proyecto ----
  const lista = el('div');
  lista.id = 'lista-conversaciones';

  const celdas = new Map<string, HTMLDivElement>();
  let activaId: string | null = null;
  // Las celdas viven en `sidebarCeldas`; las acciones que mutan estado local
  // (`seleccionar`, `alternarArchivado`, `eliminar`) se inyectan por deps
  // (declaraciones function = hoisted, seguras aquí).
  const depsCeldas: CeldaConvDeps = {
    onSeleccionar: opts.onSeleccionar,
    onRenombrar: opts.onRenombrar,
    onArchivar: opts.onArchivar,
    onEliminar: opts.onEliminar,
    puedeAbrirLateral: opts.puedeAbrirLateral,
    onAbrirEnLateral: opts.onAbrirEnLateral,
    seleccionar,
    alternarArchivado,
    eliminar,
  };
  // `conversaciones` es una copia local mutable que la sidebar reordena
  // (activar/archivar/eliminar); el llamador recibe los cambios por callbacks.

  function seleccionar(id: string): void {
    activaId = id;
    celdas.forEach((celda, clave) => {
      celda.classList.toggle('sel', clave === id);
    });
  }

  /** Convierte el título en un input inline para renombrar. */
  function empezarRenombrar(conv: Conversacion, celda: HTMLDivElement): void {
    empezarRenombrarConv(depsCeldas, conv, celda);
  }

  /** [039A-3 P4] Archiva/desarchiva por id (acción compartida: ⋯ de fila y de
   * cabecera). Muta la conversación, notifica al llamador y repinta. */
  function alternarArchivado(id: string): void {
    const conv = conversaciones.find((c) => c.id === id);
    if (!conv) return;
    conv.archivada = !conv.archivada;
    opts.onArchivar(conv.id, conv.archivada);
    pintarLista();
    cerrarMenuActual();
  }

  /** [039A-3 P4] Elimina por id (acción compartida: ⋯ de fila y de cabecera). */
  function eliminar(id: string): void {
    const conv = conversaciones.find((c) => c.id === id);
    if (!conv) return;
    celdas.delete(conv.id);
    conversaciones.splice(conversaciones.indexOf(conv), 1);
    opts.onEliminar(conv.id);
    pintarLista();
    cerrarMenuActual();
  }

  /** [039A-3 P4] Entra en modo renombrar la celda con ese id (si existe). */
  function renombrarPorId(id: string): void {
    const conv = conversaciones.find((c) => c.id === id);
    const celda = celdas.get(id);
    if (conv && celda) empezarRenombrar(conv, celda);
  }

  /** Crea una celda .conv (vive en `sidebarCeldas`). */
  function crearCelda(conv: Conversacion, dentroDeProyecto = false): HTMLDivElement {
    return crearCeldaConv(depsCeldas, conv, dentroDeProyecto);
  }

  function añadirGrupoProyecto(proyecto: Workspace, grupo: Conversacion[]): void {
    const cabecera = el('div', 'proyecto-grupo');
    const boton = el('button', 'proyecto-grupo-boton') as HTMLButtonElement;
    boton.type = 'button';
    boton.title = `Activar proyecto ${proyecto.nombre}`;
    boton.appendChild(icono('carpeta', true));
    const nombre = el('span', 'proyecto-grupo-nombre');
    nombre.textContent = proyecto.nombre;
    boton.appendChild(nombre);
    if (proyectoActivo?.id === proyecto.id) boton.classList.add('activo');
    boton.addEventListener('click', () => {
      if (proyectoActivo?.id !== proyecto.id) opts.onSeleccionarProyecto?.(proyecto.ruta);
    });
    cabecera.appendChild(boton);
    lista.appendChild(cabecera);

    grupo.filter((c) => !c.archivada).forEach((c) => {
      const d = crearCelda(c, true);
      celdas.set(c.id, d);
      lista.appendChild(d);
    });
    grupo.filter((c) => c.archivada).forEach((c) => {
      const d = crearCelda(c, true);
      celdas.set(c.id, d);
      lista.appendChild(d);
    });
  }

  function pintarLista(): void {
    lista.replaceChildren();
    celdas.clear();
    const porProyecto = new Map<string, Conversacion[]>();
    conversaciones.forEach((c) => {
      if (c.workspaceId) {
        const grupo = porProyecto.get(c.workspaceId) ?? [];
        grupo.push(c);
        porProyecto.set(c.workspaceId, grupo);
      }
    });

    proyectos.forEach((proyecto) => añadirGrupoProyecto(proyecto, porProyecto.get(proyecto.id) ?? []));

    const sinProyecto = conversaciones.filter((c) => !c.workspaceId);
    if (sinProyecto.length > 0) {
      const sep = el('div', 'conv-sep');
      sep.textContent = 'Sin proyecto';
      lista.appendChild(sep);
      sinProyecto.filter((c) => !c.archivada).forEach((c) => {
        const d = crearCelda(c);
        celdas.set(c.id, d);
        lista.appendChild(d);
      });
      sinProyecto.filter((c) => c.archivada).forEach((c) => {
        const d = crearCelda(c);
        celdas.set(c.id, d);
        lista.appendChild(d);
      });
    }
    if (activaId) seleccionar(activaId);
  }

  pintarLista();

  // ---- pie: configuración ----
  const pie = el('div', 'pie');
  const bConfig = el('button', 'pie-boton') as HTMLButtonElement;
  bConfig.id = 'btn-config';
  bConfig.type = 'button';
  bConfig.title = 'abrir configuración';
  bConfig.appendChild(icono('ajustes', true));
  const spanCfg = el('span');
  spanCfg.textContent = 'Configuración';
  bConfig.appendChild(spanCfg);
  bConfig.addEventListener('click', opts.abrirConfig);
  pie.appendChild(bConfig);

  aside.appendChild(nav);
  aside.appendChild(progSec);
  aside.appendChild(lista);
  aside.appendChild(pie);

  return {
    raiz: aside,
    seleccionar(id: string) {
      seleccionar(id);
    },
    sustituir(nuevas: Conversacion[]) {
      conversaciones.splice(0, conversaciones.length, ...nuevas.map((c) => ({ ...c })));
      if (activaId && !conversaciones.some((c) => c.id === activaId)) activaId = null;
      pintarLista();
    },
    // [039A-3 P4] Acciones por id compartidas con el ⋯ de la cabecera del
    // chat: la cabecera no sabe de filas; la sidebar es la fuente de la lista.
    empezarRenombrar(id: string) {
      renombrarPorId(id);
    },
    archivarConversacion(id: string) {
      alternarArchivado(id);
    },
    eliminarConversacion(id: string) {
      eliminar(id);
    },
    /** [069A-Proyectos] Sustituye lista de proyectos + actualiza el header. */
    sustituirProyectos(nuevos: Workspace[], activo: Workspace | null) {
      proyectos.splice(0, proyectos.length, ...nuevos);
      proyectoActivo = activo;
      pintarLista();
    },
  };
}
