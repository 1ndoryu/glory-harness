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

import { crearSimulacion } from './simulacion/simulacion';
import { crearAdaptadorReal, esEntornoTauri } from './tauri/real';
import { crearAvisoSistema } from './componentes/mensajes';
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
const adaptador = crearAdaptadorReal();

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

// Estado local de conversaciones para el sidebar (port 1:1 sin backend:
// main conserva los títulos/archivado que la sidebar notifica por callbacks).
let conversaciones = CONVERSACIONES.map((c) => ({ ...c }));

const sidebar = montarSidebar({
  conversaciones,
  onSeleccionar(id) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) cabecera.ponerTitulo(conv.titulo);
  },
  onRenombrar(id, titulo) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) conv.titulo = titulo;
    // TODO(backend): persistir vía IPC/Tauri.
  },
  onArchivar(id, archivada) {
    const conv = conversaciones.find((c) => c.id === id);
    if (conv) conv.archivada = archivada;
    // TODO(backend): persistir vía IPC/Tauri.
  },
  onEliminar(id) {
    conversaciones = conversaciones.filter((c) => c.id !== id);
    // TODO(backend): persistir vía IPC/Tauri.
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
      void adaptador.montar(
        mensajes,
        texto,
        { proveedor: modeloActual.proveedor, modelo: modeloActual.modelo, modo: entrada.getModo() },
        alTerminar,
      );
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
  },
  onModoCambiado(nuevo) {
    modoActual = nuevo;
    modal.asignarValor('modo', nuevo);
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
    // TODO(backend): persistir el resto de opciones vía IPC/Tauri.
  },
  onModeloCambiado(nuevo) {
    modeloActual = nuevo;
    entrada.setModelo(nuevo);
  },
});

// ---------- Montaje del DOM ----------
cuerpo.appendChild(sidebar.raiz);
chat.appendChild(cabecera.raiz);
chat.appendChild(mensajes);
chat.appendChild(entrada.raiz);
cuerpo.appendChild(chat);
app.appendChild(cuerpo);
raizApp.appendChild(app);

// El textarea necesita estar en el DOM para medir su altura (scrollHeight
// es 0 fuera de él), así que se ajusta tras el montaje completo del layout.
entrada.medir();

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
