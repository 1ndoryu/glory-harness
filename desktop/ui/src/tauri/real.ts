// ============================================================
// Adaptador real Tauri (plan 039A-1, Fases 4-5): la UI habla al núcleo
// vía comandos in-process y escucha el contrato AgenteEvento.
// Misma superficie que la simulación (montar/detener) para no tocar
// el layout: solo cambia la fuente de los eventos.
// ============================================================

import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import {
  crearAvisoSistema,
  crearHerramientaViva,
  crearMensajeAsistenteVivo,
  formatearResultadoHerramienta,
  crearMensajeUsuario,
  crearTarjetaAprobacion,
  type AsistenteVivo,
  type HerramientaViva,
} from '../componentes/mensajes';
import type {
  DecisionAprobacion,
  IconoNombre,
  ListadoWorkspace,
  ResultadoBusqueda,
  Workspace,
} from '../dominio/tipos';
import { el } from '../util/dom';

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
  enviarTurno(mensaje: string, panelId: string | null): Promise<void>;
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
  workspaceGitEstado(): Promise<import('../componentes/panelGit').EstadoGit>;
}

/** Transporte Tauri in-process (comportamiento 039A-1 intacto). */
export function transporteTauri(): Transporte {
  return {
    abrirSesion: (opts) =>
      invoke<InfoSesion>('abrir_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        dir: null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      }),
    reconfigurarSesion: (opts) =>
      invoke<InfoSesion>('reconfigurar_sesion', {
        provider: opts.proveedor || null,
        modelo: opts.modelo || null,
        modo: opts.modo || null,
        razonamiento: opts.razonamiento || null,
      }),
    enviarTurno: (mensaje, panelId) => invoke<void>('enviar_turno', { mensaje, panel_id: panelId }),
    detenerTurno: (panelId) => {
      void invoke('cancelar_turno', { panel_id: panelId }).catch(() => {});
    },
    responderAprobacion: (id, respuesta) =>
      invoke<void>('responder_aprobacion', { id, respuesta }),
    pendientesAprobacion: () => invoke<unknown[]>('pendientes_aprobacion'),
    requiereReenvioTrasAprobar: () => true,
    escucharTurno: async (onEvento, onFin) => {
      await listen<AgenteEvento>('agente-evento', (e) => onEvento(e.payload));
      await listen<{ ok: boolean; error?: string }>('turno-fin', (e) =>
        onFin(e.payload.ok, e.payload.error),
      );
    },
    convNueva: (titulo, panelId) =>
      invoke<InfoConversacion>('conversacion_nueva', { titulo, panel_id: panelId }),
    convListar: () => invoke<InfoConversacion[]>('listar_conversaciones'),
    convCargar: (id, panelId) =>
      invoke<CargaConversacion>('cargar_conversacion', { id, panel_id: panelId }),
    convRenombrar: (id, titulo) => invoke<boolean>('renombrar_conversacion', { id, titulo }),
    convArchivar: (id, archivada) => invoke<boolean>('archivar_conversacion', { id, archivada }),
    convEliminar: (id, panelId) =>
      invoke<InfoConversacion>('eliminar_conversacion', { id, panel_id: panelId }),
    convRewind: (hastaMensajeId, editar, panelId) =>
      invoke<CargaConversacion>('rewind_conversacion', {
        hastaMensajeId,
        editar,
        panel_id: panelId,
      }),
    tramoRestaurar: (panelId) =>
      invoke<ResultadoRestauracionTramo>('restaurar_archivos_tramo', { panel_id: panelId }),
    leerProveedores: () => invoke<ProveedorInfo[]>('proveedores_disponibles'),
    leerConfig: (clave) => invoke<string | null>('config_leer', { clave }),
    guardarConfig: (clave, valor) => invoke<void>('config_guardar', { clave, valor }),
    elegirWorkspace: () => invoke<InfoSesion>('elegir_workspace'),
    fijarWorkspace: () =>
      Promise.reject(
        new Error('en Tauri el workspace se elige con el diálogo nativo (elegirWorkspace)'),
      ),
    fijarMeta: (meta) => invoke<string | null>('actualizar_meta', { meta }),
    // [069A-Proyectos] Tauri: invoke directo a comandos del backend.
    workspacesListar: () =>
      invoke<{ workspaces: Workspace[]; activa: Workspace | null }>('workspaces_listar'),
    proyectoGuardar: (nombre, ruta) =>
      invoke<InfoSesion>('workspace_crear_o_activar', { nombre, ruta }),
    workspaceActivarsPorRuta: (ruta) =>
      invoke<InfoSesion>('workspace_activar_por_ruta', { ruta }),
    workspaceRenombrar: (id, nombre) =>
      invoke<boolean>('workspace_renombrar', { id, nombre }),
    workspaceEliminar: (id) => invoke<boolean>('workspace_eliminar', { id }),
    workspaceInfo: () => invoke<{ ruta: string; nombre: string }>('workspace_info'),
    workspaceListarEntrada: (ruta, profundidad) =>
      invoke<ListadoWorkspace>('workspace_listar_entrada', { rutaRelativa: ruta, profundidad }),
    workspaceLeerArchivo: (ruta) =>
      invoke<{ ruta: string; lineas: number; contenido: string }>('workspace_leer_archivo', {
        rutaRelativa: ruta,
      }),
    workspaceAbrirCon: (ruta) =>
      invoke<void>('workspace_abrir_con', { rutaRelativa: ruta }),
    workspaceBuscar: (consulta, ruta) =>
      invoke<ResultadoBusqueda>('workspace_buscar', {
        consulta,
        rutaRelativa: ruta ?? null,
      }),
    workspaceGitEstado: () =>
      invoke<import('../componentes/panelGit').EstadoGit>('workspace_git_estado'),
  };
}

const RESPUESTA: Record<DecisionAprobacion, string> = {
  aprobar: 'aprobar',
  permitir: 'siempre',
  denegar: 'rechazar',
};

type ArgumentosTool = Record<string, unknown>;

function argumentosDeTool(argumentos: unknown): ArgumentosTool | null {
  return argumentos && typeof argumentos === 'object' && !Array.isArray(argumentos)
    ? argumentos as ArgumentosTool
    : null;
}

function rutaDeArgumentos(argumentos: unknown): string | null {
  const objeto = argumentosDeTool(argumentos);
  const ruta = objeto?.ruta;
  return typeof ruta === 'string' && ruta.trim() ? ruta : null;
}

function textoDeArgumento(argumentos: unknown, clave: string, max = 72): string | null {
  const valor = argumentosDeTool(argumentos)?.[clave];
  if (typeof valor !== 'string' || !valor.trim()) return null;
  const texto = valor.trim().replace(/\s+/g, ' ');
  return texto.length > max ? `${texto.slice(0, max - 1)}…` : texto;
}

function rangoDeLectura(argumentos: unknown): string {
  const objeto = argumentosDeTool(argumentos);
  const inicio = objeto?.offset_linea;
  const limite = objeto?.limite_lineas;
  if (
    typeof inicio === 'number' && Number.isInteger(inicio) && inicio >= 1 &&
    typeof limite === 'number' && Number.isInteger(limite) && limite >= 1
  ) {
    return `líneas ${inicio}-${inicio + limite - 1}`;
  }
  if (typeof inicio === 'number' && Number.isInteger(inicio) && inicio >= 1) {
    return `desde línea ${inicio}`;
  }
  return '';
}

export function descripcionDeTool(tool: string, argumentos?: unknown): string {
  const ruta = rutaDeArgumentos(argumentos);
  switch (tool) {
    case 'file_write':
      return ruta ? `Modificando ${ruta} · archivo completo` : 'Modificando archivo completo';
    case 'file_patch':
      return ruta ? `Modificando ${ruta} · líneas modificadas` : 'Modificando archivo · líneas modificadas';
    case 'file_read': {
      const rango = rangoDeLectura(argumentos);
      return ruta ? `Leyendo ${ruta}${rango ? ` · ${rango}` : ''}` : `Leyendo archivo${rango ? ` · ${rango}` : ''}`;
    }
    case 'file_search': {
      const patron = textoDeArgumento(argumentos, 'patron');
      return patron ? `Buscando archivos: ${patron}` : 'Buscando archivos';
    }
    case 'web_search': {
      const consulta = textoDeArgumento(argumentos, 'query');
      return consulta ? `Buscando en la web: ${consulta}` : 'Buscando en la web';
    }
    case 'web_fetch': {
      const url = textoDeArgumento(argumentos, 'url');
      return url ? `Leyendo web: ${url}` : 'Leyendo página web';
    }
    case 'comando':
      return 'Ejecutando comando';
    case 'comando_status':
      return 'Consultando comando';
    case 'comando_matar':
      return 'Deteniendo comando';
    case 'navegador_reflejo':
      return 'Usando navegador';
    case 'task':
      return 'Ejecutando tarea';
    case 'todo':
      return 'Actualizando tareas';
    default:
      return `Ejecutando ${tool.replaceAll('_', ' ')}`;
  }
}

function iconoDeTool(tool: string): IconoNombre {
  if (tool.startsWith('file_')) return 'archivo';
  if (tool === 'web_search') return 'globo';
  if (tool.startsWith('comando')) return 'terminal';
  if (tool === 'task' || tool === 'todo') return 'flujo';
  return 'lupa';
}
export { iconoDeTool };

function compacto(valor: unknown, max = 500): string {
  const s = typeof valor === 'string' ? valor : JSON.stringify(valor);
  return s.length > max ? s.slice(0, max) + '…' : s;
}

export function esEntornoTauri(): boolean {
  return typeof (window as unknown as { __TAURI__?: unknown }).__TAURI__ !== 'undefined';
}

export function crearAdaptadorReal(hooks: HooksAdaptador = {}, transporte: Transporte = transporteTauri()) {
  let escuchando = false;
  let sesionAbierta = false;
  let claveSesion = '';
  let mensajes: HTMLElement | null = null;
  let onFin: (() => void) | null = null;
  let asistente: AsistenteVivo | null = null;
  let herramienta: HerramientaViva | null = null;
  // [089A-2] Ruta de la tool de archivo en curso (para el tab Cambios).
  let rutaHerramienta: string | null = null;
  let ultimoMensaje = '';
  let huboPeticiones = false;
  let cerrado = false;
  let uso: UsoTurno = {
    tokensPrompt: 0,
    tokensComplecion: 0,
    ocupacionPct: null,
    modelo: null,
    maxVentana: null,
    reservaSalida: null,
    totalEntrada: null,
  };
  // [039A-3 P1] Cómo terminó el último turno (para que el pie distinga
  // un corte real de un cancelado/error, que no muestran tokens de fin).
  let ultimoResultado: ResultadoTurno = 'ok';

  function aviso(texto: string, meta: string, detalle: string): void {
    mensajes?.appendChild(crearAvisoSistema(texto, meta, detalle));
    bajarScroll();
  }

  /** [039A-1 04-09 H2] Baja el scroll del chat al fondo (contenedor real). */
  function bajarScroll(): void {
    if (mensajes) mensajes.scrollTop = mensajes.scrollHeight;
  }

  function asistenteVivo(): AsistenteVivo {
    if (!asistente) {
      asistente = crearMensajeAsistenteVivo('');
      mensajes?.appendChild(asistente.raiz);
      bajarScroll();
    }
    herramienta = null;
    return asistente;
  }

  function aplicar(ev: AgenteEvento): void {
    switch (ev.tipo) {
      case 'token': {
        const a = asistenteVivo();
        a.nodo.insertBefore(document.createTextNode(ev.texto), a.cursor);
        // La respuesta fluye bajo el mensaje del usuario: mantener visible.
        bajarScroll();
        break;
      }
      case 'tool_start': {
        herramienta = crearHerramientaViva(iconoDeTool(ev.tool), descripcionDeTool(ev.tool, ev.argumentos));
        herramienta.ejecutando();
        mensajes?.appendChild(herramienta.raiz);
        bajarScroll();
        asistente = null;
        // [089A-2] Recuerda la ruta si es escritura/parche de archivo.
        rutaHerramienta =
          ev.tool === 'file_write' || ev.tool === 'file_patch'
            ? rutaDeArgumentos(ev.argumentos)
            : null;
        break;
      }
      case 'tool_result': {
        const dif = ev.diff ?? null;
        if (herramienta) {
          const detalle = formatearResultadoHerramienta(ev.resumen, dif);
          if (ev.ok) herramienta.completada('ok', { tipo: 'html', html: detalle });
          else herramienta.errored('falló', { tipo: 'html', html: detalle });
          herramienta = null;
        } else {
          // Sin bloque de herramienta: aviso en texto plano (crearAvisoSistema usa textContent).
          aviso(`${ev.tool} → ${ev.ok ? 'ok' : 'falló'}`, '', dif ? `${ev.resumen}\n${dif}` : ev.resumen);
        }
        // [089A-2] Notifica el cambio al visor (solo escrituras con ruta y diff).
        if (
          (ev.tool === 'file_write' || ev.tool === 'file_patch') &&
          rutaHerramienta &&
          dif
        ) {
          hooks.onCambioArchivo?.({
            origen: 'tool',
            tool: ev.tool,
            ruta: rutaHerramienta,
            titulo: descripcionDeTool(ev.tool, undefined),
            resumen: ev.resumen,
            diff: dif,
          });
        }
        rutaHerramienta = null;
        break;
      }
      case 'peticion_aprobacion': {
        huboPeticiones = true;
        const tarjeta = crearTarjetaAprobacion({
          titulo: `${ev.tool} (clase: ${ev.clasificacion})`,
          argsTexto: compacto(ev.argumentos),
          onDecidir: (decision, t) => {
            void transporte
              .responderAprobacion(ev.id, RESPUESTA[decision])
              .then(() => {
                t.ponerEstado(
                  decision === 'denegar' ? 'denegada por el usuario' : 'aprobada · se ejecuta al reenviar',
                );
                if (decision === 'denegar') t.marcarDenegada();
                t.quitarAcciones();
              })
              .catch((e: unknown) => t.ponerEstado(`no se pudo responder: ${String(e)}`));
          },
        });
        mensajes?.appendChild(tarjeta.raiz);
        break;
      }
      case 'requiere_aprobacion':
        aviso(`${ev.tool} requiere aprobación (tarjeta arriba)`, ev.clasificacion, '');
        break;
      case 'permiso_denegado':
        aviso(`${ev.tool} denegada (${ev.motivo})`, 'permiso', 'el modelo cambia de plan');
        break;
      case 'subagente_inicio':
        aviso(`└ subagente [${ev.perfil}]…`, '', '');
        break;
      case 'subagente_fin':
        aviso(`└ subagente: ${ev.ok ? 'fin' : 'sin resumen'}`, '', '');
        break;
      case 'plan_propuesto':
        aviso(`propuesta del modo plan: ${ev.cambios} cambios pendientes`, 'plan', '');
        break;
      case 'usage':
        uso.tokensPrompt += typeof ev.tokens_prompt === 'number' ? ev.tokens_prompt : 0;
        uso.tokensComplecion += typeof ev.tokens_complecion === 'number' ? ev.tokens_complecion : 0;
        if (typeof ev.ocupacion_pct === 'number') uso.ocupacionPct = ev.ocupacion_pct;
        // [039A-3 P1] El Usage lleva el provider/modelo REAL (tras fallback):
        // se conserva el último que respondió de verdad.
        if (ev.provider && ev.modelo) uso.modelo = `${ev.provider}/${ev.modelo}`;
        // [039A-3 P6] Refresca el indicador en vivo con el % del último uso.
        hooks.onContexto?.({ ...uso });
        break;
      case 'contexto_detalle':
        uso.ocupacionPct = ev.ocupacion_pct;
        uso.maxVentana = ev.max_ventana;
        uso.reservaSalida = ev.reserva_salida;
        uso.totalEntrada = ev.total_entrada;
        // [039A-3 P6] El `ContextoDetalle` es la fuente única del % y la
        // ventana: notifica a la UI para repintar el indicador circular.
        hooks.onContexto?.({ ...uso });
        break;
      case 'contexto':
        break;
      case 'error':
        aviso(`error: ${ev.mensaje}`, ev.retryable ? 'reintentable' : '', '');
        break;
      case 'done':
        asistente = null;
        herramienta = null;
        break;
      case 'tool_navegador':
        hooks.onToolNavegador?.(ev);
        break;
    }
  }

  async function cerrar(ok: boolean, error?: string): Promise<void> {
    if (cerrado) return;
    cerrado = true;
    ultimoResultado = ok ? 'ok' : 'error';
    if (!ok) aviso(`el turno falló: ${error ?? 'desconocido'}`, '', 'puedes reintentar');
    const fin = onFin;
    onFin = null;
    fin?.();
    // Reenvío tras aprobar (paridad REPL): si hubo peticiones y ya no quedan
    // pendientes, el agente ejecuta lo aprobado sin repetir el mensaje.
    // [069A-2 F4] Solo el transporte que lo requiere (Tauri entre turnos);
    // HTTP resuelve en vivo y nunca reenvía.
    if (ok && huboPeticiones && ultimoMensaje && transporte.requiereReenvioTrasAprobar()) {
      try {
        const pendientes = await transporte.pendientesAprobacion();
        if (pendientes.length === 0) {
          const reintento = ultimoMensaje;
          aviso('aprobaciones resueltas: se reenvía el mensaje', '', '');
          await montar(mensajes, reintento, ultimaOpcion, () => {});
        }
      } catch (e: unknown) {
        aviso(`no se pudo verificar aprobaciones: ${String(e)}`, '', '');
      }
    }
  }

  let ultimaOpcion: OpcionesTurno = { proveedor: '', modelo: '', modo: '', razonamiento: '' };

  /**
   * Abre la sesión si no existe o reconfigura si cambió provider/modelo/modo/
   * razonamiento. Devuelve la info cuando hubo apertura o cambio (`null` = ya
   * estaba acorde, sin roundtrip). La conversación se conserva salvo apertura.
   */
  async function asegurarSesion(opts: OpcionesTurno): Promise<InfoSesion | null> {
    const clave = `${opts.proveedor}|${opts.modelo}|${opts.modo}|${opts.razonamiento}`;
    if (!sesionAbierta) {
      const info = await transporte.abrirSesion(opts);
      sesionAbierta = true;
      claveSesion = clave;
      if (info.aviso) aviso(info.aviso, 'persistencia', '');
      hooks.onSesion?.(info);
      return info;
    }
    if (clave !== claveSesion) {
      // Cambio de modelo/modo/razonamiento: reconfigura SIN perder la
      // conversación (abrir_sesion crearía una conversación nueva vacía).
      const info = await transporte.reconfigurarSesion(opts);
      claveSesion = clave;
      hooks.onSesion?.(info);
      return info;
    }
    return null;
  }

  async function montar(
    contenedor: HTMLElement | null,
    texto: string,
    opts: OpcionesTurno,
    fin: () => void,
  ): Promise<void> {
    mensajes = contenedor;
    onFin = fin;
    cerrado = false;
    asistente = null;
    herramienta = null;
    huboPeticiones = false;
    ultimoResultado = 'ok';
    uso = {
      tokensPrompt: 0,
      tokensComplecion: 0,
      ocupacionPct: null,
      modelo: null,
      maxVentana: null,
      reservaSalida: null,
      totalEntrada: null,
    };
    ultimaOpcion = opts;
    ultimoMensaje = texto;
    mensajes?.appendChild(crearMensajeUsuario(texto));
    // [039A-1 04-09 H2] El mensaje del usuario debe verse al enviar: baja el
    // scroll al final (puede que el contenedor no estuviera al fondo).
    if (mensajes) mensajes.scrollTop = mensajes.scrollHeight;
    try {
      if (!escuchando) {
        await transporte.escucharTurno(
          (ev) => aplicar(ev),
          (ok, error) => void cerrar(ok, error),
        );
        escuchando = true;
      }
      await asegurarSesion(opts);
      await transporte.enviarTurno(texto, opts.panelId ?? null);
    } catch (e: unknown) {
      await cerrar(false, String(e));
    }
  }

  function detener(): void {
    // El backend aborta, marca el turno `cancelado` y emite `turno-fin`
    // (ok:false). El cierre local es optimista; el `turno-fin` tardío se
    // ignora por el flag `cerrado`. [039A-3 P5] Cancela el turno del panel
    // que lanzó `montar` (el adaptador guarda la última `OpcionesTurno`).
    transporte.detenerTurno(ultimaOpcion.panelId ?? null);
    if (!cerrado) {
      cerrado = true;
      ultimoResultado = 'cancelado';
      const fin = onFin;
      onFin = null;
      const n = el('div', 'msg-asis');
      n.textContent = '[turno cancelado por el usuario]';
      mensajes?.appendChild(n);
      fin?.();
    }
  }

  /** Uso acumulado del turno en curso (tokens reales del núcleo). */
  function usoUltimoTurno(): UsoTurno {
    return { ...uso };
  }

  /** [039A-3 P1] Cómo terminó el último turno (`ok`|`error`|`cancelado`). */
  function resultadoUltimoTurno(): ResultadoTurno {
    return ultimoResultado;
  }

  return {
    montar,
    detener,
    usoUltimoTurno,
    resultadoUltimoTurno,
    /** Abre la sesión si aún no existe (para listar/cargar al arrancar). */
    asegurarSesion,
    sesion: {
      /** [039A-3 P5] Crea una conversación en el panel dado (default
       * 'principal'). Devuelve la conversación nueva. */
      async nueva(titulo?: string, panelId?: string): Promise<InfoConversacion> {
        return transporte.convNueva(titulo ?? null, panelId ?? null);
      },
      async listar(): Promise<InfoConversacion[]> {
        return transporte.convListar();
      },
      /** [039A-3 P5] Carga una conversación en el panel dado. */
      async cargar(id: string, panelId?: string): Promise<CargaConversacion> {
        return transporte.convCargar(id, panelId ?? null);
      },
      async renombrar(id: string, titulo: string): Promise<boolean> {
        return transporte.convRenombrar(id, titulo);
      },
      async archivar(id: string, archivada: boolean): Promise<boolean> {
        return transporte.convArchivar(id, archivada);
      },
      /** [039A-3 P5] Si era la actual del panel, el backend ancla la más
       * reciente restante y la devuelve; si no queda ninguna, `null`
       * (borrador create-on-write, sin fila fantasma). [069A-7] */
      async eliminar(id: string, panelId?: string): Promise<InfoConversacion | null> {
        return transporte.convEliminar(id, panelId ?? null);
      },
      /** [039A-3 P2] Borra el hilo posterior a un mensaje de usuario y
       * devuelve la conversación recién recortada. `editar=false` conserva el
       * mensaje objetivo ("volver a este punto"); `editar=true` lo borra
       * también (se reescribe al reenviar desde el modo edición).
       * [039A-3 P5] Opera sobre el panel dado. */
      async rewind(
        hastaMensajeId: string,
        editar: boolean,
        panelId?: string,
      ): Promise<CargaConversacion> {
        return transporte.convRewind(hastaMensajeId, editar, panelId ?? null);
      },
      /** [039A-3 P3] Restaura los archivos del último tramo rebobinado (acción
       * EXPLÍCITA tras "volver a punto"). Falla si no hay tramo pendiente.
       * [039A-3 P5] Opera sobre el tramo del panel dado. */
      async restaurarTramo(panelId?: string): Promise<ResultadoRestauracionTramo> {
        return transporte.tramoRestaurar(panelId ?? null);
      },
      async proveedores(): Promise<ProveedorInfo[]> {
        return transporte.leerProveedores();
      },
      async configLeer(clave: string): Promise<string | null> {
        return transporte.leerConfig(clave);
      },
      async configGuardar(clave: string, valor: string): Promise<void> {
        await transporte.guardarConfig(clave, valor);
      },
      /**
       * Diálogo nativo de carpeta. Reabre la sesión (conversación nueva).
       * Si el usuario cancela, devuelve la sesión actual sin cambios.
       */
      async elegirWorkspace(): Promise<InfoSesion> {
        const info = await transporte.elegirWorkspace();
        sesionAbierta = true;
        hooks.onSesion?.(info);
        return info;
      },
      /** [069A-2 F4] Fija el workspace por ruta (modo web). En Tauri se
       * rechaza: el diálogo nativo es el dueño. */
      async fijarWorkspace(ruta: string): Promise<InfoSesion> {
        const info = await transporte.fijarWorkspace(ruta);
        sesionAbierta = true;
        hooks.onSesion?.(info);
        return info;
      },
      async actualizarMeta(meta: string | null): Promise<string | null> {
        return transporte.fijarMeta(meta);
      },
      // [069A-Proyectos] Proyectos (áreas de trabajo).
      workspaces: {
        async listar(): Promise<{ workspaces: Workspace[]; activa: Workspace | null }> {
          return transporte.workspacesListar();
        },
        async guardarProyecto(nombre: string, ruta: string): Promise<InfoSesion> {
          const info = await transporte.proyectoGuardar(nombre, ruta);
          sesionAbierta = true;
          hooks.onSesion?.(info);
          return info;
        },
        async activarPorRuta(ruta: string): Promise<InfoSesion> {
          const info = await transporte.workspaceActivarsPorRuta(ruta);
          sesionAbierta = true;
          hooks.onSesion?.(info);
          return info;
        },
        async renombrar(id: string, nombre: string): Promise<boolean> {
          return transporte.workspaceRenombrar(id, nombre);
        },
        async eliminar(id: string): Promise<boolean> {
          return transporte.workspaceEliminar(id);
        },
      },
      filesystem: {
        async info(): Promise<{ ruta: string; nombre: string }> {
          return transporte.workspaceInfo();
        },
        async listar(ruta: string, profundidad?: number): Promise<ListadoWorkspace> {
          return transporte.workspaceListarEntrada(ruta, profundidad);
        },
        async leer(ruta: string): Promise<{ ruta: string; lineas: number; contenido: string }> {
          return transporte.workspaceLeerArchivo(ruta);
        },
        async abrirCon(ruta: string): Promise<void> {
          return transporte.workspaceAbrirCon(ruta);
        },
        async buscar(consulta: string, ruta?: string): Promise<ResultadoBusqueda> {
          return transporte.workspaceBuscar(consulta, ruta);
        },
        async gitEstado(): Promise<import('../componentes/panelGit').EstadoGit> {
          return transporte.workspaceGitEstado();
        },
      },
    },
  };
}

export type AdaptadorReal = ReturnType<typeof crearAdaptadorReal>;
