// Piezas del panel derecho: toast global, Files, Cambios, Consola y la
// supresión de auto-apertura (extraído de `panelDerecho.ts` para no rebasar
// el techo de líneas; mismo cableado, sin cambios de comportamiento).

import { montarPanelFiles, type PanelFiles } from '../componentes/panelFiles';
import { montarPanelCambios, type PanelCambios } from '../componentes/panelCambios';
import { montarPanelConsola, type PanelConsola } from '../componentes/panelConsola';
import { montarToastGlobal, type ToastGlobal } from '../componentes/toastGlobal';
import {
  crearSupresionNavegador as crearSupresionAutoapertura,
  type SupresionNavegador,
} from './navegadorAuto';
import type { AdaptadorReal } from '../tauri/real';
import type { PanelChat } from '../componentes/panelChat';

/** Lo mínimo que el montaje pide al orquestador. */
export interface DepsPiezasPanelDerecho {
  adaptador: AdaptadorReal;
  panelActivo: () => PanelChat | null;
}

export interface PiezasPanelDerecho {
  toastGlobal: ToastGlobal;
  files: PanelFiles;
  cambios: PanelCambios;
  consola: PanelConsola;
  supresionConsola: SupresionNavegador;
}

export function montarPiezasPanelDerecho(deps: DepsPiezasPanelDerecho): PiezasPanelDerecho {
  // Files es un pane único estilo Synara: árbol a la izquierda + preview a la
  // derecha; el preview forma parte del mismo pane.
  const toastGlobal: ToastGlobal = montarToastGlobal();
  const files = montarPanelFiles({
    transporte: deps.adaptador.sesion.filesystem,
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
  });
  // [139A-7] Resumen batch si el transporte lo expone; si no, el panel usa
  // el fan-out clásico (sin simular el batch en web).
  const gitResumenBatch = deps.adaptador.sesion.filesystem.gitResumen;
  const git = montarPanelCambios({
    git: {
      estado: (ruta) => deps.adaptador.sesion.filesystem.gitEstado(ruta),
      repos: () => deps.adaptador.sesion.filesystem.gitRepos(),
      ...(gitResumenBatch ? { resumen: () => gitResumenBatch() } : {}),
    },
    cambios: {
      listar: (conv) => deps.adaptador.sesion.cambios(conv),
      rechazar: (conv, turno, ruta) => deps.adaptador.sesion.rechazarCambio(conv, turno, ruta),
    },
    convId: () => deps.panelActivo()?.conversaId ?? null,
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
    onToast(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
  });
  // [209A-1 F4-resto] La tab Consola: store por `id_ejecucion` + lista y visor.
  // Los errores del panel (portapapeles) van al toast global, como Files.
  // [219A-5 F4] × por fila + aviso honesto: `onMatar` devuelve si la mató
  // (`false` = ya había terminado); el panel avisa vía `onInfo` y el
  // `consola_fin` la congela en la vista. Los errores los muestra el panel
  // vía `onError` (aquí no se duplican).
  const consola = montarPanelConsola({
    onError(texto, detalle) {
      toastGlobal.mostrar(texto, detalle);
    },
    onInfo(texto) {
      toastGlobal.mostrar(texto);
    },
    onMatar(idEjecucion) {
      return deps.adaptador.sesion.matarConsola(idEjecucion);
    },
    // [219A-3] Puentes al backend para la sub-barra + backfill + stdin. Los
    // fallos los muestra el panel vía `onError` (aquí no se duplican).
    onSincronizar() {
      return deps.adaptador.sesion.listarConsolas();
    },
    onLeerSalida(idEjecucion) {
      return deps.adaptador.sesion.leerSalidaConsola(idEjecucion);
    },
    onEscribir(idEjecucion, texto) {
      return deps.adaptador.sesion.escribirConsola(idEjecucion, texto);
    },
    // [219A-4] [+ Nueva] de la cabecera: consola propia (shell, sin jaula).
    onCrear() {
      return deps.adaptador.sesion.crearConsola();
    },
  });
  // [209A-1 F3] Supresión de auto-apertura: la misma máquina pura que F1
  // (`crearSupresionNavegador` no sabe de navegadores: abrir/suprimir por
  // turno; aquí gobierna la tab Consola).
  const supresionConsola = crearSupresionAutoapertura();
  return { toastGlobal, files, cambios: git, consola, supresionConsola };
}
