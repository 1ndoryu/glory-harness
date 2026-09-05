// ============================================================
// Punto de entrada del front de glory-harness desktop (plan 039A-1).
// Monta: sidebar + chat (cabecera + mensajes + entrada) + modal.
// La lógica de turno es una simulación local desmontable en
// src/simulacion/ que cuando llegue el backend real (Tauri +
// contrato AgenteEvento) se sustituirá por un adaptador IPC.
// ============================================================

import './estilos/index.css';

import { CONVERSACIONES } from './datos/conversaciones';
import { historialEjemplo } from './datos/historialEjemplo';
import { MODELO_INICIAL, PROVEEDORES } from './dominio/catalogoModelos';
import type { ModeloSeleccionado } from './dominio/tipos';

import { montarSidebar } from './componentes/sidebar';
import { montarCabeceraChat } from './componentes/cabecera';
import { montarEntrada, type ModoEjecucion } from './componentes/entrada';
import { montarModalConfiguracion } from './componentes/modal';
import {
  crearHerramienta,
  renderizarBloque,
  crearAvisoSistema,
  crearMensajeAsistente,
  crearMensajeUsuario,
  crearPieTurno,
} from './componentes/mensajes';
import { abrirMenuContextual, cerrarMenuActual, crearItemMenu, crearSeparadorMenu } from './componentes/menu';
import { montarPanelMeta } from './componentes/panelMeta';

import { crearSimulacion } from './simulacion/simulacion';
import {
  crearAdaptadorReal,
  esEntornoTauri,
  iconoDeTool,
  type AccionRecuperada,
  type CargaConversacion,
  type InfoSesion,
  type MensajeGuardado,
  type OpcionesTurno,
  type UsoTurno,
} from './tauri/real';
import type { EstadoHerramienta, ResultadoHerramienta } from './dominio/tipos';
import { el } from './util/dom';
import { copiarAlPortapapeles } from './util/portapapeles';

const raizApp = document.getElementById('app');
if (!raizApp) throw new Error('falta #app');

// ---------- Layout raíz ----------
const app = el('div');
app.id = 'app';
const cuerpo = el('div');
cuerpo.id = 'cuerpo';

const chat = el('section');
chat.id = 'chat';

// [039A-3 P4] Cabecera con botón de colapsar sidebar + ⋯ de acciones de la
// conversación. El menú ⋯ se construye aquí (abrirAccionesCabecera) con la
// conversación actual; renombrar/archivar/eliminar delegan en la sidebar.
let sidebarAbierta = true;
const cabecera = montarCabeceraChat({
  titulo: 'Refactor CLI a lib+bin',
  onAcciones(rect) {
    abrirAccionesCabecera(rect);
  },
  onToggleSidebar() {
    setSidebarAbierta(!sidebarAbierta);
  },
});

// [039A-3 P4] Grip de redimensionado de la sidebar: se inserta entre la
// sidebar y el chat dentro de #cuerpo. Al arrastrar actualiza la custom
// property --sidebar-ancho (clamp 180-420) y persiste el ancho.
const grip = el('div', 'sidebar-grip');
grip.setAttribute('aria-hidden', 'true');
{
  const MIN = 180;
  const MAX = 420;
  let arrastrando = false;
  function anchoDesdeCursor(clientX: number): number {
    const izquierda = cuerpo.getBoundingClientRect().left;
    return Math.min(MAX, Math.max(MIN, Math.round(clientX - izquierda)));
  }
  grip.addEventListener('mousedown', (e) => {
    e.preventDefault();
    arrastrando = true;
    document.body.classList.add('redimensionando-sidebar');
  });
  window.addEventListener('mousemove', (e) => {
    if (!arrastrando) return;
    const ancho = anchoDesdeCursor(e.clientX);
    cuerpo.style.setProperty('--sidebar-ancho', `${ancho}px`);
  });
  window.addEventListener('mouseup', () => {
    if (!arrastrando) return;
    arrastrando = false;
    document.body.classList.remove('redimensionando-sidebar');
    // Persistir el ancho final (real en config; mock en localStorage).
    const ancho = Math.round(parseFloat(getComputedStyle(cuerpo).getPropertyValue('--sidebar-ancho')) || 260);
    guardarSidebar('sidebar_ancho', String(ancho));
  });
}

const mensajes = el('div');
mensajes.id = 'mensajes';

const simulacion = crearSimulacion();
// Motor real (Tauri in-process) o simulación solo para maquetar en navegador
// con VITE_MOCK=1. Sin Tauri y sin mock no se fingen turnos: se avisa.
const USA_REAL = esEntornoTauri();
const USA_MOCK = !USA_REAL && import.meta.env.VITE_MOCK === '1';

// ---------- Backend real: estado y helpers (solo USA_REAL) ----------
// `conversaActualId` es la conversación que el backend tiene como actual.
let conversaActualId: string | null = null;

function avisoChat(texto: string, meta: string, detalle: string): void {
  mensajes.appendChild(crearAvisoSistema(texto, meta, detalle));
  mensajes.scrollTop = mensajes.scrollHeight;
}

function limpiarChat(): void {
  mensajes.replaceChildren();
  resetUsuariosHistorial();
  // [039A-3 P2] Al cambiar de conversación o repintar se descarta una edición
  // pendiente: su `editandoId` apuntaría a un mensaje que ya no está en el
  // historial visible (el envío fallaría con "mensaje objetivo no
  // encontrado"). `entrada` se monta después; el closure se ejecuta en
  // runtime, cuando `entrada` ya existe.
  entrada?.cancelarEnEdicion();
}

