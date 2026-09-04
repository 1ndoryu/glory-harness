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
} from './componentes/mensajes';
import { montarPanelMeta } from './componentes/panelMeta';

import { crearSimulacion } from './simulacion/simulacion';
import {
  crearAdaptadorReal,
  esEntornoTauri,
  iconoDeTool,
  type AccionRecuperada,
  type InfoSesion,
  type MensajeGuardado,
  type OpcionesTurno,
} from './tauri/real';
import type { EstadoHerramienta, ResultadoHerramienta } from './dominio/tipos';
import { el } from './util/dom';

const raizApp = document.getElementById('app');
if (!raizApp) throw new Error('falta #app');

// ---------- Layout raíz ----------
const app = el('div');
app.id = 'app';
const cuerpo = el('div');
cuerpo.id = 'cuerpo';

const chat = el('section');
chat.id = 'chat';

const cabecera = montarCabeceraChat('Refactor CLI a lib+bin');

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
 */
function pintarHistorial(historial: MensajeGuardado[], acciones: AccionRecuperada[] = []): void {
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
      mensajes.appendChild(crearMensajeUsuario(m.contenido));
      // Acciones del turno de ESTE user (si las hay).
      (porTurno.get(idxUser) ?? []).forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
      idxUser++;
    } else if (m.rol === 'assistant') {
      mensajes.appendChild(crearMensajeAsistente(m.contenido));
    }
  }
  // Residuales al final (turno cancelado sin user persistido, etc.).
  residuales.forEach((a) => mensajes.appendChild(bloqueDesdeAccion(a)));
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

function enviarReal(texto: string): void {
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
    }
    entrada.setCorriendo(false);
  };
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
      // (acciones) intercaladas, no solo user/assistant.
      pintarHistorial(carga.mensajes, carga.acciones);
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
    if (!USA_REAL) return;
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
  onEnviar(texto) {
    enviarReal(texto);
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
    enviarReal(ultimoTextoEnviado);
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
      const [provG, modG, modoG, razG] = await Promise.all([
        adaptador.sesion.configLeer('proveedor'),
        adaptador.sesion.configLeer('modelo'),
        adaptador.sesion.configLeer('modo'),
        adaptador.sesion.configLeer('nivelRazonamiento'),
      ]);
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
        pintarHistorial(carga.mensajes, carga.acciones);
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
