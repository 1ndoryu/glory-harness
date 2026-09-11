/* [089A-5] Historial de navegación de la app (atrás/adelante de la barra
 * superior, misma lógica que Synara): cada visita es conversación + área
 * juntas. La sidebar registra sus navegaciones (`navegarA`) y restaura con
 * `irAtras`/`irAdelante`; las acciones `irA*` las pone la sidebar por `deps`
 * y no registran, así el restore no se auto-registra (sin reentrancia). */

import type { BarraSuperior } from '../componentes/barraSuperior';
import { crearHistorialApp, type EntradaHistorialApp } from '../dominio/historialApp';

export interface HistorialVistaDeps {
  barra: BarraSuperior;
  rutaInicial: string | null;
  /** Carga la conversación en el panel enfocado (mock y real). */
  irAConversacion: (id: string) => void;
  /** Marca visual en la lista (no dispara navegación). */
  seleccionarVisual: (id: string) => void;
  /** Cambia el área activa; `false` si no se pudo. */
  irAProyecto: (ruta: string) => Promise<boolean>;
  /** Deja el principal en borrador y lo activa. */
  irABorrador: () => void;
}

export interface HistorialVista {
  /** Navegación del usuario: registra la visita que se abandona y fija la nueva. */
  navegarA: (nueva: EntradaHistorialApp) => void;
  irAtras: () => void;
  irAdelante: () => void;
}

export function crearHistorialVista(deps: HistorialVistaDeps): HistorialVista {
  const historial = crearHistorialApp();
  let visitaActual: EntradaHistorialApp = {
    conversaId: null,
    proyectoRuta: deps.rutaInicial,
  };

  function refrescarBotones(): void {
    deps.barra.setPuedeNavegar(historial.puedeAtras(), historial.puedeAdelante());
  }

  function navegarA(nueva: EntradaHistorialApp): void {
    historial.registrar(visitaActual);
    visitaActual = { ...nueva };
    refrescarBotones();
  }

  async function restaurar(destino: EntradaHistorialApp | null): Promise<void> {
    if (!destino) return;
    const anterior = visitaActual;
    visitaActual = { ...destino };
    // Área `null` (mock o estado transitorio): no se puede resolver, se omite
    // la activación y se restaura solo la conversación.
    if (destino.proyectoRuta !== null && destino.proyectoRuta !== anterior.proyectoRuta) {
      await deps.irAProyecto(destino.proyectoRuta);
    }
    if (destino.conversaId !== null) {
      deps.irAConversacion(destino.conversaId);
      deps.seleccionarVisual(destino.conversaId);
    } else {
      deps.irABorrador();
    }
    refrescarBotones();
  }

  return {
    navegarA,
    irAtras() {
      void restaurar(historial.atras(visitaActual));
    },
    irAdelante() {
      void restaurar(historial.adelante(visitaActual));
    },
  };
}
