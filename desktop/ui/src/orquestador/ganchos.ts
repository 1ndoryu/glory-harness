/* Ganchos del adaptador backend (extraídos de main.ts [089A-16 F1b]): sesión,
 * contexto, tools de navegador, conexión SSE y cambios de archivo. Todo el
 * estado compartido llega por `deps` (cierres perezosos cuando la pieza se
 * crea después); sin importes del orquestador. */

import type { PanelNavegador } from '../componentes/panelNavegador';
import type { PanelChat } from '../componentes/panelChat';
import type { CambioArchivoFiles } from '../componentes/panelFiles';
import type { HooksAdaptador, InfoSesion } from '../tauri/real';

export interface GanchosDeps {
  usaReal: boolean;
  sincronizarModeloDesdeSesion: (info: InfoSesion) => void;
  asignarWorkspace: (ws: string) => void;
  refrescarProyectos: () => Promise<void>;
  resincronizarSidebar: () => Promise<void>;
  paneles: () => PanelChat[];
  getNavegador: () => PanelNavegador;
  avisar: (texto: string, meta: string, detalle: string) => void;
  registrarCambioArchivo: (cambio: CambioArchivoFiles) => void;
}

export function crearGanchos(deps: GanchosDeps): HooksAdaptador {
  return {
    onSesion(info: InfoSesion) {
      deps.sincronizarModeloDesdeSesion(info);
      const ws = info.workspace;
      if (ws && ws !== '<desconocido>') deps.asignarWorkspace(ws);
      // [069A-Proyectos] Tras cambiar de workspace (proyecto), refrescar
      // la lista de proyectos + conversaciones. No esperar si falla.
      if (deps.usaReal) {
        void deps.refrescarProyectos().then(() => deps.resincronizarSidebar());
      }
    },
    // [039A-3 P6] El `ContextoDetalle`/`usage` del backend repinta el indicador
    // circular de TODOS los paneles con el % y la ventana real (fuente única).
    // Seguro: se ejecuta en runtime, cuando `panelesRegistrados` ya existe.
    onContexto(u) {
      deps.paneles().forEach((p) =>
        p.setContexto({
          pct: u.ocupacionPct,
          maxVentana: u.maxVentana,
          reservaSalida: u.reservaSalida,
          totalEntrada: u.totalEntrada,
        }),
      );
    },
    // [069A-1 F6] Refleja las tools de navegador del agente en el panel UI,
    // incluyendo captura base64 para mostrar la imagen.
    onToolNavegador(ev) {
      const navegador = deps.getNavegador();
      if (ev.ok && ev.url) navegador.fijarURL(ev.url);
      // [069A-1 F6] Mostrar captura base64 si viene en el evento
      if (ev.accion === 'capturar' && ev.captura_base64) {
        navegador.actualizarCaptura(ev.captura_base64);
      }
    },
    // [069A-2 F4] Estado de conexión SSE (solo modo web): visible, nunca
    // silencioso. Seguro: `avisoGlobal` solo corre en runtime.
    onConexion(estado, detalle) {
      if (estado !== 'en-linea')
        deps.avisar(`backend web: ${estado}`, '', detalle ?? '');
    },
    // [089A-12] Cambios de archivos del agente → preview integrado en Files.
    // Seguro: corre en runtime, cuando `files` ya existe.
    onCambioArchivo(cambio) {
      deps.registrarCambioArchivo(cambio);
    },
  };
}
