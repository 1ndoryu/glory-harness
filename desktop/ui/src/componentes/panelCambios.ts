import '../estilos/cambios.css';
import { el } from '../util/dom';
import { montarPanelGit, type GitTransport, type PanelGit } from './panelGit';
import type { CambioArchivoTurno, RestauracionArchivo } from '../tauri/realTipos';

/** Transporte del panel Cambios: vault por turno + rechazo puntual. */
export interface CambiosTransport {
  listar(conversacionId: string): Promise<CambioArchivoTurno[]>;
  rechazar(conversacionId: string, turnoId: string, ruta: string): Promise<RestauracionArchivo>;
}

export interface PanelCambios {
  raiz: HTMLElement;
  recargar(): void;
  /** [129A-7 F3] Refresco en vivo tras una escritura del agente (el vault ya
   * la tiene: re-consulta con antirrebote y conserva el diff vivo por ruta). */
  registrarCambioVivo(ruta: string, diff: string | null): void;
}

/* [129A-7] Panel "Cambios": la tab antes llamada "Git local". Arriba la
 * sección filesystem (cambios del agente por turno, funciona sin git,
 * agrupados por carpeta y colapsados); debajo el estado git, que se queda
 * como está (sin escrituras). Aceptar = solo marca revisado (localStorage por
 * conversación). Rechazar = restaura el previo del vault, directo. */
