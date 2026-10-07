/* Transporte del adaptador real + hooks + proveedor.
 * Extraído de `realTipos.ts` (superaba el límite de 300 líneas): sin cambios
 * de contrato; `realTipos.ts` re-exporta este módulo. */
import type { EstadoGit, RepoGit, ResumenRepo } from '../../componentes/panelGit';
import type { FichaSubagente } from '../../componentes/subagentesViva';
import type {
  ComandoMetaVisible,
  EstadoMetaVisible,
  ListadoWorkspace,
  ResultadoBusqueda,
  Workspace,
} from '../../dominio/tipos';
import type { AgenteEvento, EventoTurnoLog } from './realTiposEventos';
import type {
  InfoConsolaLista,
  NuevaConsola,
  TranscriptConsola,
} from './realTiposConsola';
import type {
  CambioArchivoTurno,
  CargaConversacion,
  CierreTurno,
  InfoConversacion,
  InfoSesion,
  OpcionesTurno,
  RestauracionArchivo,
  ResultadoRestauracionTramo,
  UsoTurno,
} from './realTiposSesion';
import type {
  ComandoArea,
  ListadoMemoria,
  ResumenCompactacion,
  ResultadoCarpetaMemoria,
} from './realTiposMemoria';

export interface ProveedorInfo {
  id: string;
  modelos: string[];
  claves: number;
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
  /** [129A-10 F2] El agente mostró un archivo: el orquestador lo enseña en
   * Files (vista, sin editar). */
  onMostrarArchivo?: (ev: AgenteEvento & { tipo: 'mostrar_archivo' }) => void;
  /** [209A-1 F3] El usuario pulsó "ver en Consola" en la fila de un `comando`:
   * el orquestador abre la tab y revela esa ejecución. Solo en vivo (el
   * historial estático no conserva `consola_id`; la tab sí es durable). */
  onVerConsola?: (id: string) => void;
  /** [209A-1 F3] Streaming de consola (`consola_inicio/chunk/fin`): el
   * orquestador lo refleja en la tab Consola (store + auto-apertura). Un
   * solo `TurnoReal` por adaptador, así que cada evento llega una vez. */
  onConsolaEvento?: (
    ev: AgenteEvento & { tipo: 'consola_inicio' | 'consola_chunk' | 'consola_fin' },
  ) => void;
  /** [129A-11 F3] Click en un subagente de la tarjeta flotante: el
   * orquestador abre su ficha de solo lectura en el lateral (tab
   * `subagente:<n>`, sin caja de escritura por construcción). */
  onVerSubagente?: (ficha: FichaSubagente) => void;
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
  responderAprobacion(id: string, respuesta: string): Promise<boolean>;
  pendientesAprobacion(): Promise<unknown[]>;
  /** Registra (una vez) el reenvío turno→UI: eventos + cierre. */
  escucharTurno(
    onEvento: (ev: AgenteEvento) => void,
    onFin: (ok: boolean, error?: string) => void,
  ): Promise<void>;
  /** [139A-8 F6n R6] Extras del último `turn.finished` (título + uso).
   * Opcional como `workspaceGitResumen`: solo el transporte web lo expone;
   * ausente = refetch clásico, sin simular el optimista. */
  leerCierreUltimoTurno?: () => CierreTurno | null;
  convNueva(titulo: string | null, panelId: string | null): Promise<InfoConversacion>;
  convListar(): Promise<InfoConversacion[]>;
  convCargar(id: string, panelId: string | null): Promise<CargaConversacion>;
  convRenombrar(id: string, titulo: string): Promise<boolean>;
  convArchivar(id: string, archivada: boolean): Promise<boolean>;
  /** [069A-7] `null` = no quedó ninguna conversación en el panel (borrador). */
  convEliminar(id: string, panelId: string | null): Promise<InfoConversacion | null>;
  // [119A-2 F4] Batch por proyecto: archivar devuelve cuántas cambió;
  // eliminar devuelve la conversación a anclar (`null` = borrador).
  convArchivarProyecto(id: string, archivada: boolean): Promise<number>;
  convEliminarProyecto(id: string, panelId: string | null): Promise<InfoConversacion | null>;
  /** [209A-1 F4-resto] Mata UNA consola viva (la × de la tab Consola sobre
   * una entrada viva). `false` = no existe o ya terminó (idempotente). */
  consolaMatar(idEjecucion: string): Promise<boolean>;
  /** [219A-3] Sub-barra de la tab Consola: vivas primero + recientes
   * archivadas (backfill al abrir la tab a mitad de turno). */
  consolasListar(): Promise<InfoConsolaLista[]>;
  /** [219A-3] Transcript retenido por `id_ejecucion` (backfill del visor).
   * Falla si el runner ya no retiene ese id. */
  consolaSalida(idEjecucion: string): Promise<TranscriptConsola>;
  /** [219A-3] Bytes crudos al stdin de una viva. Devuelve los bytes
   * aceptados; falla si terminó o no existe. */
  consolaEscribir(idEjecucion: string, texto: string): Promise<number>;
  /** [219A-4] Abre una consola PROPIA del operador ([+ Nueva] de la tab).
   * Sin `comando` = shell por defecto del SO. */
  consolaCrear(comando?: string): Promise<NuevaConsola>;
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
  // [119A-2 F2] Abrir la carpeta del proyecto en el Explorador (solo app).
  workspaceRevelar(id: string): Promise<void>;
  // [119A-2 F3] Fijar/soltar proyecto (los fijados van primero).
  workspaceFijar(id: string, fijado: boolean): Promise<boolean>;
  workspaceInfo(): Promise<{ ruta: string; nombre: string }>;
  workspaceListarEntrada(ruta: string, profundidad?: number): Promise<ListadoWorkspace>;
  workspaceLeerArchivo(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }>;
  workspaceAbrirCon(ruta: string): Promise<void>;
  workspaceBuscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda>;
  /** [139A-2] Estado git del área o de un repo concreto (`workspace_git_repos`). */
  workspaceGitEstado(ruta?: string): Promise<EstadoGit>;
  /** [139A-2] Repos bajo el área activa (barrido descendente). */
  workspaceGitRepos(): Promise<RepoGit[]>;
  /** [139A-7] Resumen batch (descubrir+estado por repo en 1 IPC).
   * Opcional: el transporte web no lo expone (el panel usa el fan-out). */
  workspaceGitResumen?(): Promise<ResumenRepo[]>;
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
  /** [129A-4 F4] Log de un turno (eventos persistidos por el backend).
   * Opcional: el transporte web no lo expone (el visor avisa en vez de
   * simularlo). */
  logTurno?(turnoId: string): Promise<EventoTurnoLog[]>;
  /** [129A-7] Panel "Cambios": lista por turno + rechazo puntual.
   * Opcionales: el transporte web no los expone (el panel avisa en vez de
   * simularlos). */
  cambiosListar?(conversacionId: string): Promise<CambioArchivoTurno[]>;
  cambioRechazar?(conversacionId: string, turnoId: string, ruta: string): Promise<RestauracionArchivo>;
}

/** true solo dentro de la app Tauri (hay `__TAURI__` global). */
export function esEntornoTauri(): boolean {
  return typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== 'undefined';
}