/**
 * [039A-3 P1/P2] Texto del último tramo visible (desde el último mensaje de
 * usuario hasta el último del asistente). Recorre los nodos actuales de
 * #mensajes y conserva solo texto plano; si no hay tramo, devuelve `null`.
 */
function tramoParaCopiar(): string | null {
  const hijos = Array.from(mensajes.children);
  // Último índice de un nodo que cumple el predicado (ES2022: sin
  // findLastIndex, se recorre en orden inverso).
  const ultimoIndice = (pred: (n: Element) => boolean): number => {
    for (let i = hijos.length - 1; i >= 0; i--) {
      if (pred(hijos[i])) return i;
    }
    return -1;
  };
  const ultimoUser = ultimoIndice((n) => n.classList.contains('msg-user'));
  if (ultimoUser < 0) return null;
  // Último assistant o, si aún no hay (turno fallido), el user a secas.
  const ultimoAsis = ultimoIndice((n) => n.classList.contains('msg-asis'));
  const fin = ultimoAsis >= ultimoUser ? ultimoAsis : ultimoUser;
  const texto = hijos
    .slice(ultimoUser, fin + 1)
    .map((n) => {
      if (n.classList.contains('pie-turno')) return '';
      return (n.textContent ?? '').trim();
    })
    .filter((s) => s.length > 0)
    .join('\n\n');
  return texto || null;
}

/** [039A-3 P1] Copia el último tramo (user → assistant) al portapapeles. */
function copiarUltimoTramo(): void {
  const texto = tramoParaCopiar();
  if (!texto) {
    avisoChat('no hay mensajes que copiar', '', '');
    return;
  }
  void copiarAlPortapapeles(texto)
    .then(() => avisoChat('tramo copiado al portapapeles', 'copiar', ''))
    .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
}

// ---------- [039A-3 P2] acciones por mensaje (editar / volver / copiar) ----------
// Los mensajes de usuario del historial cargado se registran aquí (id → texto)
// para que el menú "⋯" de cada uno sepa editar/volver/copiar sin re-parsear
// el DOM. Se resetea en `limpiarChat()` (cambio de conversación).
let usuariosHistorial = new Map<string, string>();

function resetUsuariosHistorial(): void {
  usuariosHistorial = new Map<string, string>();
}

/**
 * Copia al portapapeles un tramo concreto: el mensaje de usuario con `id` y
 * su respuesta (hasta el siguiente user o el fin del historial). Se usa en el
 * menú del mensaje ("copiar"); a diferencia de `copiarUltimoTramo` (que solo
 * cubre el último tramo visible), aquí se reconstruye desde el mapa de ids.
 */
function copiarTramoMensaje(id: string): void {
  const texto = tramoDesdeId(id);
  if (!texto) {
    avisoChat('no hay texto que copiar', '', '');
    return;
  }
  void copiarAlPortapapeles(texto)
    .then(() => avisoChat('mensaje copiado al portapapeles', 'copiar', ''))
    .catch((e: unknown) => avisoChat(`no se pudo copiar: ${String(e)}`, '', ''));
}

/**
 * Reconstruye el texto plano del tramo de un mensaje de usuario concreto a
 * partir de los nodos DOM de #mensajes (user + sus acciones + su respuesta).
 * Devuelve `null` si no hay un mensaje de usuario con ese id visible.
 */
function tramoDesdeId(id: string): string | null {
  const nodo = mensajes.querySelector<HTMLElement>(`.msg-user[data-id="${CSS.escape(id)}"]`);
  if (!nodo) return null;
  const hijos = Array.from(mensajes.children);
  const i = hijos.indexOf(nodo);
  if (i < 0) return null;
  // Texto del propio mensaje + bloques hasta el siguiente msg-user.
  const partes: string[] = [];
  for (let j = i; j < hijos.length; j++) {
    const n = hijos[j];
    if (j > i && n.classList.contains('msg-user')) break;
    if (n.classList.contains('pie-turno')) continue;
    const t = (n.textContent ?? '').trim();
    if (t) partes.push(t);
  }
  return partes.length ? partes.join('\n\n') : null;
}

/**
 * Reconstruye el DOM tras un rewind (volver a un punto o editar) usando la
 * respuesta `CargaConversacion` que devuelve el backend. Se reutiliza la
 * misma vía que al cargar una conversación para no duplicar lógica.
 */
function aplicarCarga(carga: CargaConversacion): void {
  conversaActualId = carga.id;
  limpiarChat();
  pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
  cabecera.ponerTitulo(carga.titulo);
  sidebar.seleccionar(carga.id);
}

/**
 * "Volver a este punto": rewind con `editar=false` (el mensaje objetivo se
 * conserva; se borra solo el hilo posterior) y repinta con lo que devuelve
 * el backend. Si hay turno activo se bloquea (mismo guard que el envío).
 */
async function volverA(id: string): Promise<void> {
  if (entrada.getCorriendo()) {
    avisoChat('termina el turno antes de volver a un punto', '', '');
    return;
  }
  if (!USA_REAL) {
    avisoChat('volver a un punto requiere la app Tauri', '', '');
    return;
  }
  try {
    const carga = await adaptador.sesion.rewind(id, false);
    aplicarCarga(carga);
  } catch (e: unknown) {
    avisoChat(`no se pudo volver a ese punto: ${String(e)}`, '', '');
  }
}

/**
 * Editar un mensaje de usuario: entra en modo edición en el textarea (texto
 * del mensaje + barra "editando…"). El borrado del hilo posterior NO ocurre
 * aquí: se hace al enviar (rewind `editar=true` + reenvío), como pide P2.
 */
