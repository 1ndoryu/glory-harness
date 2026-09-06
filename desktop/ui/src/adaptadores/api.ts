// ============================================================
// Transporte HTTP/SSE del modo web (plan 069A-2, F4): la misma superficie
// `AdaptadorReal` sobre `glory-harness web`. El render vive una sola vez en
// `tauri/real.ts`; aquí solo viaja: fetch + EventSource con cookie.
// Supuestos: la UI se sirve del MISMO origen que la API (el servidor sirve
// la UI compilada) o del webview Tauri→localhost; la cookie `gh_sesion`
// (HttpOnly, SameSite=Lax) autoriza el SSE que EventSource no puede firmar.
// El token maestro viaja en `?token=` (solo memoria, nunca localStorage).
// ============================================================

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

/** Claves que viven en el servidor; el resto cae a localStorage (igual que
 * el mock): la superficie configLeer/Guardar no cambia. */
const CLAVES_SERVIDOR = new Set([
  'proveedor',
  'modelo',
  'modo',
  'nivelRazonamiento',
  'contexto_max_ventana',
  'workspace',
]);

function tokenMaestro(): string {
  return new URLSearchParams(window.location.search).get('token') ?? '';
}

function errorHttp(ruta: string, estado: number, cuerpo: string): Error {
  let codigo = `http_${estado}`;
  let mensaje = cuerpo.slice(0, 300);
  try {
    const j = JSON.parse(cuerpo) as { code?: string; message?: string };
    if (j.code) codigo = j.code;
    if (j.message) mensaje = j.message;
  } catch {
    /* cuerpo no-JSON: se usa tal cual */
  }
  const e = new Error(`${ruta}: ${mensaje}`);
  (e as Error & { codigo?: string }).codigo = codigo;
  return e;
}

