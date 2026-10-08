/* Eventos del adaptador real: contrato AgenteEvento del núcleo + log de turno.
 * Extraído de `realTipos.ts` (superaba el límite de 300 líneas): sin cambios
 * de contrato; `realTipos.ts` re-exporta este módulo. */
import type { TareaVisible } from '../../dominio/tipos';

/**
 * Contrato AgenteEvento del núcleo (tag `tipo`, snake_case). Fiel a
 * `core/src/evento.rs` (`#[serde(tag = "tipo", rename_all = "snake_case")]`):
 * el backend envía `{ tipo: "token", texto }`, NO `{ evento: ... }`. Los
 * campos opcionales pueden no venir según el proveedor; el adaptador nunca
 * asume presentes los no obligatorios. [039A-1 04-09 H2-fix] El discriminante
 * era `evento` y el switch no caía nunca (respuesta invisible en vivo).
 */
export type AgenteEvento =
  | { tipo: 'token'; texto: string }
  /** [129A-1] Pensamiento completo del modelo (`reasoning_content`): un solo
   * evento al completar la llamada. Sin razonamiento no se emite. */
  | { tipo: 'razonamiento'; texto: string }
  /** [129A-2] Fragmento de pensamiento EN VIVO: se anexa a un summary
   * abierto (con spinner + contador) hasta que llega `razonamiento`. */
  | { tipo: 'razonamiento_delta'; texto: string }
  | { tipo: 'tool_start'; tool: string; argumentos: unknown }
  | {
      tipo: 'tool_result';
      tool: string;
      ok: boolean;
      resumen: string;
      diff?: string | null;
      /** [209A-1 F3] Id de ejecución de consola (tool `comando`): la fila
       * del resumen enlaza a su consola. `undefined` en el resto de tools.
       * Fiel a `core/src/contrato/evento.rs` (`ToolResult.consola_id`). */
      consola_id?: string | null;
    }
  | { tipo: 'peticion_aprobacion'; id: string; tool: string; argumentos: unknown; clasificacion: string }
  | { tipo: 'requiere_aprobacion'; tool: string; clasificacion: string }
  | { tipo: 'permiso_denegado'; tool: string; motivo: string }
  | { tipo: 'subagente_inicio'; perfil: string; instruccion: string }
  | { tipo: 'subagente_fin'; resumen: string; ok: boolean; parcial: boolean }
  | { tipo: 'plan_propuesto'; cambios: number; resumen: string }
  | {
      tipo: 'usage';
      tokens_prompt: number;
      tokens_complecion: number;
      ocupacion_pct?: number | null;
      provider?: string | null;
      modelo?: string | null;
    }
  | { tipo: 'contexto'; skills: number }
  | {
      tipo: 'contexto_detalle';
      max_ventana: number;
      reserva_salida: number;
      system_instrucciones: number;
      definiciones_tools: number;
      mensajes: number;
      resultados_tools: number;
      total_entrada: number;
      ocupacion_pct: number;
    }
  | { tipo: 'telemetria'; subagentes_parciales: number; herramientas: Array<{ tool: string; usos: number; fallos: number; duracion_ms_total: number }> }
   /** [109A-5 F2] Plan visible de la conversación: llega tras cada acción de la
    * tool `ListaTodo` y al arrancar un turno con plan vigente (resume). Trae la lista
   * COMPLETA, no un delta. */
  | { tipo: 'tareas_actualizadas'; items: TareaVisible[] }
  /** [109A-5 F3] Meta declarada como lograda: llega al marcar la meta (no al
   * cerrar el turno, porque `lograr` puede ocurrir entre turnos). `turno_id`
   * ancla el badge al pie de ESE turno. */
  | { tipo: 'meta_lograda'; meta: string; lograda_en: string; elapsed_ms: number; turno_id: string }
  /* [109A-5 F4] Pausa automática: el backend congela el reloj tras 3 turnos
   * consecutivos con el mismo bloqueo declarado. El motivo explica el porqué
   * (no lo pausó el usuario) y `turnos` es la evidencia. */
  | { tipo: 'meta_pausada_por_bloqueo'; motivo: string; turnos: number }
  | { tipo: 'error'; mensaje: string; retryable: boolean }
  | { tipo: 'done'; turno_id: string }
  /** [069A-1 F6] El agente ejecutó una operación del navegador interno.
   * `captura_base64` solo está presente para accion="capturar". */
  | { tipo: 'tool_navegador'; accion: string; url?: string; selector?: string; captura_base64?: string; ok: boolean; descripcion: string }
  /** [129A-10 F2] El agente quiere mostrar un archivo en Files (vista, no
   * edición): el orquestador abre la tab y lo previsualiza. */
  | { tipo: 'mostrar_archivo'; ruta: string; descripcion: string }
  /** [209A-1 F3] Arranque de una ejecución `comando`: comando VERBATIM +
   * conversación dueña. Fiel a `core/src/contrato/evento.rs`
   * (`ConsolaInicio`): la UI crea la entrada con su id ANTES del primer
   * chunk. */
  | { tipo: 'consola_inicio'; id_ejecucion: string; comando: string; conversacion_id: string }
  /** [209A-1 F3] Línea de salida en vivo (sin `\n` final). `flujo` es
   * `stdout`|`stderr` (literales del contrato, para el prefijo/color). */
  | { tipo: 'consola_chunk'; id_ejecucion: string; flujo: 'stdout' | 'stderr'; linea: string }
  /** [209A-1 F3] Fin de la ejecución: la UI congela la entrada. `codigo`
   * `null` = matada o sigue en fondo (desacoplada). */
  | {
      tipo: 'consola_fin';
      id_ejecucion: string;
      codigo: number | null;
      truncada: boolean;
      duracion_ms: number;
    };

/** [129A-4 F4] Evento de un turno para el visor (`log_turno` del backend).
 * El `payload_json` es el JSON íntegro del evento (tag `tipo` snake_case). */
export interface EventoTurnoLog {
  id: number;
  turno_id: string;
  tipo: string;
  payload_json: string;
  creado_en: string;
}