function empezarEdicion(id: string): void {
  if (entrada.getCorriendo()) {
    avisoChat('termina el turno antes de editar un mensaje', '', '');
    return;
  }
  const texto = usuariosHistorial.get(id);
  if (texto === undefined) {
    avisoChat('el mensaje ya no está en esta conversación', '', '');
    return;
  }
  entrada.ponerEnEdicion(id, texto);
}

/** Abre el menú contextual del mensaje de usuario (botón "⋯"). */
function abrirAccionesMensaje(id: string, rect: DOMRect): void {
  abrirMenuContextual({
    rect,
    construir(m) {
      m.appendChild(
        crearItemMenu({
          texto: 'Editar',
          onClick() {
            cerrarMenuActual();
            empezarEdicion(id);
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: 'Volver a este punto',
          onClick() {
            cerrarMenuActual();
            void volverA(id);
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: 'Copiar',
          onClick() {
            cerrarMenuActual();
            copiarTramoMensaje(id);
          },
        }),
      );
    },
  });
}

// ---------- [039A-3 P4] sidebar colapsable + redimensionable ----------
// El estado (abierta/colapsada + ancho) vive aquí; la cabecera solo muestra
// el botón toggle. Se persiste: en config del backend cuando hay sesión real
// (`sidebar_ancho`/`sidebar_colapsada`), y en localStorage en el navegador
// (mock) para poder verificarlo.

const CLAVE_ANCHO = 'sidebar_ancho';
const CLAVE_COLAPSADA = 'sidebar_colapsada';

/** Persiste el estado del sidebar (real → config; mock → localStorage). */
function guardarSidebar(clave: string, valor: string): void {
  if (USA_REAL) {
    void adaptador.sesion
      .configGuardar(clave, valor)
      .catch((e: unknown) => avisoChat(`no se pudo guardar ${clave}: ${String(e)}`, '', ''));
  } else {
    try {
      window.localStorage.setItem(clave, valor);
    } catch {
      /* sin persistencia local: no bloquea */
    }
  }
}

/** Lee el estado del sidebar (real → config; mock → localStorage). */
function leerSidebar(clave: string): string | null {
  if (USA_REAL) return null; // se lee async en el arranque real
  try {
    return window.localStorage.getItem(clave);
  } catch {
    return null;
  }
}

/** Aplica el estado visual de la sidebar (clase + custom property). */
function pintarSidebar(): void {
  cuerpo.classList.toggle('sidebar-colapsada', !sidebarAbierta);
  cabecera.setSidebarAbierta(sidebarAbierta);
}

/** Colapsa (false) o expande (true) la sidebar y persiste el estado. */
function setSidebarAbierta(abierta: boolean): void {
  if (sidebarAbierta === abierta) return;
  sidebarAbierta = abierta;
  pintarSidebar();
  guardarSidebar(CLAVE_COLAPSADA, abierta ? '0' : '1');
}

/** [039A-3 P4] Menú ⋯ de la cabecera: acciones de la conversación actual.
 * Reutiliza la misma lógica que el ⋯ de cada fila de la sidebar (que es la
 * dueña de la lista); aquí solo se dispara sobre la conversación activa. */
function abrirAccionesCabecera(rect: DOMRect): void {
  const conv = conversaciones.find((c) => c.id === conversaActualId);
  if (!conv) {
    avisoChat('no hay conversación activa', '', '');
    return;
  }
  const id = conv.id;
  abrirMenuContextual({
    rect,
    construir(m) {
      m.appendChild(
        crearItemMenu({
          texto: 'Cambiar nombre',
          onClick() {
            // Renombrar en la cabecera edita el título inline; al guardar se
            // delega en el mismo onRenombrar de la sidebar (backend real).
            cerrarMenuActual();
            cabecera.empezarRenombrar(
              conv.titulo,
              (nuevo) => {
                conv.titulo = nuevo;
                cabecera.ponerTitulo(nuevo);
                if (USA_REAL) {
                  void adaptador.sesion
                    .renombrar(id, nuevo)
                    .then(async (ok) => {
                      if (!ok) avisoChat('el backend no renombró', '', '');
                      await resincronizarSidebar();
                    })
                    .catch(() => resincronizarSidebar());
                } else {
                  sidebar.sustituir(conversaciones);
                }
              },
              () => undefined,
            );
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: conv.archivada ? 'Desarchivar' : 'Archivar',
          onClick() {
            sidebar.archivarConversacion(id);
          },
        }),
      );
      m.appendChild(
        crearItemMenu({
          texto: 'Copiar ID',
          onClick() {
            cerrarMenuActual();
            void copiarAlPortapapeles(id);
          },
        }),
      );
      m.appendChild(crearSeparadorMenu());
      m.appendChild(
        crearItemMenu({
          texto: 'Eliminar',
          onClick() {
            sidebar.eliminarConversacion(id);
          },
        }),
      );
    },
  });
}

/** Render de una acción recuperada → bloque `.herramienta` estático. */
function bloqueDesdeAccion(accion: AccionRecuperada): HTMLElement {
  const meta = accion.ok ? 'ok' : 'falló';
  // [039A-1 04-09 H6] Si hay diff, se muestra (como en vivo); si no, el
  // resumen. El texto va con saltos de línea conservados (pre-wrap).
  const cuerpo = accion.diff ? `${accion.resumen}\n${accion.diff}` : accion.resumen;
  const resultado: ResultadoHerramienta = { tipo: 'texto', texto: cuerpo };
  const estado: EstadoHerramienta = accion.ok
    ? { estado: 'completada', meta, resultado }
    : { estado: 'error', meta, resultado };
  return crearHerramienta({
    icono: iconoDeTool(accion.tool),
    titulo: accion.tool,
    estado,
  });
}

/**
 * Pinta historial persistido intercalando las herramientas del turno en su
 * posición (user → acciones → assistant). [039A-1 04-09 H6] Cada acción
 * pertenece al turno cuyo `user` la disparó: el backend ancla la acción al
 * `creado_en` del turno (mayor o igual que el `creado_en` del user que la
 * provocó y anterior al siguiente user). Se asigna cada acción al ÚLTIMO user
 * con `creado_en <= turno_en` (tolerante a segundos compartidos).
 *
 * [039A-3 P1] `ultimo_uso` (opcional) repinta el pie de turno del último
 * turno al cargar: los tokens/modelo reales viajan en `turnos`, no en los
 * mensajes, así que el pie es la única traza de ese uso tras recargar.
 */
function pintarHistorial(
  historial: MensajeGuardado[],
  acciones: AccionRecuperada[] = [],
  ultimo_uso?: { provider: string; modelo: string; tokens_prompt: number; tokens_complecion: number } | null,
): void {
  const users = historial.filter((m) => m.rol === 'user');
  const en = (s: string): number => Date.parse(s) || 0;
  // Mapa: índice de user → acciones que le pertenecen (en orden del backend).
  const porTurno = new Map<number, AccionRecuperada[]>();
  const residuales: AccionRecuperada[] = [];
  acciones.forEach((a) => {
    const t = en(a.turno_en);
    // Último user con creado_en <= turno_en (o el primero si todo es posterior).
    let indice = -1;
    for (let i = 0; i < users.length; i++) {
      if (en(users[i].creado_en) <= t) indice = i;
      else break;
    }
    if (indice < 0) {
      // Acción previa al primer user (p. ej. turno sin mensaje persistido).
      residuales.push(a);
      return;
    }
    const lista = porTurno.get(indice) ?? [];
    lista.push(a);
    porTurno.set(indice, lista);
  });

  // Recorre el historial y emite cada user seguido de sus acciones.
  let idxUser = 0;
  for (const m of historial) {
    if (m.rol === 'user') {
      // [039A-3 P2] El user se registra en el mapa id → texto y se pinta con
      // botón "⋯" (editar/volver/copiar). Los mensajes persistidos siempre
      // traen id: el historial viene del backend.
      usuariosHistorial.set(m.id, m.contenido);
      mensajes.appendChild(crearMensajeUsuario(m.contenido, m.id, abrirAccionesMensaje));
      // Acciones del turno de ESTE user (si las hay).
      (porTurno.get(idxUser) ?? []).forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
      idxUser++;
    } else if (m.rol === 'assistant') {
      mensajes.appendChild(crearMensajeAsistente(m.contenido));
    }
  }
  // Residuales al final (turno cancelado sin user persistido, etc.).
  residuales.forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
  // [039A-3 P1] Pie del último turno si el backend registró uso real.
  if (ultimo_uso) {
    mensajes.appendChild(
      crearPieTurno({
        tokensPrompt: ultimo_uso.tokens_prompt,
        tokensComplecion: ultimo_uso.tokens_complecion,
        modelo: ultimo_uso.provider
          ? `${ultimo_uso.provider}/${ultimo_uso.modelo}`
          : ultimo_uso.modelo || null,
        // Al recargar no viaja la ocupación de contexto (solo tokens/uso):
        // el pie muestra tokens/modelo; el % se ignora (se recalcula en vivo).
        ocupacionPct: null,
        maxVentana: null,
        reservaSalida: null,
        alCopiar: copiarUltimoTramo,
      }),
    );
  }
  mensajes.scrollTop = mensajes.scrollHeight;
}

/** Opciones del turno con el modelo vigente (+ deriva del allowlist). */
function opcionesTurno(): OpcionesTurno {
  let proveedor = modeloActual.proveedor;
  // 039A-1 F4: `meta/*` y `stealth/*` solo existen en el allowlist de
  // `glory`; el catálogo estático aún los agrupa bajo commandcode.
  if (proveedor === 'commandcode' && /^(meta|stealth)\//.test(modeloActual.modelo)) {
    proveedor = 'glory';
  }
  return {
    proveedor,
    modelo: modeloActual.modelo,
    modo: entrada.getModo(),
    // [039A-1 04-09 H7] El nivel de razonamiento de la barra/modal viaja al
    // turno real (el backend lo aplica al config del runtime).
    razonamiento: entrada.getRazonamiento(),
  };
}

/**
 * Sincroniza los selectores con lo que el backend resolvió (p. ej. defaults
 * cuando se pidió `null`). `info.modelo` viene como `proveedor/modelo`.
 */
function sincronizarModeloDesdeSesion(info: InfoSesion): void {
  const corte = info.modelo.indexOf('/');
  if (corte < 0) return;
  const proveedor = info.modelo.slice(0, corte);
  const modelo = info.modelo.slice(corte + 1);
  if (proveedor === modeloActual.proveedor && modelo === modeloActual.modelo) return;
  const nombre =
    proveedor === modeloActual.proveedor && modelo === modeloActual.modelo
      ? modeloActual.nombre
      : modelo;
  modeloActual = { proveedor, modelo, nombre };
  entrada.setModelo(modeloActual);
  modal.setModelo(modeloActual);
}

/**
 * [039A-1 04-09 H1] Visibilidad del panel meta: se muestra solo cuando hay
 * meta definida o el modo es `meta` (permite escribirla). Con el panel oculto
 * no queda hueco colgante en la entrada (el CSS anula el margin).
 */
function sincronizarPanelMeta(): void {
  const hayMeta = panelMeta.getMeta().trim().length > 0;
  const visible = hayMeta || modoActual === 'meta';
  panelMeta.mostrar(visible);
}

const adaptador = crearAdaptadorReal({
  onSesion(info) {
    sincronizarModeloDesdeSesion(info);
    // [039A-1 04-09 H3] La ruta real del workspace que el backend resolvió
    // (no el valor por defecto del esquema) se refleja en el modal Contexto.
    const ws = info.workspace;
    if (ws && ws !== '<desconocido>') modal.asignarValor('workspace', ws);
  },
});

/** Recarga la lista del backend y la pinta en la sidebar. */
async function resincronizarSidebar(): Promise<void> {
  const lista = await adaptador.sesion.listar();
  conversaciones = lista.map((c) => ({ id: c.id, titulo: c.titulo, archivada: c.archivada }));
  sidebar.sustituir(conversaciones);
}

/** Meta del panel → backend (el panel real la lee de su propio campo). */
async function empujarMeta(): Promise<void> {
  const meta = panelMeta.getMeta().trim() ? panelMeta.getMeta().trim() : null;
  try {
    await adaptador.sesion.actualizarMeta(meta);
  } catch (e: unknown) {
    avisoChat(`no se pudo fijar la meta: ${String(e)}`, '', 'el turno sigue sin meta');
  }
}

// Último mensaje enviado (para reanudar) e inicio del turno (para el reloj).
let ultimoTextoEnviado = '';
let inicioTurno: number | null = null;

async function enviarReal(texto: string, editandoId?: string | null): Promise<void> {
  if (entrada.getCorriendo()) return;
  entrada.setCorriendo(true);
  ultimoTextoEnviado = texto;
  inicioTurno = Date.now();
  if (USA_REAL) panelMeta.setEstado('corriendo');
  const alTerminar = () => {
    inicioTurno = null;
    if (USA_REAL) {
      panelMeta.setEstado('inactivo');
      const u = adaptador.usoUltimoTurno();
      panelMeta.setTokens(u.tokensPrompt + u.tokensComplecion);
      // [039A-3 P1] Pie de turno: cierra la respuesta con tokens/modelo/uso
      // y botón Copiar. Solo cuando el turno terminó ok (si fue cancelado o
      // error, el pie no aporta tokens de fin y ya hay aviso en el chat).
      if (adaptador.resultadoUltimoTurno() === 'ok') {
        anadirPieTurno(u);
      }
      // [039A-1 04-09 H5] El backend auto-nombra la conversación tras el
      // primer mensaje; al terminar el turno se refresca la lista y el
      // título de la cabecera para reflejarlo sin recargar.
      void (async () => {
        await resincronizarSidebar();
        if (conversaActualId) {
          try {
            const lista2 = await adaptador.sesion.listar();
            const actual = lista2.find((c) => c.id === conversaActualId);
            if (actual) cabecera.ponerTitulo(actual.titulo);
          } catch {
            /* el refresco del título es cosmético: no bloquea */
          }
        }
      })();
    } else if (USA_MOCK) {
      // [039A-3 P1] Mock: simular el pie con valores fijos del turno de demo.
      anadirPieTurno({
        tokensPrompt: 1240,
        tokensComplecion: 385,
        ocupacionPct: 7,
        maxVentana: 150000,
        reservaSalida: 20000,
        modelo: 'glory/gpt-4.1',
        totalEntrada: 9100,
      });
    }
    entrada.setCorriendo(false);
  };
  // [039A-3 P2] Edición: al reenviar un mensaje reescrito, primero se borra
  // el hilo posterior (rewind `editar=true`) y se repinta con la respuesta
  // del backend; después se monta el turno nuevo con el texto corregido.
  if (USA_REAL && editandoId) {
    try {
      const carga = await adaptador.sesion.rewind(editandoId, true);
      aplicarCarga(carga);
    } catch (e: unknown) {
      avisoChat(`no se pudo editar el mensaje: ${String(e)}`, '', '');
      entrada.setCorriendo(false);
      inicioTurno = null;
      if (USA_REAL) panelMeta.setEstado('inactivo');
      return;
    }
  }
  if (USA_REAL) {
    void (async () => {
      // En modo meta la meta editable viaja al backend antes del turno.
      if (entrada.getModo() === 'meta') await empujarMeta();
      await adaptador.montar(mensajes, texto, opcionesTurno(), alTerminar);
    })();
  } else if (USA_MOCK) {
    simulacion.montar(mensajes, texto, entrada.getModo() === 'autonomo', alTerminar);
  } else {
    mensajes.appendChild(
      crearAvisoSistema('Abre esta UI desde la app Tauri', 'sin backend', 'en navegador solo maqueta con VITE_MOCK=1'),
    );
    inicioTurno = null;
    entrada.setCorriendo(false);
  }
}

// ---------- Estado de la vista (fuente única: barra y modal comparten) ----------
let modeloActual: ModeloSeleccionado = MODELO_INICIAL;
let modoActual: ModoEjecucion = 'predeterminado';
// etiquetas visibles del razonamiento
const RAZONAMIENTO_ETIQUETA: Record<string, string> = {
  low: 'Bajo',
  medium: 'Medio',
  high: 'Alto',
};

/** [039A-3 P1] Añade el pie de turno como último bloque visible del chat. */
function anadirPieTurno(u: UsoTurno): void {
  mensajes.appendChild(
    crearPieTurno({
      tokensPrompt: u.tokensPrompt,
      tokensComplecion: u.tokensComplecion,
      modelo: u.modelo,
      ocupacionPct: u.ocupacionPct,
      maxVentana: u.maxVentana,
      reservaSalida: u.reservaSalida,
      alCopiar: copiarUltimoTramo,
    }),
  );
  mensajes.scrollTop = mensajes.scrollHeight;
}
let razonamientoActual: string = 'medium';

// Estado local de conversaciones para el sidebar (mock: datos 1:1; real: el
// backend es la fuente y esto solo es caché para el título de cabecera).
let conversaciones = USA_REAL ? [] : CONVERSACIONES.map((c) => ({ ...c }));

async function nuevaConversacionReal(): Promise<void> {
  if (entrada.getCorriendo()) {
    avisoChat('termina el turno antes de abrir otra conversación', '', '');
    return;
  }
  try {
    const conv = await adaptador.sesion.nueva();
    conversaActualId = conv.id;
    await resincronizarSidebar();
    limpiarChat();
    cabecera.ponerTitulo(conv.titulo);
    sidebar.seleccionar(conv.id);
  } catch (e: unknown) {
    avisoChat(`no se pudo crear la conversación: ${String(e)}`, '', '');
  }
}

const sidebar = montarSidebar({
  conversaciones,
  async onSeleccionar(id) {
    // [039A-3 P4] En mock también se mantiene la conversación activa para que
    // el ⋯ de la cabecera y las acciones actúen sobre la conversación actual.
    conversaActualId = id;
    if (!USA_REAL) {
      const conv = conversaciones.find((c) => c.id === id);
      if (conv) cabecera.ponerTitulo(conv.titulo);
      return;
    }
    if (entrada.getCorriendo()) {
      avisoChat('termina el turno antes de cambiar de conversación', '', '');
      return;
    }
    try {
      const carga = await adaptador.sesion.cargar(id);
      conversaActualId = carga.id;
      limpiarChat();
      // [039A-1 04-09 H6] Al cargar se repintan también las herramientas
      // (acciones) intercaladas, no solo user/assistant. [039A-3 P1] Y el
      // pie de turno del último turno si hay uso registrado.
      pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
      cabecera.ponerTitulo(carga.titulo);
      sidebar.seleccionar(carga.id);
    } catch (e: unknown) {
      avisoChat(`no se pudo cargar la conversación: ${String(e)}`, '', '');
    }
  },
  async onRenombrar(id, titulo) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) conv.titulo = titulo;
    if (!USA_REAL) return;
    try {
      const ok = await adaptador.sesion.renombrar(id, titulo);
      if (!ok) {
        avisoChat('el backend no renombró (id ajeno o inexistente)', '', '');
        await resincronizarSidebar();
      }
    } catch (e: unknown) {
      avisoChat(`no se pudo renombrar: ${String(e)}`, '', '');
      await resincronizarSidebar();
    }
  },
  async onArchivar(id, archivada) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) conv.archivada = archivada;
    if (!USA_REAL) return;
    try {
      const ok = await adaptador.sesion.archivar(id, archivada);
      if (!ok) {
        avisoChat('el backend no archivó (id ajeno o inexistente)', '', '');
        await resincronizarSidebar();
      }
    } catch (e: unknown) {
      avisoChat(`no se pudo archivar: ${String(e)}`, '', '');
      await resincronizarSidebar();
    }
  },
  async onEliminar(id) {
    conversaciones = conversaciones.filter((c) => c.id !== id);
    if (!USA_REAL) {
      // [039A-3 P4] En mock también se actualiza la sidebar y, si se eliminó la
      // conversación activa, se pasa a la primera restante (o se queda sin
      // activa). El chat decorativo del mock no se limpia: es un historial de
      // ejemplo fijo (al navegar el mock no repinta por conversación).
      sidebar.sustituir(conversaciones);
      if (id === conversaActualId) {
        const resto = conversaciones.find((c) => !c.archivada);
        if (resto) {
          conversaActualId = resto.id;
          cabecera.ponerTitulo(resto.titulo);
          sidebar.seleccionar(resto.id);
        } else {
          // Sin conversaciones restantes: el chat queda en blanco sin activa.
          conversaActualId = null;
          cabecera.ponerTitulo('Sin conversación');
        }
      }
      return;
    }
    try {
      // Si era la actual, el backend crea una nueva y la devuelve.
      const actual = await adaptador.sesion.eliminar(id);
      await resincronizarSidebar();
      if (id === conversaActualId) {
        conversaActualId = actual.id;
        limpiarChat();
        cabecera.ponerTitulo(actual.titulo);
        sidebar.seleccionar(actual.id);
      }
    } catch (e: unknown) {
      avisoChat(`no se pudo eliminar: ${String(e)}`, '', '');
      await resincronizarSidebar();
    }
  },
  // Agentes / Flujo / Complementos no existen aún en el producto: avisan
  // "próximamente" en el chat (ver plan 039A-1 §10, sin backend en v1).
  // "Nueva conversación" sí es real (con backend o con estado local).
  onAccionNav(accion) {
    if (accion === 'nueva') {
      if (USA_REAL) void nuevaConversacionReal();
      else {
        const n = conversaciones.length + 1;
        const nueva = { id: `local-${Date.now()}`, titulo: `Conversación ${n}` };
        conversaciones = [nueva, ...conversaciones];
        conversaActualId = nueva.id;
        sidebar.sustituir(conversaciones);
        limpiarChat();
        cabecera.ponerTitulo(nueva.titulo);
        sidebar.seleccionar(nueva.id);
      }
      return;
    }
    const nombre =
      accion === 'agente'
        ? 'Agentes'
        : accion === 'flujo'
          ? 'Flujo'
          : accion === 'complementos'
            ? 'Complementos'
            : accion;
    mensajes.appendChild(
      crearAvisoSistema(`${nombre}: próximamente`, 'sin backend', `${nombre} no está disponible en esta versión.`),
    );
    mensajes.scrollTop = mensajes.scrollHeight;
  },
  abrirConfig: () => modal.abrir(),
});