export function crearTransporteApi(base: string, hooks: HooksAdaptador = {}): Transporte {
  const maestro = tokenMaestro();
  let sid = '';
  let ultimoTurno: string | null = null;
  let fuente: EventSource | null = null;
  let ultimoInfo: InfoSesion | null = null;
  let ultimoAviso: string | null = null;

  const avisarConexion = hooks.onConexion;

  async function http<T>(metodo: string, ruta: string, cuerpo?: unknown): Promise<T> {
    const cabeceras: Record<string, string> = { 'Content-Type': 'application/json' };
    // La sesión opaca autoriza; el maestro SOLO crea sesiones (§5.2/§7).
    if (ruta === '/api/v1/session') {
      if (!maestro) throw new Error('falta el token maestro (?token=...) para abrir sesión');
      cabeceras['Authorization'] = `Bearer ${maestro}`;
    } else if (sid) {
      cabeceras['Authorization'] = `Bearer ${sid}`;
    }
    let r: Response;
    try {
      r = await fetch(base + ruta, {
        method: metodo,
        credentials: 'same-origin',
        headers: cabeceras,
        body: cuerpo === undefined ? undefined : JSON.stringify(cuerpo),
      });
    } catch (e: unknown) {
      avisarConexion?.('error', `sin conexión con ${base}: ${String(e)}`);
      throw new Error(`sin conexión con el backend (${base})`);
    }
    if (!r.ok) throw errorHttp(ruta, r.status, await r.text());
    return (await r.json()) as T;
  }

  interface SesionCreada extends InfoSesion {
    session_id: string;
  }

  function recordar(info: InfoSesion, aviso?: string | null): InfoSesion {
    ultimoInfo = info;
    if (aviso !== undefined) ultimoAviso = aviso;
    return info;
  }

  async function componerSesion(): Promise<InfoSesion> {
    // PATCH config devuelve solo config: se compone la InfoSesion completa.
    const [cfg, provs] = await Promise.all([
      http<{ config: Record<string, unknown> }>('GET', `/api/v1/session/${sid}/config`),
      http<{ proveedores: Array<{ nombre: string; disponible: boolean }> }>(
        'GET',
        `/api/v1/session/${sid}/providers`,
      ),
    ]);
    const c = cfg.config;
    const info: InfoSesion = {
      modelo: `${String(c['provider'] ?? '')}/${String(c['modelo'] ?? '')}`,
      workspace: String(c['workspace'] ?? ultimoInfo?.workspace ?? ''),
      proveedores: provs.proveedores.map((p) => ({ nombre: p.nombre, claves: p.disponible ? 1 : 0 })),
      conversacion: ultimoInfo?.conversacion ?? {
        id: '',
        titulo: '',
        archivada: false,
        actualizada_en: '',
      },
      aviso: ultimoAviso,
    };
    return recordar(info);
  }

  function abrirFuente(
    onEvento: (ev: import('../tauri/real').AgenteEvento) => void,
    onFin: (ok: boolean, error?: string) => void,
  ): void {
    if (fuente) return;
    avisarConexion?.('conectando');
    const es = new EventSource(`${base}/api/v1/session/${sid}/events`);
    fuente = es;
    es.onopen = () => avisarConexion?.('en-linea');
    es.onerror = () => avisarConexion?.('reconectando', 'el navegador reintenta solo');
    const evento = (tipo: string, fn: (d: unknown) => void) => {
      es.addEventListener(tipo, (e) => {
        try {
          fn(JSON.parse((e as MessageEvent).data) as unknown);
        } catch {
          /* frame corrupto: se ignora sin romper el stream */
        }
      });
    };
    evento('agent.event', (d) => onEvento(d as import('../tauri/real').AgenteEvento));
    evento('turn.started', (d) => {
      const t = (d as { turn_id?: string }).turn_id;
      if (t) ultimoTurno = t;
    });
    evento('turn.finished', (d) => {
      const f = d as { turn_id?: string; ok?: boolean; error?: string };
      ultimoTurno = null;
      onFin(f.ok === true, f.error ?? undefined);
    });
    evento('error', (d) => {
      const f = (d as { code?: string; message?: string }) ?? {};
      onEvento({ tipo: 'error', mensaje: f.message ?? f.code ?? 'error', retryable: true } as never);
    });
  }

  return {
    abrirSesion: async (_opts: OpcionesTurno) => {
      const creada = await http<SesionCreada>('POST', '/api/v1/session');
      sid = creada.session_id;
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
      await http('PATCH', `/api/v1/session/${sid}/config`, parche);
      return componerSesion();
    },
    enviarTurno: async (mensaje, _panelId) => {
      const r = await http<{ turn_id: string }>('POST', `/api/v1/session/${sid}/turns`, {
        message: mensaje,
      });
      ultimoTurno = r.turn_id;
    },
    detenerTurno: (_panelId) => {
      if (!ultimoTurno) return;
      const tid = ultimoTurno;
      void http('POST', `/api/v1/session/${sid}/turns/${tid}/cancel`).catch(() => {});
    },
    responderAprobacion: async (id, respuesta) => {
      await http('POST', `/api/v1/session/${sid}/approvals/${id}`, {
        approved: respuesta !== 'rechazar',
        siempre: respuesta === 'siempre' ? true : undefined,
      });
    },
    pendientesAprobacion: () => Promise.resolve([]),
    // HTTP resuelve aprobaciones en vivo durante el turno: nunca reenvía.
    requiereReenvioTrasAprobar: () => false,
    escucharTurno: async (onEvento, onFin) => {
      abrirFuente(onEvento, onFin);
    },
    convNueva: async (titulo) => {
      const r = await http<{ conversacion: InfoConversacion }>(
        'POST',
        `/api/v1/session/${sid}/conversations`,
        titulo ? { titulo } : {},
      );
      return r.conversacion;
    },
    convListar: async () => {
      const r = await http<{ conversaciones: InfoConversacion[] }>(
        'GET',
        `/api/v1/session/${sid}/conversations`,
      );
      return r.conversaciones;
    },
    convCargar: async (id) => {
      const r = await http<CargaConversacion>(
        'GET',
        `/api/v1/session/${sid}/conversations/${id}/messages`,
      );
      return { ...r, archivos_tramo: [] };
    },
    convRenombrar: async (id, titulo) => {
      const r = await http<{ ok: boolean }>(
        'PATCH',
        `/api/v1/session/${sid}/conversations/${id}`,
        { titulo },
      );
      return r.ok;
    },
    convArchivar: async (id, archivada) => {
      const r = await http<{ ok: boolean }>(
        'PATCH',
        `/api/v1/session/${sid}/conversations/${id}`,
        { archivada },
      );
      return r.ok;
    },
    convEliminar: async (id) => {
      const r = await http<{ actual: InfoConversacion }>(
        'DELETE',
        `/api/v1/session/${sid}/conversations/${id}`,
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
        `/api/v1/session/${sid}/providers`,
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
        const r = await http<{ workspace: string }>('GET', `/api/v1/session/${sid}/workspace`);
        return r.workspace;
      }
      const r = await http<{ config: Record<string, unknown> }>(
        'GET',
        `/api/v1/session/${sid}/config`,
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
        await http('POST', `/api/v1/session/${sid}/workspace`, { ruta: valor });
        return;
      }
      const parche: Record<string, unknown> = {};
      if (clave === 'nivelRazonamiento') parche['razonamiento'] = valor;
      else if (clave === 'contexto_max_ventana') parche['max_ventana'] = Number(valor);
      else parche[clave] = valor;
      await http('PATCH', `/api/v1/session/${sid}/config`, parche);
    },
    elegirWorkspace: () =>
      Promise.reject(
        new Error('en modo web la ruta se fija con fijarWorkspace (campo de texto)'),
      ),
    fijarWorkspace: async (ruta) => {
      const r = await http<{ workspace: string }>('POST', `/api/v1/session/${sid}/workspace`, {
        ruta,
      });
      const base_info: InfoSesion = ultimoInfo ?? {
        modelo: '/',
        workspace: r.workspace,
        proveedores: [],
        conversacion: { id: '', titulo: '', archivada: false, actualizada_en: '' },
      };
      return recordar({ ...base_info, workspace: r.workspace });
    },
    fijarMeta: () =>
      Promise.reject(new Error('la meta no disponible en modo web (fase 069A-2)')),
  };
}

/** Adaptador web: misma superficie `AdaptadorReal` sobre HTTP/SSE. */
export function crearAdaptadorApi(base: string, hooks: HooksAdaptador = {}): AdaptadorReal {
  return crearAdaptadorReal(hooks, crearTransporteApi(base, hooks));
}