export function montarPanelCambios(opts: {
  git: GitTransport;
  cambios: CambiosTransport;
  convId: () => string | null;
  onError?: (texto: string, detalle?: string) => void;
  onToast?: (texto: string, detalle?: string) => void;
}): PanelCambios {
  const raiz = el('div', 'panel-cambios');
  const lista = el('div', 'cambios-lista');
  lista.setAttribute('role', 'list');
  const separador = el('div', 'cambios-separador');
  separador.textContent = 'Git (estado, sin cambios)';
  const git: PanelGit = montarPanelGit({ transporte: opts.git, onError: opts.onError });
  raiz.append(lista, separador, git.raiz);

  let secuencia = 0;
  let debounce: ReturnType<typeof setTimeout> | null = null;
  let diffsVivos = new Map<string, string>();
  let carpetasAbiertas = new Set<string>();
  let diffAbierto: string | null = null;

  function claveRevisados(conv: string): string {
    return `cambios-revisados:${conv}`;
  }

  function leerRevisados(conv: string): Set<string> {
    try {
      const crudo = localStorage.getItem(claveRevisados(conv));
      const arr: unknown = crudo ? JSON.parse(crudo) : [];
      return new Set(Array.isArray(arr) ? arr.filter((x): x is string => typeof x === 'string') : []);
    } catch {
      return new Set();
    }
  }

  function guardarRevisados(conv: string, revisados: Set<string>): void {
    try {
      localStorage.setItem(claveRevisados(conv), JSON.stringify([...revisados]));
    } catch {
      /* sin persistencia: la marca vive en memoria esta sesión */
    }
  }

  function carpetaDe(ruta: string): string {
    const i = Math.max(ruta.lastIndexOf('/'), ruta.lastIndexOf('\\'));
    return i < 0 ? '(raíz)' : ruta.slice(0, i) || '(raíz)';
  }

  function nombreDe(ruta: string): string {
    const i = Math.max(ruta.lastIndexOf('/'), ruta.lastIndexOf('\\'));
    return i < 0 ? ruta : ruta.slice(i + 1);
  }

  function etiquetaTurno(turnoId: string, enMs: number): string {
    const corto = turnoId.slice(0, 8);
    const hora = new Date(enMs).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return `turno ${corto} · ${hora}`;
  }

  function pintar(cambios: CambioArchivoTurno[], conv: string): void {
    lista.replaceChildren();
    if (cambios.length === 0) {
      const vacio = el('div', 'cambios-vacio');
      vacio.textContent = 'el agente aún no tocó archivos en esta conversación';
      lista.appendChild(vacio);
      return;
    }
    const revisados = leerRevisados(conv);
    const porCarpeta = new Map<string, CambioArchivoTurno[]>();
    for (const c of cambios) {
      const carpeta = carpetaDe(c.ruta);
      const g = porCarpeta.get(carpeta);
      if (g) g.push(c);
      else porCarpeta.set(carpeta, [c]);
    }
    for (const [carpeta, archivos] of [...porCarpeta.entries()].sort(([a], [b]) =>
      a.localeCompare(b),
    )) {
      archivos.sort((a, b) => a.en_ms - b.en_ms);
      const grupo = el('details', 'cambios-grupo');
      if (carpetasAbiertas.has(`${conv}|${carpeta}`)) grupo.open = true;
      grupo.addEventListener('toggle', () => {
        const k = `${conv}|${carpeta}`;
        if (grupo.open) carpetasAbiertas.add(k);
        else carpetasAbiertas.delete(k);
      });
      const cab = el('summary', 'cambios-grupo-cabecera');
      const nombre = el('span', 'cambios-grupo-nombre');
      nombre.textContent = carpeta;
      const contador = el('span', 'cambios-grupo-contador');
      contador.textContent = String(archivos.length);
      cab.append(nombre, contador);
      grupo.appendChild(cab);
      for (const c of archivos) {
        grupo.appendChild(filaCambio(c, conv, revisados));
      }
      lista.appendChild(grupo);
    }
  }

  function filaCambio(
    c: CambioArchivoTurno,
    conv: string,
    revisados: Set<string>,
  ): HTMLElement {
    const clave = `${c.turno_id}|${c.ruta}`;
    const fila = el('div', 'cambios-entrada');
    fila.setAttribute('role', 'listitem');
    const principal = el('div', 'cambios-entrada-principal');
    const ruta = el('span', 'cambios-ruta');
    ruta.textContent = nombreDe(c.ruta);
    ruta.title = c.ruta;
    const turno = el('span', 'cambios-turno');
    turno.textContent = etiquetaTurno(c.turno_id, c.en_ms);
    turno.title = `turno ${c.turno_id}`;
    principal.append(ruta, turno);
    const acciones = el('div', 'cambios-acciones');
    const estado = el('span', 'cambios-estado');
    const btnAceptar = el('button', 'cambios-boton') as HTMLButtonElement;
    btnAceptar.type = 'button';
    btnAceptar.textContent = 'Aceptar';
    const btnRechazar = el('button', 'cambios-boton cambios-boton-rechazar') as HTMLButtonElement;
    btnRechazar.type = 'button';
    btnRechazar.textContent = 'Rechazar';
    const diff = diffsVivos.get(c.ruta) ?? null;
    let btnDiff: HTMLButtonElement | null = null;
    if (diff) {
      btnDiff = el('button', 'cambios-boton') as HTMLButtonElement;
      btnDiff.type = 'button';
      btnDiff.textContent = diffAbierto === clave ? 'Ocultar diff' : 'Ver diff';
      btnDiff.addEventListener('click', () => {
        diffAbierto = diffAbierto === clave ? null : clave;
        recargar();
      });
    }
    const refrescarEstado = () => {
      const ok = revisados.has(clave);
      estado.textContent = ok ? 'revisado' : '';
      fila.classList.toggle('revisado', ok);
      btnAceptar.disabled = ok;
    };
    refrescarEstado();
    btnAceptar.addEventListener('click', () => {
      revisados.add(clave);
      guardarRevisados(conv, revisados);
      refrescarEstado();
    });
    btnRechazar.addEventListener('click', () => {
      btnAceptar.disabled = true;
      btnRechazar.disabled = true;
      estado.textContent = 'restaurando…';
      opts.cambios
        .rechazar(conv, c.turno_id, c.ruta)
        .then((r) => {
          if (r.estado === 'restaurado') {
            opts.onToast?.(`rechazado: ${c.ruta}`, 'restaurado al estado previo del turno');
            recargar();
          } else {
            opts.onToast?.(
              `no se tocó ${c.ruta}`,
              r.detalle ?? r.estado,
            );
            recargar();
          }
        })
        .catch((e: unknown) => {
          opts.onToast?.(`no se pudo rechazar ${c.ruta}`, String(e));
          recargar();
        });
    });
    acciones.append(estado, ...(btnDiff ? [btnDiff] : []), btnAceptar, btnRechazar);
    fila.append(principal, acciones);
    if (diff && diffAbierto === clave) {
      const pre = el('pre', 'cambios-diff');
      pre.textContent = diff;
      fila.appendChild(pre);
    }
    return fila;
  }

  async function cargar(): Promise<void> {
    const id = ++secuencia;
    git.recargar();
    const conv = opts.convId();
    if (!conv) {
      if (id !== secuencia) return;
      lista.replaceChildren();
      const vacio = el('div', 'cambios-vacio');
      vacio.textContent = 'abre una conversación para ver sus cambios';
      lista.appendChild(vacio);
      return;
    }
    try {
      const cambios = await opts.cambios.listar(conv);
      if (id !== secuencia) return;
      pintar(cambios, conv);
    } catch (error: unknown) {
      if (id !== secuencia) return;
      lista.replaceChildren();
      opts.onError?.('no se pudieron listar los cambios', String(error));
    }
  }

  function recargar(): void {
    void cargar();
  }

  function registrarCambioVivo(ruta: string, diff: string | null): void {
    if (diff) diffsVivos.set(ruta, diff);
    if (debounce) clearTimeout(debounce);
    debounce = setTimeout(recargar, 800);
  }

  return { raiz, recargar, registrarCambioVivo };
}
