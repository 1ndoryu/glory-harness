/* Arranque de la UI (extraído de main.ts [089A-16 F1b]): estado inicial de la
 * sidebar, reloj del turno, historial inicial (mock) y arranque real
 * (sesión + config guardada + última conversación). Se ejecuta una vez,
 * después del montaje del DOM. Sin importes del orquestador. */

import { historialEjemplo } from '../datos/historialEjemplo';
import { renderizarBloque } from '../componentes/mensajes';
import { CLAVE_ANCHO, CLAVE_COLAPSADA } from './barraLateral';
import type { BarraLateral } from './barraLateralTipos';
import type { PanelChat } from '../componentes/panelChat';
import type { PanelMeta } from '../componentes/panelMeta';
import type { Conversacion } from '../dominio/tipos';
import { leerSidebar, type PersistenciaDeps } from './persistencia';
import type { SesionGuardadaVista } from './vistaModal';

/** Núcleo de arranque: montaje, paneles y meta. */
export interface ArranqueNucleo {
  cuerpo: HTMLElement;
  persistencia: PersistenciaDeps;
  barraLateral: BarraLateral;
  pintarToggleDerecho: () => void;
  restaurarPanelDerecho: () => Promise<void>;
  paneles: PanelChat[];
  panelMeta: PanelMeta;
  principal: PanelChat;
}

/** Entorno de ejecución (real/mock, flags y tema). */
export interface ArranqueEntorno {
  usaReal: boolean;
  usaMock: boolean;
  usaTauri: boolean;
  baseApi: string | null;
  modoTexto: string;
  claveTemaOscuro: string;
}

/** Consultas de estado que el arranque lee. */
export interface ArranqueConsultas {
  hayTurno: () => boolean;
  panelActivo: () => PanelChat | null;
  usoUltimoTurno: () => { tokensPrompt: number; tokensComplecion: number };
  getConversaciones: () => Conversacion[];
}

/** Acciones de sesión y sincronización que el arranque dispara. */
export interface ArranqueAcciones {
  asegurarSesion: () => Promise<unknown>;
  configLeer: (id: string) => Promise<string | null>;
  aplicarSesionGuardada: (sesion: SesionGuardadaVista) => void;
  sincronizarPanelMeta: () => void;
  resincronizarSidebar: () => Promise<void>;
  seleccionarSidebar: (id: string) => void;
  activarPanel: (panel: PanelChat | null) => void;
}

export interface ArranqueDeps
  extends ArranqueNucleo, ArranqueEntorno, ArranqueConsultas, ArranqueAcciones {}

export function ejecutarArranque(deps: ArranqueDeps): void {
  // Estado inicial de la sidebar (ancho/colapso persistidos + selección).
  const ancho = leerSidebar(deps.persistencia, CLAVE_ANCHO);
  if (ancho) {
    const n = Number(ancho);
    if (Number.isFinite(n)) deps.cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
  }
  const col = leerSidebar(deps.persistencia, CLAVE_COLAPSADA);
  deps.barraLateral.fijarAbierta(col !== '1');
  deps.barraLateral.pintarSidebar();
  // [089A-2] El toggle derecho arranca en "mostrar" (panel oculto, sin tabs).
  deps.pintarToggleDerecho();

  // Recalcular alturas del textarea tras montar al DOM.
  deps.paneles.forEach((p) => p.medir());
  deps.panelMeta.medir();
  deps.sincronizarPanelMeta();

  // Reloj del turno (panelMeta global): refleja el turno del panel que lanzó.
  window.setInterval(() => {
    if (!deps.usaReal || !deps.hayTurno()) return;
    const p = deps.panelActivo();
    const inicio = p?.getInicioTurno() ?? null;
    if (inicio === null) return;
    deps.panelMeta.setTiempo((Date.now() - inicio) / 1000);
    const u = deps.usoUltimoTurno();
    deps.panelMeta.setTokens(u.tokensPrompt + u.tokensComplecion);
  }, 1000);

  // ---------- Historial inicial (mock) ----------
  if (deps.usaMock) {
    historialEjemplo().forEach((bloque) => {
      deps.principal.mensajes.appendChild(renderizarBloque(bloque));
    });
    // La primera conversación activa queda como conversaId del principal
    // (para el ⋯ de cabecera y la selección de la sidebar).
    const candidata = deps.getConversaciones().find((c) => !c.archivada);
    if (candidata) {
      void deps.principal.cargarConversacion(candidata.id);
      deps.seleccionarSidebar(candidata.id);
    }
    deps.activarPanel(deps.principal);
    void deps.restaurarPanelDerecho();
  } else if (deps.usaReal) {
    deps.principal.avisoLocal(
      'Sesión real del núcleo (sin simulación)',
      deps.modoTexto,
      'escribe y envía',
    );
  }

  // ---------- Arranque real: sesión + lista + última conversación ----------
  if (deps.usaReal) {
    void (async () => {
      try {
        await deps.asegurarSesion();
        const [provG, modG, modoG, razG, anchoG, colG, ctxG, ganchoG, temaG] = await Promise.all([
          deps.configLeer('proveedor'),
          deps.configLeer('modelo'),
          deps.configLeer('modo'),
          deps.configLeer('nivelRazonamiento'),
          deps.configLeer(CLAVE_ANCHO),
          deps.configLeer(CLAVE_COLAPSADA),
          deps.configLeer('contexto_max_ventana'),
          deps.configLeer('gancho_pre_compact'),
          deps.configLeer(deps.claveTemaOscuro),
        ]);
        if (anchoG) {
          const n = Number(anchoG);
          if (Number.isFinite(n))
            deps.cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
        }
        if (colG) {
          deps.barraLateral.fijarAbierta(colG !== '1');
          deps.barraLateral.pintarSidebar();
        }
        deps.aplicarSesionGuardada({
          proveedor: provG,
          modelo: modG,
          modo: modoG,
          razonamiento: razG,
          contextoMaxVentana: ctxG,
          ganchoPreCompact: ganchoG,
          temaOscuro: temaG,
        });
        deps.sincronizarPanelMeta();
        await deps.resincronizarSidebar();
        // [069A-7] Recargar con historial → carga la última conversación real
        // (decisión A). Con 0 conversaciones → el principal queda en BORRADOR
        // (create-on-write): NO se crea fila, NO se llama al backend.
        const candidatas = deps.getConversaciones().filter((c) => !c.archivada);
        const primera = candidatas[0];
        if (primera) {
          await deps.principal.cargarConversacion(primera.id);
        } else {
          deps.principal.ponerBorrador();
        }
        deps.activarPanel(deps.principal);
        await deps.restaurarPanelDerecho();
      } catch (e: unknown) {
        deps.principal.avisoLocal(
          `el backend no arrancó: ${String(e)}`,
          deps.modoTexto,
          deps.baseApi !== null && !deps.usaTauri
            ? 'revisa ?api= y ?token= y recarga'
            : 'puedes escribir igual (reintenta al enviar)',
        );
      }
    })();
  }
}
