// Transporte HTTP/SSE del modo web (plan 069A-2, F4): la misma superficie
// `AdaptadorReal` sobre `glory-harness web`. El render vive una sola vez en
// `tauri/real.ts`; aquí solo viaja: fetch + EventSource con cookie.
// Supuestos: la UI se sirve del MISMO origen que la API (el servidor sirve
// la UI compilada) o del webview Tauri→localhost; la cookie `gh_sesion`
// (HttpOnly, SameSite=Lax) autoriza el SSE que EventSource no puede firmar.
// El token maestro viaja en `?token=` (solo memoria, nunca localStorage).

import {
  crearAdaptadorReal,
  type AdaptadorReal,
  type CargaConversacion,
  type HooksAdaptador,
  type InfoConversacion,
  type InfoSesion,
  type OpcionesTurno,
  type ProveedorInfo,
  type Transporte,
} from '../tauri/real';
import type { EstadoGit } from '../componentes/panelGit';
import type { ListadoWorkspace, ResultadoBusqueda, Workspace } from '../dominio/tipos';
import { crearClienteApi } from './apiCliente';

/** Claves que viven en el servidor; el resto cae a localStorage (igual que
 * el mock): la superficie configLeer/Guardar no cambia. */
const CLAVES_SERVIDOR = new Set([
  'proveedor',
  'modelo',
  'modo',
  'nivelRazonamiento',
  'contexto_max_ventana',
  'gancho_pre_compact',
  'workspace',
]);

