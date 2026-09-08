// ============================================================
// Esquema declarativo de opciones de configuración (centro único).
// Cada opción describe el control con que se edita y su valor.
// Los formularios se construyen a partir de estas definiciones
// (ver componentes/formulario.ts), de modo que añadir una opción
// nueva es añadir una entrada aquí — sin tocar el DOM del modal.
// ============================================================

export type TipoControl =
  | 'texto'
  | 'seleccion'
  | 'segmentado'
  | 'lectura'
  | 'valor'
  | 'booleano';

export interface OpcionControl {
  /** id técnico (clave en el estado de configuración). */
  id: string;
  /** etiqueta visible. */
  etiqueta: string;
  /** tipo de control que edita la opción. */
  tipo: TipoControl;
  /** valor actual (string para la mayoría; boolean en 'booleano'). */
  valor: string | boolean;
  /** nota de ayuda bajo el control (opcional). */
  nota?: string;
  /** 'seleccion' | 'segmentado': opciones válidas. */
  opciones?: Array<{ valor: string; etiqueta: string }>;
  /** 'texto': placeholder del input. */
  placeholder?: string;
}

export interface GrupoOpciones {
  /** título del grupo (sección del formulario). */
  titulo: string;
  opciones: OpcionControl[];
}

export interface FormularioDefinicion {
  /** id del panel/formulario. */
  id: string;
  grupos: GrupoOpciones[];
}

// ---------- Opciones reales del modal de configuración ----------
// Alineado con TurnoConfig del core (core/src/runtime.rs) y la allowlist de
// proveedores/modelos (core/src/llm.rs). El harness no persiste en SQLite:
// la persistencia real es en memoria (PersistenciaMemoria del CLI).

export const OPCIONES_MODELO: GrupoOpciones = {
  titulo: 'Modelo',
  opciones: [
    {
      id: 'proveedorModelo',
      etiqueta: 'Proveedor / modelo',
      tipo: 'valor',
      valor: 'commandcode / poolside/laguna-s-2.1-free',
    },
    {
      id: 'nivelRazonamiento',
      etiqueta: 'Razonamiento',
      tipo: 'segmentado',
      valor: 'medium',
      nota: 'Esfuerzo de razonamiento (low / medium / high).',
      opciones: [
        { valor: 'low', etiqueta: 'Bajo' },
        { valor: 'medium', etiqueta: 'Medio' },
        { valor: 'high', etiqueta: 'Alto' },
      ],
    },
    {
      id: 'claves',
      etiqueta: 'Claves',
      tipo: 'valor',
      valor: '~/.glory-harness.env (cargadas)',
    },
  ],
};

export const OPCIONES_EJECUCION: GrupoOpciones = {
  titulo: 'Ejecución',
  opciones: [
    {
      id: 'modo',
      etiqueta: 'Modo',
      tipo: 'segmentado',
      valor: 'predeterminado',
      nota: 'Autónomo ejecuta las tools sin aprobar; meta solo lee (deniega cambios).',
      opciones: [
        { valor: 'predeterminado', etiqueta: 'Predeterminado' },
        { valor: 'meta', etiqueta: 'Meta' },
        { valor: 'autonomo', etiqueta: 'Autónomo' },
      ],
    },
    {
      id: 'estilo',
      etiqueta: 'Estilo de respuesta',
      tipo: 'segmentado',
      valor: 'conciso',
      opciones: [
        { valor: 'conciso', etiqueta: 'Conciso' },
        { valor: 'detallado', etiqueta: 'Detallado' },
        { valor: 'amable', etiqueta: 'Amable' },
      ],
    },
    {
      id: 'maxTurns',
      etiqueta: 'Máx. turnos',
      tipo: 'texto',
      valor: '10',
      nota: 'Límite de pasos LLM → tools por turno.',
    },
    {
      id: 'timeoutTool',
      etiqueta: 'Timeout de tool (s)',
      tipo: 'texto',
      valor: '15',
      nota: 'Tiempo máximo de espera de una herramienta.',
    },
    {
      id: 'temperatura',
      etiqueta: 'Temperatura',
      tipo: 'seleccion',
      valor: '0.2',
      opciones: [
        { valor: '0', etiqueta: '0 — determinista' },
        { valor: '0.2', etiqueta: '0.2' },
        { valor: '0.5', etiqueta: '0.5' },
        { valor: '0.8', etiqueta: '0.8' },
        { valor: '1', etiqueta: '1 — creativo' },
      ],
    },
    {
      id: 'maxTokens',
      etiqueta: 'Máx. tokens',
      tipo: 'seleccion',
      valor: '2048',
      opciones: [
        { valor: '1024', etiqueta: '1 024' },
        { valor: '2048', etiqueta: '2 048' },
        { valor: '4096', etiqueta: '4 096' },
        { valor: '8192', etiqueta: '8 192' },
      ],
    },
  ],
};

