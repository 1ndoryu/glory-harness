/* Cliente HTTP/SSE del adaptador web: `http<T>`, fuente EventSource,
 * composición de sesión y estado compartido (sid/turno/último info). */
import type {
  AgenteEvento,
  HooksAdaptador,
  InfoSesion,
} from '../tauri/real';

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

export interface ClienteApi {
  http<T>(metodo: string, ruta: string, cuerpo?: unknown): Promise<T>;
  abrirFuente(
    onEvento: (ev: AgenteEvento) => void,
    onFin: (ok: boolean, error?: string) => void,
  ): void;
  recordar(info: InfoSesion, aviso?: string | null): InfoSesion;
  componerSesion(): Promise<InfoSesion>;
  fijarWorkspaceImpl(ruta: string): Promise<InfoSesion>;
  getSid: () => string;
  setSid: (sid: string) => void;
  getUltimoInfo: () => InfoSesion | null;
  getUltimoTurno: () => string | null;
  setUltimoTurno: (tid: string | null) => void;
}

export function crearClienteApi(
  base: string,
  hooks: HooksAdaptador = {},
): ClienteApi {
  const maestro = tokenMaestro();
  let sid = '';
  let ultimoTurno: string | null = null;
  let fuente: EventSource | null = null;
  let ultimoInfo: InfoSesion | null = null;
  let ultimoAviso: string | null = null;

  const avisarConexion = hooks.onConexion;

  async function http<T>(
    metodo: string,
    ruta: string,
    cuerpo?: unknown,
  ): Promise<T> {
    const cabeceras: Record<string, string> = {
      'Content-Type': 'application/json',
    };
    // La sesión opaca autoriza; el maestro SOLO crea sesiones (§5.2/§7).
    if (ruta === '/api/v1/session') {
      // Modo local tokenless: el backend solo lo permite en loopback. Si hay
      // token configurado, sigue siendo obligatorio para crear la sesión.
      if (maestro) cabeceras['Authorization'] = `Bearer ${maestro}`;
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

  function recordar(info: InfoSesion, aviso?: string | null): InfoSesion {
    ultimoInfo = info;
    if (aviso !== undefined) ultimoAviso = aviso;
    return info;
  }

  async function componerSesion(): Promise<InfoSesion> {
    // PATCH config devuelve solo config: se compone la InfoSesion completa.
    const [cfg, provs] = await Promise.all([
      http<{ config: Record<string, unknown> }>(
        'GET',
        `/api/v1/session/${sid}/config`,
      ),
      http<{ proveedores: Array<{ nombre: string; disponible: boolean }> }>(
        'GET',
        `/api/v1/session/${sid}/providers`,
      ),
    ]);
    const c = cfg.config;
    const info: InfoSesion = {
      modelo: `${String(c['provider'] ?? '')}/${String(c['modelo'] ?? '')}`,
      workspace: String(c['workspace'] ?? ultimoInfo?.workspace ?? ''),
      proveedores: provs.proveedores.map((p) => ({
        nombre: p.nombre,
        claves: p.disponible ? 1 : 0,
      })),
      // [069A-7] Se conserva `null` si la sesión no tiene conversación
      // (borrador create-on-write); nunca se fabrica una fila fantasma.
      conversacion: ultimoInfo?.conversacion ?? null,
      aviso: ultimoAviso,
    };
    return recordar(info);
  }

  function abrirFuente(
    onEvento: (ev: AgenteEvento) => void,
    onFin: (ok: boolean, error?: string) => void,
  ): void {
    if (fuente) return;
    avisarConexion?.('conectando');
    const es = new EventSource(`${base}/api/v1/session/${sid}/events`);
    fuente = es;
    es.onopen = () => avisarConexion?.('en-linea');
    es.onerror = () =>
      avisarConexion?.('reconectando', 'el navegador reintenta solo');
    const evento = (tipo: string, fn: (d: unknown) => void) => {
      es.addEventListener(tipo, (e) => {
        try {
          fn(JSON.parse((e as MessageEvent).data) as unknown);
        } catch {
          /* frame corrupto: se ignora sin romper el stream */
        }
      });
    };
    evento('agent.event', (d) => onEvento(d as AgenteEvento));
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
      onEvento({
        tipo: 'error',
        mensaje: f.message ?? f.code ?? 'error',
        retryable: true,
      } as never);
    });
  }

  /** [069A-Proyectos] Extraído para reuso en workspaceActivarsPorRuta. */
  async function fijarWorkspaceImpl(ruta: string): Promise<InfoSesion> {
    const r = await http<{ workspace: string }>(
      'POST',
      `/api/v1/session/${sid}/workspace`,
      { ruta },
    );
    const base_info: InfoSesion = ultimoInfo ?? {
      modelo: '/',
      workspace: r.workspace,
      proveedores: [],
      conversacion: null,
    };
    return recordar({ ...base_info, workspace: r.workspace });
  }

  return {
    http,
    abrirFuente,
    recordar,
    componerSesion,
    fijarWorkspaceImpl,
    getSid: () => sid,
    setSid: (s) => {
      sid = s;
    },
    getUltimoInfo: () => ultimoInfo,
    getUltimoTurno: () => ultimoTurno,
    setUltimoTurno: (tid) => {
      ultimoTurno = tid;
    },
  };
}
