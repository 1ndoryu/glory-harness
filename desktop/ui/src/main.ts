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
import { renderizarBloque } from './componentes/mensajes';
// TEMPORAL (boceto 03-09 §10.5): panel meta — retirar al implementar el real
import { montarPanelMetaBoceto } from './componentes/panelMetaBoceto';

import { crearSimulacion } from './simulacion/simulacion';
import {
  crearAdaptadorReal,
  esEntornoTauri,
  type InfoSesion,
  type MensajeGuardado,
  type OpcionesTurno,
} from './tauri/real';
import { crearAvisoSistema, crearMensajeAsistente, crearMensajeUsuario } from './componentes/mensajes';
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

/** Pinta historial persistido (user/asistente; el resto no se guarda). */
function pintarHistorial(historial: MensajeGuardado[]): void {
  for (const m of historial) {
    if (m.rol === 'user') mensajes.appendChild(crearMensajeUsuario(m.contenido));
    else if (m.rol === 'assistant') mensajes.appendChild(crearMensajeAsistente(m.contenido));
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
  return { proveedor, modelo: modeloActual.modelo, modo: entrada.getModo() };
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

const adaptador = crearAdaptadorReal({ onSesion: sincronizarModeloDesdeSesion });

/** Recarga la lista del backend y la pinta en la sidebar. */
async function resincronizarSidebar(): Promise<void> {
  const lista = await adaptador.sesion.listar();
  conversaciones = lista.map((c) => ({ id: c.id, titulo: c.titulo, archivada: c.archivada }));
  sidebar.sustituir(conversaciones);
}

/** Meta del boceto → backend (el panel real la leerá de su propio campo). */
async function empujarMeta(): Promise<void> {
  const campo = document.getElementById('pm-meta') as HTMLTextAreaElement | null;
  const meta = campo?.value.trim() ? campo.value.trim() : null;
  try {
    await adaptador.sesion.actualizarMeta(meta);
  } catch (e: unknown) {
    avisoChat(`no se pudo fijar la meta: ${String(e)}`, '', 'el turno sigue sin meta');
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
      pintarHistorial(carga.mensajes);
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
  onEnviar(texto) {
    if (entrada.getCorriendo()) return;
    entrada.setCorriendo(true);
    const alTerminar = () => entrada.setCorriendo(false);
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
      entrada.setCorriendo(false);
    }
  },
  onDetener() {
    if (USA_REAL) adaptador.detener();
    else simulacion.detener();
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
    } else if (id === 'nivelRazonamiento') {
      razonamientoActual = String(valor);
      entrada.setRazonamiento(RAZONAMIENTO_ETIQUETA[razonamientoActual] ?? razonamientoActual);
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
// TEMPORAL (boceto 03-09 §10.5): el panel meta va DENTRO de #entrada,
// justo antes de .caja, para que tenga exactamente el mismo ancho que la
// caja de abajo (hereda el max-width/padding de #entrada). Se retira al
// implementar el panel real.
const panelMeta = montarPanelMetaBoceto();
entrada.raiz.insertBefore(panelMeta.raiz, entrada.raiz.firstChild);
chat.appendChild(entrada.raiz);
cuerpo.appendChild(chat);
app.appendChild(cuerpo);
raizApp.appendChild(app);

// El textarea necesita estar en el DOM para medir su altura (scrollHeight
// es 0 fuera de él), así que se ajusta tras el montaje completo del layout.
entrada.medir();
panelMeta.medir();

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
        entrada.setRazonamiento(RAZONAMIENTO_ETIQUETA[razG]);
        modal.asignarValor('nivelRazonamiento', razG);
      }
      await resincronizarSidebar();
      // Reabrir donde se quedó: la más reciente no archivada; si la recién
      // creada está vacía y hay hilo anterior, se vuelve a ese hilo.
      const candidatas = conversaciones.filter((c) => !c.archivada);
      const primera = candidatas[0];
      if (primera) {
        let carga = await adaptador.sesion.cargar(primera.id);
        if (carga.mensajes.length === 0 && candidatas[1]) {
          carga = await adaptador.sesion.cargar(candidatas[1].id);
        }
        conversaActualId = carga.id;
        limpiarChat();
        pintarHistorial(carga.mensajes);
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