export const OPCIONES_PERMISOS: GrupoOpciones = {
  titulo: 'Permisos',
  opciones: [
    {
      id: 'busquedaWeb',
      etiqueta: 'Búsqueda web',
      tipo: 'booleano',
      valor: true,
    },
    {
      id: 'recordatorios',
      etiqueta: 'Recordatorios',
      tipo: 'booleano',
      valor: true,
    },
    {
      id: 'memoria',
      etiqueta: 'Memoria',
      tipo: 'booleano',
      valor: true,
    },
    {
      id: 'skills',
      etiqueta: 'Skills',
      tipo: 'booleano',
      valor: true,
    },
  ],
};

export const OPCIONES_APARIENCIA: GrupoOpciones = {
  titulo: 'Apariencia',
  opciones: [
    {
      id: 'temaOscuro',
      etiqueta: 'Modo oscuro',
      tipo: 'booleano',
      valor: false,
      nota: 'Invierte la paleta monocroma de la interfaz.',
    },
  ],
};

export const OPCIONES_CONTEXTO: GrupoOpciones = {
  titulo: 'Contexto',
  opciones: [
    {
      id: 'idioma',
      etiqueta: 'Idioma',
      tipo: 'seleccion',
      valor: 'es',
      opciones: [
        { valor: 'es', etiqueta: 'Español' },
        { valor: 'en', etiqueta: 'Inglés' },
      ],
    },
    {
      id: 'workspace',
      etiqueta: 'Workspace',
      tipo: 'texto',
      // [039A-1 04-09 H3] El valor real lo fija el backend al abrir la sesión
      // (`InfoSesion.workspace`); la UI lo sincroniza vía modal.asignarValor.
      // Este valor por defecto solo aplica en el mock (navegador).
      valor: '',
      placeholder: 'ruta del workspace',
      nota: 'Se abre con el workspace real del backend al iniciar la sesión.',
    },
    {
      // [039A-3 P6] Ventana de contexto configurada (default 150k). El backend
      // la inyecta como `contexto.max_ventana` al construir la sesión (clave
      // persistida `contexto_max_ventana`, misma que el id para configGuardar).
      // Tope blando: si el modelo real tiene menos ventana, el % del
      // `ContextoDetalle` manda (fuente única, decisión §2.10).
      id: 'contexto_max_ventana',
      etiqueta: 'Ventana de contexto',
      tipo: 'texto',
      valor: '150000',
      placeholder: '150000',
      nota: 'Máx. tokens de contexto (default 150000). Se aplica al iniciar la sesión.',
    },
    {
      id: 'preferencias',
      etiqueta: 'Preferencias',
      tipo: 'texto',
      valor: '',
      placeholder: 'p. ej. “código en español, respuestas concisas”',
      nota: 'Se inyectan en el prompt del sistema.',
    },
  ],
};

/** Definición completa del modal de configuración (nav + formularios). */
export const FORMULARIO_CONFIGURACION: Array<{
  id: string;
  etiqueta: string;
  grupos: GrupoOpciones[];
}> = [
  { id: 'modelo', etiqueta: 'Modelo', grupos: [OPCIONES_MODELO] },
  { id: 'ejecucion', etiqueta: 'Ejecución', grupos: [OPCIONES_EJECUCION] },
  { id: 'permisos', etiqueta: 'Permisos', grupos: [OPCIONES_PERMISOS] },
  { id: 'apariencia', etiqueta: 'Apariencia', grupos: [OPCIONES_APARIENCIA] },
  { id: 'contexto', etiqueta: 'Contexto', grupos: [OPCIONES_CONTEXTO] },
];
