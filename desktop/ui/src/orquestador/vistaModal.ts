/* Modales de configuración y "Nuevo proyecto" + estado vista (extraído de
 * main.ts [089A-16 F1b]). El estado mutable (modelo/modo/razonamiento) vive
 * en el objeto `estado` que devuelve el módulo; el orquestador lo comparte
 * con la fábrica de paneles y el arranque. Sin importes del orquestador. */

import { invoke } from '@tauri-apps/api/core';
import { montarModalConfiguracion, type ModalConfiguracion } from '../componentes/modal';
import { montarModalProyecto, type ModalProyecto } from '../componentes/modalProyecto';
import type { PanelChat } from '../componentes/panelChat';
import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import type { ModoEjecucion } from '../componentes/entrada';

export interface EstadoVista {
  modelo: ModeloSeleccionado;
  modo: ModoEjecucion;
  razonamiento: string;
}

export interface SesionGuardadaVista {
  proveedor: string | null;
  modelo: string | null;
  modo: string | null;
  razonamiento: string | null;
  contextoMaxVentana: string | null;
  ganchoPreCompact: string | null;
  temaOscuro: string | null;
}

/** Estado inicial del modal: modelo, modo, razonamiento y tema. */
export interface VistaModalEstado {
  modeloInicial: ModeloSeleccionado;
  modoInicial: ModoEjecucion;
  razonamientoInicial: string;
  proveedores: ProveedorModelo[];
  claveTemaOscuro: string;
  etiquetasRazonamiento: Record<string, string>;
}

/** Entorno de ejecución que el modal consulta. */
export interface VistaModalEntorno {
  paneles: PanelChat[];
  usaReal: boolean;
  usaTauri: boolean;
}

/** Acciones de persistencia y vista delegadas al orquestador. */
export interface VistaModalAcciones {
  sincronizarPanelMeta: () => void;
  aplicarTemaOscuro: (activo: boolean) => void;
  configGuardar: (id: string, valor: string) => Promise<void>;
  configGuardarModelo: (nuevo: ModeloSeleccionado) => Promise<void>;
  guardarProyecto: (nombre: string, ruta: string) => Promise<void>;
  hayTurno: () => boolean;
  avisar: (texto: string, meta: string, detalle: string) => void;
  ponerBorradorPrincipal: () => void;
  activarPrincipal: () => void;
}

export interface VistaModalDeps
  extends VistaModalEstado, VistaModalEntorno, VistaModalAcciones {}

export interface VistaModal {
  estado: EstadoVista;
  modal: ModalConfiguracion;
  modalProyecto: ModalProyecto;
  /** Aplica la config persistida del backend (arranque real). */
  aplicarSesionGuardada: (sesion: SesionGuardadaVista) => void;
}