const entrada = montarEntrada({
  proveedores: PROVEEDORES,
  modeloActual,
  modo: modoActual,
  razonamiento: razonamientoActual,
  onEnviar(texto, editandoId) {
    // [039A-3 P2] `editandoId` viene cuando el textarea estaba en modo edición
    // de un mensaje: se hace rewind(editar=true) + reenvío con el texto nuevo.
    void enviarReal(texto, editandoId);
  },
  onDetener() {
    if (USA_REAL) {
      adaptador.detener();
      panelMeta.setEstado('pausado');
    } else simulacion.detener();
    entrada.setCorriendo(false);
  },
  onModeloCambiado(nuevo) {
    modeloActual = nuevo;
    modal.setModelo(nuevo);
    if (USA_REAL) {
      // Config persistida (039A-1 F4): el próximo arranque la respeta.
      void adaptador.sesion
        .configGuardar('proveedor', nuevo.proveedor)
        .then(() => adaptador.sesion.configGuardar('modelo', nuevo.modelo))
        .catch((e: unknown) => avisoChat(`no se pudo guardar el modelo: ${String(e)}`, '', ''));
    }
  },
  onModoCambiado(nuevo) {
    modoActual = nuevo;
    modal.asignarValor('modo', nuevo);
    if (USA_REAL) {
      void adaptador.sesion
        .configGuardar('modo', nuevo)
        .catch((e: unknown) => avisoChat(`no se pudo guardar el modo: ${String(e)}`, '', ''));
    }
    // [039A-1 04-09 H1] Cambiar a modo meta muestra el panel (para escribirla);
    // salir de meta con meta vacía lo oculta.
    sincronizarPanelMeta();
  },
  onRazonamientoCambiado(nuevo) {
    // [039A-1 04-09 H7] El nivel elegido en la barra es la fuente: se propaga
    // al modal (segmentado) y se persiste para el próximo arranque. El turno
    // lo toma vía `opcionesTurno().razonamiento` al enviar.
    razonamientoActual = nuevo;
    modal.asignarValor('nivelRazonamiento', nuevo);
    if (USA_REAL) {
      void adaptador.sesion
        .configGuardar('nivelRazonamiento', nuevo)
        .catch((e: unknown) =>
          avisoChat(`no se pudo guardar el razonamiento: ${String(e)}`, '', ''),
        );
    }
  },
});

