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
  /** [129A-7 F3] Refresco en vivo del panel Cambios (ruta + diff vivo). */
  registrarCambioVivo: (ruta: string, diff: string | null) => void;
  /** [129A-10 F1] Auto-apertura de la tab por el agente (cierre perezoso de
   * runtime, como `verEnCambios`): 'suprimida' = el usuario la cerró a mitad
   * de turno y solo se avisa por toast. */
  abrirNavegadorPorAgente: () => 'abierta' | 'ya' | 'suprimida';
  /** [129A-10 F2] Muestra un archivo en Files (cierre perezoso, runtime). */
  mostrarArchivoEnFiles: (ruta: string) => void;
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
      // [129A-10 F1] "Ve a esa página" visible: al abrir/navegar ok se abre
      // la tab SIN robar el foco (`abrirTab` solo conmuta la tab visible).
      // click/capturar/js no reabren; el cierre intencional a mitad de turno
      // se respeta (solo toast) hasta el próximo turno.
      if (ev.ok && (ev.accion === 'abrir' || ev.accion === 'navegar')) {
        const destino = ev.url ? ` a ${ev.url}` : '';
        const r = deps.abrirNavegadorPorAgente();
        if (r === 'abierta') deps.avisar(`el agente abrió el navegador${destino}`, '', '');
        else if (r === 'suprimida') {
          deps.avisar(`el agente navega${destino} (cerraste la tab: se reabre en el próximo turno)`, '', '');
        }
      }
    },
    // [069A-2 F4] Estado de conexión SSE (solo modo web): visible, nunca
    // silencioso. Seguro: `avisoGlobal` solo corre en runtime.
    onConexion(estado, detalle) {
      if (estado !== 'en-linea')
        deps.avisar(`backend web: ${estado}`, '', detalle ?? '');
    },
    // [089A-12] Cambios de archivos del agente → preview integrado en Files.
    // [129A-7 F3] …y refresco en vivo del panel Cambios (diff por ruta).
    // Seguro: corre en runtime, cuando `files` ya existe.
    onCambioArchivo(cambio) {
      deps.registrarCambioArchivo(cambio);
      deps.registrarCambioVivo(cambio.ruta, cambio.diff);
    },
    // [129A-10 F2] El agente muestra un archivo: se abre Files y se
    // previsualiza la ruta (vista, sin editar), con aviso visible.
    // Seguro: corre en runtime (cierre perezoso, como `verEnCambios`).
    onMostrarArchivo(ev) {
      deps.mostrarArchivoEnFiles(ev.ruta);
      deps.avisar(`el agente muestra ${ev.ruta}`, '', '');
    },
  };
}
