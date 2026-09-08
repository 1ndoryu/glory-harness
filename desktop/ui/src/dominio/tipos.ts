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
  /** Proyecto asociado; null/undefined = sin proyecto. */
  workspaceId?: string | null;
  workspaceNombre?: string | null;
}

/** [069A-Proyectos] Área de trabajo (proyecto): agrupa conversaciones por
 * carpeta. Serializado 1:1 con el backend. */
export interface Workspace {
  id: string;
  nombre: string;
  ruta: string;
  creada_en: string;
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
  | 'carpeta'
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
  | 'spin'
  | 'mas'
  | 'mas-horizontal'
  | 'pausa'
  | 'reproducir'
  | 'flecha-izq'
  | 'flecha-der'
  | 'recargar'
  | 'camara'
  | 'seleccionar'
  | 'minimizar'
  | 'maximizar'
  | 'restaurar';

// ---------- Elemento elegido en el navegador (feature seleccionar) ----------

/** Descriptor de un elemento del navegador que el usuario eligió para
 * pasárselo al modelo. Lo produce el panel navegador (desktop/WebView2) al
 * hacer hover+clic en "seleccionar elemento". */
export interface ElementoSeleccionado {
  /** URL de la página donde está el elemento. */
  pagina: string;
  /** Selector CSS robusto que localiza el elemento (para `navegador_reflejo`). */
  selector: string;
  /** Etiqueta corta: `tag#id.clase` (para el badge). */
  etiqueta: string;
  /** Recorte del texto visible del elemento (para el badge/contexto). */
  texto: string;
}