// El modal se construye después de `entrada` para poder referenciarlo en los
// callbacks de la entrada (onModeloCambiado/onModoCambiado). El estado real
// se presiembra y los dos selectores (barra y modal) comparten modelo.
const modal = montarModalConfiguracion({
  modelo: modeloActual,
  proveedores: PROVEEDORES,
  modo: modoActual,
  razonamiento: razonamientoActual,
  onCambio(id, valor) {
    if (id === 'modo') {
      modoActual = valor as ModoEjecucion;
      entrada.setModo(modoActual);
      // [039A-1 04-09 H1] El panel meta depende del modo (visible en `meta`).
      sincronizarPanelMeta();
    } else if (id === 'nivelRazonamiento') {
      razonamientoActual = String(valor);
      // [039A-1 04-09 H7] El nivel elegido en el modal se refleja en la barra.
      entrada.setRazonamientoValor(razonamientoActual);
    }
    if (USA_REAL && (id === 'modo' || id === 'nivelRazonamiento')) {
      // Guardado automático → config persistida (el resto de opciones del
      // modal sigue siendo local hasta que tenga comando backend).
      void adaptador.sesion
        .configGuardar(id, String(valor))
        .catch((e: unknown) => avisoChat(`no se pudo guardar ${id}: ${String(e)}`, '', ''));
    }
  },
  onModeloCambiado(nuevo) {
    modeloActual = nuevo;
    entrada.setModelo(nuevo);
    if (USA_REAL) {
      void adaptador.sesion
        .configGuardar('proveedor', nuevo.proveedor)
        .then(() => adaptador.sesion.configGuardar('modelo', nuevo.modelo))
        .catch((e: unknown) => avisoChat(`no se pudo guardar el modelo: ${String(e)}`, '', ''));
    }
  },
});

