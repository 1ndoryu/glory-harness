/* Tipos del adaptador real (contrato AgenteEvento, sesión, transporte).
 * Solo tipos + `esEntornoTauri`; sin runtime salvo esa guarda. */
import type { EstadoGit } from '../componentes/panelGit';
import type {
  ComandoMetaVisible,
  EstadoMetaVisible,
  ListadoWorkspace,
  ResultadoBusqueda,
  TareaVisible,
  Workspace,
} from '../dominio/tipos';

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
  | { tipo: 'tool_start'; tool: string; argumentos: unknown }
  | { tipo: 'tool_result'; tool: string; ok: boolean; resumen: string; diff?: string | null }
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
   * tool `todo` y al arrancar un turno con plan vigente (resume). Trae la lista
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
  | { tipo: 'tool_navegador'; accion: string; url?: string; selector?: string; captura_base64?: string; ok: boolean; descripcion: string };

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
}

export interface CargaConversacion {
  id: string;
  titulo: string;
  mensajes: MensajeGuardado[];
  acciones: AccionRecuperada[];
  /** [039A-3 P1] Uso/modelo real del último turno (para repintar el pie). */
  ultimo_uso: {
    provider: string;
    modelo: string;
    tokens_prompt: number;
    tokens_complecion: number;
  } | null;
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

export interface ProveedorInfo {
  id: string;
  modelos: string[];
  claves: number;
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
}

/** Resultado de cierre de un turno, para que el llamador decida el pie. */
export type ResultadoTurno = 'ok' | 'error' | 'cancelado';

/** [109A-3] Un recuerdo del proyecto activo (DTO del backend). */
export interface RecuerdoMemoria {
  clave: string;
  contenido: string;
  origen: string;
  usos: number;
  ultimo_uso: string | null;
  actualizada_en: string;
  /** Archivado por el curador: se conserva para auditar, no se inyecta. */
  archivada: boolean;
}

/** [109A-3] Ámbito activo resuelto por el backend y sus recuerdos. El front
 * no elige el ámbito: el panel siempre muestra el proyecto abierto. */
export interface ListadoMemoria {
  /** `true` = ámbito global (la carpeta activa no es un área registrada). */
  global: boolean;
  /** Etiqueta del núcleo: `global` o `proyecto`. */
  ambito: string;
  /** Nombre del área activa; `null` en el ámbito global. */
  proyecto: string | null;
  ruta: string | null;
  /** Carpeta `.glory/memorias` del área activa; `null` sin área. */
  carpeta: string | null;
  recuerdos: RecuerdoMemoria[];
}

/** [109A-3] Resultado de exportar o importar la carpeta de memorias. */
export interface ResultadoCarpetaMemoria {
  carpeta: string;
  recuerdos: number;
  /** Archivos rechazados al importar, con motivo (vacío al exportar). */
  omitidos: string[];
}

/** [109A-4 F3] Resultado de `/compactar`. Sin el resumen del tramo: se
 * persiste como punto de compactación y NO se pinta en el chat (el historial
 * visible no cambia). `compactado: false` trae el `motivo` del no-op. */
export interface ResumenCompactacion {
  compactado: boolean;
  motivo: string | null;
  tokens_antes: number;
  tokens_despues: number;
  ahorro_pct: number;
  ocupacion_pct: number;
  tramos: number;
}

/** [109A-4] Comando `/` definido por el área activa (`.glory/comandos/*.md`). */
export interface ComandoArea {
  nombre: string;
  descripcion: string;
}

export interface HooksAdaptador {
  /** Se llama con cada `abrir_sesion`/`reconfigurar`/`elegir_workspace`. */
  onSesion?: (info: InfoSesion) => void;
  /** [039A-3 P6] Se llama al llegar `contexto_detalle`/`usage` con el estado
   * de contexto del turno (ocupacion_pct + max_ventana) para que la UI
   * actualice el indicador circular en vivo. Hook opcional: no rompe API. */
  onContexto?: (uso: UsoTurno) => void;
  /** [069A-1 F4] El agente usó una tool de navegador: refleja la acción
   * en el UI del navegador (log + anotaciones). */
  onToolNavegador?: (ev: AgenteEvento & { tipo: 'tool_navegador' }) => void;
  /** [069A-2 F4] Estado de la conexión del transporte (solo el HTTP/SSE la
   * reporta; Tauri in-process no la usa). */
  onConexion?: (estado: 'conectando' | 'en-linea' | 'reconectando' | 'error', detalle?: string) => void;
  /** [089A-12] El agente modificó un archivo (file_write/file_patch con
   * diff): el orquestador actualiza el pane Files. Hook opcional. */
  onCambioArchivo?: (cambio: {
    origen: 'tool';
    tool: 'file_write' | 'file_patch';
    ruta: string;
    titulo: string;
    resumen: string;
    diff: string | null;
  }) => void;
}

/** [069A-2 F4] Transporte del adaptador: la fuente de los eventos y el
 * destino de los comandos. El render (aplicar/aviso/montar) vive una sola
 * vez en `crearAdaptadorReal`; Tauri IPC y HTTP/SSE solo implementan esto. */
export interface Transporte {
  abrirSesion(opts: OpcionesTurno): Promise<InfoSesion>;
  reconfigurarSesion(opts: OpcionesTurno): Promise<InfoSesion>;
  enviarTurno(mensaje: string, panelId: string | null, soloLectura?: boolean): Promise<void>;
  /** [109A-4 F4] ¿Este transporte sabe correr un turno solo-lectura? Tauri sí
   * (política del núcleo); el modo web no tiene esa política por turno, así
   * que `/meta` se rechaza con motivo explícito en vez de simularlo. */
  soportaSoloLectura(): boolean;
  detenerTurno(panelId: string | null): void;
  responderAprobacion(id: string, respuesta: string): Promise<void>;
  pendientesAprobacion(): Promise<unknown[]>;
  /** Tauri responde aprobaciones entre turnos y reenvía; HTTP resuelve en
   * vivo durante el turno y nunca reenvía. */
  requiereReenvioTrasAprobar(): boolean;
  /** Registra (una vez) el reenvío turno→UI: eventos + cierre. */
  escucharTurno(
    onEvento: (ev: AgenteEvento) => void,
    onFin: (ok: boolean, error?: string) => void,
  ): Promise<void>;
  convNueva(titulo: string | null, panelId: string | null): Promise<InfoConversacion>;
  convListar(): Promise<InfoConversacion[]>;
  convCargar(id: string, panelId: string | null): Promise<CargaConversacion>;
  convRenombrar(id: string, titulo: string): Promise<boolean>;
  convArchivar(id: string, archivada: boolean): Promise<boolean>;
  /** [069A-7] `null` = no quedó ninguna conversación en el panel (borrador). */
  convEliminar(id: string, panelId: string | null): Promise<InfoConversacion | null>;
  convRewind(hastaMensajeId: string, editar: boolean, panelId: string | null): Promise<CargaConversacion>;
  tramoRestaurar(panelId: string | null): Promise<ResultadoRestauracionTramo>;
  leerProveedores(): Promise<ProveedorInfo[]>;
  leerConfig(clave: string): Promise<string | null>;
  guardarConfig(clave: string, valor: string): Promise<void>;
  elegirWorkspace(): Promise<InfoSesion>;
  fijarWorkspace(ruta: string): Promise<InfoSesion>;
  fijarMeta(meta: string | null): Promise<string | null>;
  /** [109A-5 F3] Ciclo de vida de la meta por conversación. `metaLeer` es la
   * fuente del panel (persecución + historial): sin ella la UI solo tendría el
   * texto del textarea. `null` = sin fila durable todavía (borrador). */
  metaAplicar(comando: ComandoMetaVisible): Promise<EstadoMetaVisible | null>;
  metaLeer(conversacionId: string): Promise<EstadoMetaVisible | null>;
  // [069A-Proyectos] Gestión de proyectos (áreas de trabajo).
  workspacesListar(): Promise<{ workspaces: Workspace[]; activa: Workspace | null }>;
  proyectoGuardar(nombre: string, ruta: string): Promise<InfoSesion>;
  workspaceActivarsPorRuta(ruta: string): Promise<InfoSesion>;
  workspaceRenombrar(id: string, nombre: string): Promise<boolean>;
  workspaceEliminar(id: string): Promise<boolean>;
  workspaceInfo(): Promise<{ ruta: string; nombre: string }>;
  workspaceListarEntrada(ruta: string, profundidad?: number): Promise<ListadoWorkspace>;
  workspaceLeerArchivo(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }>;
  workspaceAbrirCon(ruta: string): Promise<void>;
  workspaceBuscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda>;
  workspaceGitEstado(): Promise<EstadoGit>;
  // [109A-3] Memorias del proyecto activo (panel "Memorias" del modal).
  memoriaListar(): Promise<ListadoMemoria>;
  memoriaBorrar(clave: string): Promise<ListadoMemoria>;
  memoriaCurar(): Promise<string>;
  memoriaExportar(): Promise<ResultadoCarpetaMemoria>;
  memoriaImportar(): Promise<ResultadoCarpetaMemoria>;
  // [109A-4] Comandos `/` propios del área activa (catálogo + expansión).
  comandosListar(): Promise<ComandoArea[]>;
  comandoExpandir(nombre: string, argumentos: string): Promise<string>;
  // [109A-4 F3] Compactación por demanda de la conversación del panel.
  compactarConversacion(panelId: string | null, instruccion: string | null): Promise<ResumenCompactacion>;
}

/** true solo dentro de la app Tauri (hay `__TAURI__` global). */
export function esEntornoTauri(): boolean {
  return typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== 'undefined';
}