export function montarVistaModal(deps: VistaModalDeps): VistaModal {
  const estado: EstadoVista = {
    modelo: deps.modeloInicial,
    modo: deps.modoInicial,
    razonamiento: deps.razonamientoInicial,
  };

  const modal = montarModalConfiguracion({
    modelo: estado.modelo,
    proveedores: deps.proveedores,
    modo: estado.modo,
    razonamiento: estado.razonamiento,
    onCambio(id, valor) {
      if (id === 'modo') {
        estado.modo = valor as ModoEjecucion;
        deps.paneles.forEach((p) => p.setModo(estado.modo));
        deps.sincronizarPanelMeta();
      } else if (id === 'nivelRazonamiento') {
        estado.razonamiento = String(valor);
        deps.paneles.forEach((p) => p.setRazonamiento(estado.razonamiento));
      } else if (id === 'gancho_pre_compact') {
        // El JSON se valida en el boundary del backend; aquí solo se persiste
        // mediante la misma ruta que las demás opciones reales.
      } else if (id === 'contexto_max_ventana') {
        // [039A-3 P6] La ventana se persiste vía configGuardar (abajo); el
        // backend la consumirá al construir la sesión (inyección de
        // `contexto.max_ventana`, pendiente de P6 backend). No hay estado local
        // que actualizar: la fuente para el indicador es el ContextoDetalle.
      } else if (id === deps.claveTemaOscuro) {
        deps.aplicarTemaOscuro(valor === true || valor === 'true');
      }
      if (
        deps.usaReal &&
        (id === 'modo' ||
          id === 'nivelRazonamiento' ||
          id === 'contexto_max_ventana' ||
          id === 'gancho_pre_compact' ||
          id === deps.claveTemaOscuro)
      ) {
        void deps
          .configGuardar(id, valor === true ? '1' : String(valor))
          .catch((e: unknown) => deps.avisar(`no se pudo guardar ${id}: ${String(e)}`, '', ''));
      }
    },
    onModeloCambiado(nuevo) {
      estado.modelo = nuevo;
      deps.paneles.forEach((p) => p.setModelo(nuevo));
      if (deps.usaReal) {
        void deps
          .configGuardarModelo(nuevo)
          .catch((e: unknown) =>
            deps.avisar(`no se pudo guardar el modelo: ${String(e)}`, '', ''),
          );
      }
    },
  });

  // [069A-Proyectos] Modal "Nuevo proyecto" autocontenido.
  const modalProyecto = montarModalProyecto({
    invoke: deps.usaTauri ? invoke : undefined,
    onGuardar(nombre, ruta) {
      void (async () => {
        try {
          if (deps.hayTurno()) {
            deps.avisar(
              'hay un turno en curso',
              '',
              'espera a que termine para crear un proyecto',
            );
            return;
          }
          await deps.guardarProyecto(nombre, ruta);
          // onSesion / refrescarProyectos refrescarán sidebar + lista.
          deps.ponerBorradorPrincipal();
          deps.activarPrincipal();
        } catch (e: unknown) {
          deps.avisar(`no se pudo crear el proyecto: ${String(e)}`, '', '');
        }
      })();
    },
  });

  function aplicarSesionGuardada(sesion: SesionGuardadaVista): void {
    if (sesion.modelo) {
      estado.modelo = {
        proveedor: sesion.proveedor ?? estado.modelo.proveedor,
        modelo: sesion.modelo,
        nombre: sesion.modelo,
      };
      deps.paneles.forEach((p) => p.setModelo(estado.modelo));
      modal.setModelo(estado.modelo);
    }
    if (
      sesion.modo === 'predeterminado' ||
      sesion.modo === 'meta' ||
      sesion.modo === 'autonomo'
    ) {
      estado.modo = sesion.modo;
      deps.paneles.forEach((p) => p.setModo(estado.modo));
      modal.asignarValor('modo', estado.modo);
      deps.sincronizarPanelMeta();
    }
    if (sesion.razonamiento && deps.etiquetasRazonamiento[sesion.razonamiento]) {
      estado.razonamiento = sesion.razonamiento;
      deps.paneles.forEach((p) => p.setRazonamiento(sesion.razonamiento as string));
      modal.asignarValor('nivelRazonamiento', sesion.razonamiento);
    }
    // [039A-3 P6] Restaura la ventana de contexto persistida en el modal
    // (el backend la lee de config al construir la sesión; aquí solo se
    // refleja el valor guardado en el control del panel Contexto).
    if (sesion.contextoMaxVentana && Number(sesion.contextoMaxVentana) > 0) {
      modal.asignarValor('contexto_max_ventana', sesion.contextoMaxVentana);
    }
    if (sesion.ganchoPreCompact !== null) {
      modal.asignarValor('gancho_pre_compact', sesion.ganchoPreCompact);
    }
    if (sesion.temaOscuro !== null) {
      const activo = sesion.temaOscuro === '1' || sesion.temaOscuro === 'true';
      deps.aplicarTemaOscuro(activo);
      modal.asignarValor(deps.claveTemaOscuro, activo);
    }
  }

  return { estado, modal, modalProyecto, aplicarSesionGuardada };
}