// ---------- Montaje del DOM ----------
cuerpo.appendChild(sidebar.raiz);
// [039A-3 P4] El grip de redimensionado va entre la sidebar y el chat.
cuerpo.appendChild(grip);
chat.appendChild(cabecera.raiz);
chat.appendChild(mensajes);
// El panel meta va DENTRO de #entrada, justo antes de .caja, para que
// tenga exactamente el mismo ancho que la caja de abajo (hereda el
// max-width/padding de #entrada).
const panelMeta = montarPanelMeta({
  onMetaCambiada(meta) {
    // [039A-1 04-09 H1] Escribir meta la muestra; borrarla (fuera de modo
    // meta) la oculta al confirmar edición.
    sincronizarPanelMeta();
    if (!USA_REAL) return;
    const valor = meta.trim() ? meta.trim() : null;
    void adaptador.sesion
      .actualizarMeta(valor)
      .catch((e: unknown) => avisoChat(`no se pudo fijar la meta: ${String(e)}`, '', ''));
  },
  onPausar() {
    if (entrada.getCorriendo()) {
      if (USA_REAL) adaptador.detener();
      else simulacion.detener();
      entrada.setCorriendo(false);
      panelMeta.setEstado('pausado');
    }
  },
  onReanudar() {
    if (entrada.getCorriendo()) return;
    if (!ultimoTextoEnviado) {
      avisoChat('nada que reanudar: envía un mensaje primero', '', '');
      return;
    }
    void enviarReal(ultimoTextoEnviado);
  },
});
entrada.raiz.insertBefore(panelMeta.raiz, entrada.raiz.firstChild);
chat.appendChild(entrada.raiz);
cuerpo.appendChild(chat);
app.appendChild(cuerpo);
raizApp.appendChild(app);