export function crearTransporteApi(base: string, hooks: HooksAdaptador = {}): Transporte {
  // HTTP/SSE + estado compartido (sid/turno/último info) en `apiCliente`.
  const cliente = crearClienteApi(base, hooks);
  const http = cliente.http;
  const recordar = cliente.recordar;

  interface SesionCreada extends InfoSesion {
    session_id: string;
  }

  return {
    abrirSesion: async (_opts: OpcionesTurno) => {
      const creada = await http<SesionCreada>('POST', '/api/v1/session');
      cliente.setSid(creada.session_id);
      return recordar({
        modelo: creada.modelo,
        workspace: creada.workspace,
        proveedores: creada.proveedores,
        conversacion: creada.conversacion,
        aviso: creada.aviso,
      });
    },
    reconfigurarSesion: async (opts: OpcionesTurno) => {
      const parche: Record<string, unknown> = {};
      if (opts.proveedor) parche['provider'] = opts.proveedor;
      if (opts.modelo) parche['modelo'] = opts.modelo;
      if (opts.modo) parche['modo'] = opts.modo;
      if (opts.razonamiento) parche['razonamiento'] = opts.razonamiento;
      await http('PATCH', `/api/v1/session/${cliente.getSid()}/config`, parche);
      return cliente.componerSesion();
    },
    enviarTurno: async (mensaje, _panelId) => {
      const r = await http<{ turn_id: string }>('POST', `/api/v1/session/${cliente.getSid()}/turns`, {
        message: mensaje,
      });
      cliente.setUltimoTurno(r.turn_id);
    },
    detenerTurno: (_panelId) => {
      const tid = cliente.getUltimoTurno();
      if (!tid) return;
      void http('POST', `/api/v1/session/${cliente.getSid()}/turns/${tid}/cancel`).catch(() => {});
    },
    responderAprobacion: async (id, respuesta) => {
      await http('POST', `/api/v1/session/${cliente.getSid()}/approvals/${id}`, {
        approved: respuesta !== 'rechazar',
        siempre: respuesta === 'siempre' ? true : undefined,
      });
    },
    pendientesAprobacion: () => Promise.resolve([]),
    // HTTP resuelve aprobaciones en vivo durante el turno: nunca reenvía.
    requiereReenvioTrasAprobar: () => false,
    escucharTurno: async (onEvento, onFin) => {
      cliente.abrirFuente(onEvento, onFin);
    },
    convNueva: async (titulo) => {
      const r = await http<{ conversacion: InfoConversacion }>(
        'POST',
        `/api/v1/session/${cliente.getSid()}/conversations`,
        titulo ? { titulo } : {},
      );
      return r.conversacion;
    },
    convListar: async () => {
      const r = await http<{ conversaciones: InfoConversacion[] }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/conversations`,
      );
      return r.conversaciones;
    },
    convCargar: async (id) => {
      const r = await http<CargaConversacion>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/conversations/${id}/messages`,
      );
      return { ...r, archivos_tramo: [] };
    },
    convRenombrar: async (id, titulo) => {
      const r = await http<{ ok: boolean }>(
        'PATCH',
        `/api/v1/session/${cliente.getSid()}/conversations/${id}`,
        { titulo },
      );
      return r.ok;
    },
    convArchivar: async (id, archivada) => {
      const r = await http<{ ok: boolean }>(
        'PATCH',
        `/api/v1/session/${cliente.getSid()}/conversations/${id}`,
        { archivada },
      );
      return r.ok;
    },
    convEliminar: async (id) => {
      const r = await http<{ actual: InfoConversacion | null }>(
        'DELETE',
        `/api/v1/session/${cliente.getSid()}/conversations/${id}`,
      );
      return r.actual;
    },
    convRewind: () =>
      Promise.reject(new Error('volver a un punto no disponible en modo web (fase 069A-2)')),
    tramoRestaurar: () =>
      Promise.reject(new Error('restaurar archivos no disponible en modo web (fase 069A-2)')),
    leerProveedores: async () => {
      const r = await http<{ proveedores: Array<{ nombre: string; disponible: boolean }> }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/providers`,
      );
      return r.proveedores.map(
        (p): ProveedorInfo => ({ id: p.nombre, modelos: [], claves: p.disponible ? 1 : 0 }),
      );
    },
    leerConfig: async (clave) => {
      if (!CLAVES_SERVIDOR.has(clave)) {
        try {
          return window.localStorage.getItem(clave);
        } catch {
          return null;
        }
      }
      if (clave === 'workspace') {
        const r = await http<{ workspace: string }>('GET', `/api/v1/session/${cliente.getSid()}/workspace`);
        return r.workspace;
      }
      const r = await http<{ config: Record<string, unknown> }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/config`,
      );
      const c = r.config;
      switch (clave) {
        case 'proveedor':
          return (c['provider'] as string) ?? null;
        case 'modelo':
          return (c['modelo'] as string) ?? null;
        case 'modo':
          return (c['modo'] as string) ?? null;
        case 'nivelRazonamiento':
          return (c['razonamiento'] as string) ?? null;
        case 'contexto_max_ventana':
          return c['max_ventana'] === undefined ? null : String(c['max_ventana']);
        case 'gancho_pre_compact': {
          const hook = c['gancho_pre_compact'];
          return hook === undefined || hook === null ? null : JSON.stringify(hook);
        }
        default:
          return null;
      }
    },
    guardarConfig: async (clave, valor) => {
      if (!CLAVES_SERVIDOR.has(clave)) {
        try {
          window.localStorage.setItem(clave, valor);
        } catch {
          /* sin persistencia local */
        }
        return;
      }
      if (clave === 'workspace') {
        await http('POST', `/api/v1/session/${cliente.getSid()}/workspace`, { ruta: valor });
        return;
      }
      const parche: Record<string, unknown> = {};
      if (clave === 'nivelRazonamiento') parche['razonamiento'] = valor;
      else if (clave === 'contexto_max_ventana') parche['max_ventana'] = Number(valor);
      else if (clave === 'gancho_pre_compact') {
        const texto = valor.trim();
        if (texto === '' || texto === 'null') {
          parche['gancho_pre_compact'] = null;
        } else {
          try {
            parche['gancho_pre_compact'] = JSON.parse(texto);
          } catch {
            throw new Error('gancho_pre_compact debe ser JSON válido');
          }
        }
      }
      /* [FG1-069A-10 F2] El backend espera `provider` (ParcheConfig), no
       * `proveedor` (clave del front en español). */
      else if (clave === 'proveedor') parche['provider'] = valor;
      else parche[clave] = valor;
      await http('PATCH', `/api/v1/session/${cliente.getSid()}/config`, parche);
    },
    elegirWorkspace: () =>
      Promise.reject(
        new Error('en modo web la ruta se fija con fijarWorkspace (campo de texto)'),
      ),
    // [069A-Proyectos] Extraída a función compartida para workspaceActivarsPorRuta.
    fijarWorkspace: async (ruta) => cliente.fijarWorkspaceImpl(ruta),
    fijarMeta: async (meta) => {
      const r = await http<{ ok: boolean; meta: string | null }>(
        'PATCH',
        `/api/v1/session/${cliente.getSid()}/meta`,
        { meta },
      );
      return r.meta;
    },
    // [069A-Proyectos] HTTP workspaces
    workspacesListar: async () => {
      const r = await http<{ ok: boolean; workspaces: Workspace[]; activa: Workspace | null }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/workspaces`,
      );
      return { workspaces: r.workspaces, activa: r.activa };
    },
    proyectoGuardar: async (nombre, ruta) => {
      // Web: POST /workspaces crea la fila + POST /workspace la activa
      const creada = await http<{ ok: boolean; activa: Workspace; creada: Workspace; workspace: string }>(
        'POST',
        `/api/v1/session/${cliente.getSid()}/workspaces`,
        { nombre, ruta },
      );
      const base_info: InfoSesion = cliente.getUltimoInfo() ?? {
        modelo: '/',
        workspace: creada.workspace,
        proveedores: [],
        conversacion: null,
      };
      // El backend ya cambió el workspace de la sesión; componer info limpia.
      const info: InfoSesion = {
        ...base_info,
        workspace: creada.workspace,
      };
      return recordar(info);
    },
    workspaceActivarsPorRuta: async (ruta) => {
      // Reusa fijarWorkspace (POST /workspace) que cambia la ruta activa.
      return cliente.fijarWorkspaceImpl(ruta);
    },
    workspaceRenombrar: async (id, nombre) => {
      const r = await http<{ ok: boolean; renombrada: boolean }>(
        'PATCH',
        `/api/v1/session/${cliente.getSid()}/workspaces/${encodeURIComponent(id)}`,
        { nombre },
      );
      return r.renombrada;
    },
    workspaceEliminar: async (id) => {
      const r = await http<{ ok: boolean; eliminada: boolean }>(
        'DELETE',
        `/api/v1/session/${cliente.getSid()}/workspaces/${encodeURIComponent(id)}`,
      );
      return r.eliminada;
    },
    workspaceInfo: async () =>
      http<{ ruta: string; nombre: string }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/info`,
      ),
    workspaceListarEntrada: async (ruta, profundidad) => {
      // [089A-10] El backend limita profundidad a 0..=3; el panel Files usa
      // profundidad 1 para la raíz (carpetas expandibles) y 0 para el resto.
      const nivel = profundidad && profundidad > 0 ? `&profundidad=${profundidad}` : '';
      return http<ListadoWorkspace>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/listar?ruta=${encodeURIComponent(ruta)}${nivel}`,
      );
    },
    workspaceLeerArchivo: async (ruta) =>
      http<{ ruta: string; lineas: number; contenido: string }>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/leer?ruta=${encodeURIComponent(ruta)}`,
      ),
    workspaceAbrirCon: async () => {
      throw new Error('abrir archivos con otra aplicación no está disponible en modo web');
    },
    workspaceBuscar: async (consulta, ruta) => {
      const extra = ruta ? `&ruta=${encodeURIComponent(ruta)}` : '';
      return http<ResultadoBusqueda>(
        'GET',
        `/api/v1/session/${cliente.getSid()}/files/buscar?consulta=${encodeURIComponent(consulta)}${extra}`,
      );
    },
    workspaceGitEstado: async (): Promise<EstadoGit> =>
      http<EstadoGit>('GET', `/api/v1/session/${cliente.getSid()}/git/estado`),
  };
}

/** Adaptador web: misma superficie `AdaptadorReal` sobre HTTP/SSE. */
export function crearAdaptadorApi(base: string, hooks: HooksAdaptador = {}): AdaptadorReal {
  return crearAdaptadorReal(hooks, crearTransporteApi(base, hooks));
}
