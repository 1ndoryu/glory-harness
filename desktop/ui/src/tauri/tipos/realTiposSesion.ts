/* Tipos de sesión/conversación/turno del adaptador real.
 * Extraído de `realTipos.ts` (superaba el límite de 300 líneas): sin cambios
 * de contrato; `realTipos.ts` re-exporta este módulo. */

export interface OpcionesTurno {
  proveedor: string;
  modelo: string;
  modo: string;
  /** [039A-1 04-09 H7] Nivel de razonamiento (low|medium|high). */
  razonamiento: string;
  /** [039A-3 P5] Panel destino del turno (`'principal'` default en el backend;
   * el panel lateral pasa `'lateral'`). En M1 solo hay un turno a la vez y
   * este panel es el que recibe los eventos (el adaptador es compartido). */
  panelId?: string;
  /** [109A-4 F4] Turno SOLO LECTURA (`/meta <texto>`): el backend lo corre en
   * modo meta (deniega toda tool con efecto) sin cambiar el modo de la sesión.
   * `undefined`/`false` = turno normal. */
  soloLectura?: boolean;
}

export interface InfoConversacion {
  id: string;
  titulo: string;
  archivada: boolean;
  /** [119A-3 F1] RFC3339 de creación (criterio «Created at»). */
  creada_en: string;
  actualizada_en: string;
  workspace_id?: string | null;
  workspace_nombre?: string | null;
}

export interface InfoSesion {
  modelo: string;
  workspace: string;
  proveedores: Array<{ nombre: string; claves: number }>;
  /** [069A-7] `null` = sin conversación (borrador create-on-write): la sesión
   * no tiene fila anclada hasta el primer mensaje. */
  conversacion: InfoConversacion | null;
  aviso?: string | null;
}

export interface MensajeGuardado {
  id: string;
  conversacion_id: string;
  rol: string;
  contenido: string;
  creado_en: string;
}

/** [039A-1 04-09 H6] Acción (tool) persistida de una conversación, para
 * repintar el bloque `.herramienta` al recargar (resumen/diff). */
export interface AccionRecuperada {
  tool: string;
  ok: boolean;
  resumen: string;
  argumentos_json: string | null;
  diff: string | null;
  turno_en: string;
  /** [129A-7] Id del turno (agrupar cambios por turno en "Cambios"). */
  turno_id: string;
}

/** [20-09-2026] Uso/modelo real de un turno con su ancla temporal (para
 * repintar CADA pie de turno al recargar, no solo el último). */
export interface UsoTurnoRecuperado {
  /** `creado_en` del turno (ancla: el turno de un mensaje es el último con
   * `turno_en <= creado_en` del mensaje). */
  turno_en: string;
  provider: string;
  modelo: string;
  tokens_prompt: number;
  tokens_complecion: number;
}

export interface CargaConversacion {
  id: string;
  titulo: string;
  mensajes: MensajeGuardado[];
  acciones: AccionRecuperada[];
  /** [039A-3 P1] Uso/modelo real del último turno (para repintar el pie).
   * Se conserva por compatibilidad; el front prefiere `usos_turno`. */
  ultimo_uso: {
    provider: string;
    modelo: string;
    tokens_prompt: number;
    tokens_complecion: number;
  } | null;
  /** [20-09-2026] Uso/modelo real de TODOS los turnos (cada pie de turno al
   * recargar, no solo el último). */
  usos_turno: UsoTurnoRecuperado[];
  /** [039A-3 P3] Archivos que tocó el último tramo rebobinado ("volver a
   * punto"), listos para la acción EXPLÍCITA "restaurar archivos de este
   * tramo". Vacío cuando la carga no viene de un rewind. */
  archivos_tramo?: string[];
}

/** [039A-3 P3] Resultado de la restauración explícita de un tramo. */
export interface RestauracionArchivo {
  ruta: string;
  /** "restaurado" | "cambio_externo" | "omitido" | "error" */
  estado: string;
  detalle?: string | null;
}

export interface ResultadoRestauracionTramo {
  archivos: string[];
  restaurados: RestauracionArchivo[];
  omitidos: RestauracionArchivo[];
}

/** [129A-7] Un archivo tocado por el agente en un turno (panel "Cambios"):
 * primera escritura de cada (turno, ruta) según el vault. */
export interface CambioArchivoTurno {
  turno_id: string;
  ruta: string;
  herramienta: string;
  en_ms: number;
}

/** Totales del último turno (para el panel meta / cabecera, sin simular). */
export interface UsoTurno {
  tokensPrompt: number;
  tokensComplecion: number;
  ocupacionPct: number | null;
  /** [039A-3 P1] Modelo REAL que respondió (provider/modelo del último Usage
   * tras fallback); `null` si el proveedor no lo reportó. */
  modelo: string | null;
  /** [039A-3 P1] Ventana máxima de contexto (del `ContextoDetalle`). */
  maxVentana: number | null;
  /** [039A-3 P1] Reserva de salida declarada en el `ContextoDetalle`. */
  reservaSalida: number | null;
  /** [039A-3 P1] Tokens totales de entrada del último desglose de contexto. */
  totalEntrada: number | null;
  /** [129A-2] Velocidad medida del turno (tokens de compleción / segundo de
   * turno, con la misma heurística de reloj que el pie). `null` = sin medir
   * (p. ej. historial recargado o turno cancelado). */
  velocidadTokS: number | null;
}

/** Resultado de cierre de un turno, para que el llamador decida el pie. */
export type ResultadoTurno = 'ok' | 'error' | 'cancelado';

/** [139A-8 F6n R6] Extras del `turn.finished` web (título vigente + uso
 * autoritativo): el front los aplica en optimista y se ahorra el `listar`
 * post-turno. `null` = el transporte no los expone (Tauri/aborto) y el front
 * conserva el refetch. */
export interface CierreTurno {
  titulo: string | null;
  uso: {
    entrada: number;
    salida: number;
    proveedor: string | null;
    modelo: string | null;
  } | null;
}