// El textarea necesita estar en el DOM para medir su altura (scrollHeight
// es 0 fuera de él), así que se ajusta tras el montaje completo del layout.
entrada.medir();
panelMeta.medir();
// [039A-1 04-09 H1] Estado inicial del panel: oculto salvo que el modo sea
// meta o haya meta persistida (se verá al reabrir con modo meta guardado).
sincronizarPanelMeta();

// [039A-3 P4] Estado inicial del sidebar en el navegador (mock): se restaura
// el ancho y el colapsado guardados en localStorage. En Tauri (USA_REAL) se
// lee de config en el arranque real (más abajo, bloque USA_REAL).
{
  const ancho = leerSidebar(CLAVE_ANCHO);
  if (ancho) {
    const n = Number(ancho);
    if (Number.isFinite(n)) cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
  }
  const col = leerSidebar(CLAVE_COLAPSADA);
  sidebarAbierta = col !== '1';
  pintarSidebar();
  // [039A-3 P4] En mock la conversación mostrada en el chat es la primera no
  // archivada: así el ⋯ de la cabecera y las acciones actúan sobre la
  // conversación correcta desde el primer render.
  if (conversaActualId === null) {
    const candidata = conversaciones.find((c) => !c.archivada);
    if (candidata) conversaActualId = candidata.id;
  }
}

