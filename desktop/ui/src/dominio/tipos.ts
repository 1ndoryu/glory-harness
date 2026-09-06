// ============================================================
// Tipos del dominio UI (port del mockup del plan 039A-1).
// Sin lógica de backend: modelan lo que la vista necesita
// renderizar y lo que la capa de simulación emite.
// ============================================================

/** Proveedor con su catálogo de modelos (mismo espíritu que task). */
export interface ProveedorModelo {
  id: string;
  etiqueta: string;
  modelos: Modelo[];
}

export interface Modelo {
  /** id técnico del modelo (p. ej. 'poolside/laguna-s-2.1-free'). */
  modelo: string;
  /** nombre amigable mostrado en la UI (p. ej. 'Laguna S 2.1 Free'). */
  nombre: string;
}

export interface ModeloSeleccionado {
  proveedor: string;
  modelo: string;
  nombre: string;
}

/** Conversación listada en la sidebar. */
export interface Conversacion {
  id: string;
  titulo: string;
  seleccionada?: boolean;
  /** Ocultas bajo la sección Archivadas de la sidebar. */
  archivada?: boolean;
}

/** Resultado de una herramienta: texto plano o HTML de diff (- / +). */
export type ResultadoHerramienta =
  | { tipo: 'texto'; texto: string }
  | { tipo: 'html'; html: string };

// ---------- Contrato de eventos (lo que renderiza #mensajes) ----------

export interface MensajeUsuario {
  tipo: 'usuario';
  texto: string;
}

export interface MensajeAsistente {
  tipo: 'asistente';
  texto: string;
}

export interface BloqueRazonamiento {
  tipo: 'razonamiento';
  /** texto del pensamiento interno. */
  texto: string;
  /** estado: null = fluyendo; si no, texto del meta (p. ej. '1.4 s'). */
  meta: string | null;
}

export type EstadoHerramienta =
  | { estado: 'ejecutando' }
  | { estado: 'completada'; meta: string; resultado: ResultadoHerramienta }
  | { estado: 'error'; meta: string; resultado: ResultadoHerramienta };

export interface BloqueHerramienta {
  tipo: 'herramienta';
  icono: IconoNombre;
  titulo: string;
  detalle: EstadoHerramienta;
}

export type DecisionAprobacion = 'aprobar' | 'permitir' | 'denegar';

export interface BloqueAprobacion {
  tipo: 'aprobacion';
  titulo: string;
  /** JSON de argumentos (texto pre-formateado). */
  argsTexto: string;
  /** estado visual: pendiente | aprobada | denegada. */
  resolucion: 'pendiente' | 'aprobada' | 'denegada';
  /** texto del meta tras resolver (p. ej. 'ok · 4 ms'). */
  meta?: string;
  /** argsHtml tras aprobar (diff visible). */
  argsHtml?: string;
}

export interface BloqueAvisoSistema {
  tipo: 'aviso';
  texto: string;
  meta: string;
  detalle: string;
}

/** Un bloque visible en la columna #mensajes. */
export type Bloque =
  | MensajeUsuario
  | MensajeAsistente
  | BloqueRazonamiento
  | BloqueHerramienta
  | BloqueAprobacion
  | BloqueAvisoSistema;

// ---------- Iconos (nombres; el SVG lo resuelve componentes/iconos) ----------

export type IconoNombre =
  | 'lupa'
  | 'archivo'
  | 'lapiz'
  | 'globo'
  | 'cerebro'
  | 'terminal'
  | 'check'
  | 'x'
  | 'x-circulo'
  | 'reloj'
  | 'mensaje'
  | 'agente'
  | 'flujo'
  | 'complementos'
  | 'ajustes'
  | 'nueva'
  | 'copiar'
  | 'volver'
  | 'panel-izq-cerrar'
  | 'panel-izq-abrir'
  | 'flecha-arriba'
  | 'detener'
  | 'chevron-abajo'
  | 'chevron-derecha'
  | 'navegador'
  | 'spin';