// Reloj del turno + tokens reales del núcleo (sin simulación): mientras hay
// turno en curso se actualizan cada segundo; al cerrar queda el total.
window.setInterval(() => {
  if (!USA_REAL || inicioTurno === null) return;
  panelMeta.setTiempo((Date.now() - inicioTurno) / 1000);
  const u = adaptador.usoUltimoTurno();
  panelMeta.setTokens(u.tokensPrompt + u.tokensComplecion);
}, 1000);

// ---------- Historial inicial ----------
// En modo real no se finge historial: la conversación empieza vacía contra el
// núcleo. El ejemplo 1:1 del mockup solo se muestra maquetando (VITE_MOCK=1).
if (USA_MOCK) {
  historialEjemplo().forEach((bloque) => {
    mensajes.appendChild(renderizarBloque(bloque));
  });
} else if (USA_REAL) {
  mensajes.appendChild(
    crearAvisoSistema('Sesión real del núcleo (sin simulación)', 'tauri in-process', 'escribe y envía'),
  );
}

// Añade el modal fuera de #app (estilo port 1:1: el mockup lo tenía
// fuera del #cuerpo pero dentro de body; lo dejamos como hermano del layout).
document.body.appendChild(modal.raiz);

// ---------- Arranque real: sesión + lista + última conversación ----------
if (USA_REAL) {
  void (async () => {
    try {
      await adaptador.asegurarSesion(opcionesTurno());
      // Config persistida (F4): el modelo/modo/razonamiento guardados mandan
      // sobre los iniciales del mockup.
      const [provG, modG, modoG, razG, anchoG, colG] = await Promise.all([
        adaptador.sesion.configLeer('proveedor'),
        adaptador.sesion.configLeer('modelo'),
        adaptador.sesion.configLeer('modo'),
        adaptador.sesion.configLeer('nivelRazonamiento'),
        adaptador.sesion.configLeer(CLAVE_ANCHO),
        adaptador.sesion.configLeer(CLAVE_COLAPSADA),
      ]);
      // [039A-3 P4] Restaurar ancho/colapsado del sidebar desde la config.
      if (anchoG) {
        const n = Number(anchoG);
        if (Number.isFinite(n)) {
          cuerpo.style.setProperty('--sidebar-ancho', `${Math.round(n)}px`);
        }
      }
      if (colG) {
        sidebarAbierta = colG !== '1';
        pintarSidebar();
      }
      if (modG) {
        modeloActual = { proveedor: provG ?? modeloActual.proveedor, modelo: modG, nombre: modG };
        entrada.setModelo(modeloActual);
        modal.setModelo(modeloActual);
      }
      if (modoG === 'predeterminado' || modoG === 'meta' || modoG === 'autonomo') {
        modoActual = modoG;
        entrada.setModo(modoActual);
        modal.asignarValor('modo', modoActual);
      }
      if (razG && RAZONAMIENTO_ETIQUETA[razG]) {
        razonamientoActual = razG;
        entrada.setRazonamientoValor(razG);
        modal.asignarValor('nivelRazonamiento', razG);
      }
      // [039A-1 04-09 H1] Tras restaurar el modo guardado, el panel meta se
      // muestra solo si corresponde (modo meta o meta persistida).
      sincronizarPanelMeta();
      await resincronizarSidebar();
      // Reabrir donde se quedó: la más reciente no archivada. El backend ya
      // reutilizó esa conversación en `abrir_sesion` (H4); no se crea una
      // vacía nueva si hay hilo anterior, así que no hay que saltar a una
      // segunda candidata.
      const candidatas = conversaciones.filter((c) => !c.archivada);
      const primera = candidatas[0];
      if (primera) {
        const carga = await adaptador.sesion.cargar(primera.id);
        conversaActualId = carga.id;
        limpiarChat();
        pintarHistorial(carga.mensajes, carga.acciones, carga.ultimo_uso);
        cabecera.ponerTitulo(carga.titulo);
        sidebar.seleccionar(carga.id);
      }
    } catch (e: unknown) {
      avisoChat(
        `el backend no arrancó: ${String(e)}`,
        'tauri',
        'puedes escribir igual (reintenta al enviar)',
      );
    }
  })();
}
