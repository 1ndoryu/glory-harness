/* Ejecutor de comandos `/` del panel (plan 109A-4 F2).
 *
 * Se llama ANTES de montar el turno: un `/comando` nunca cae al LLM por
 * accidente. Devuelve un resultado explícito para que el panel decida:
 *
 * - `consumido`: el comando ya hizo su efecto aquí (avisos, limpiar, modelo).
 * - `prompt`: el comando se convierte en un texto que SÍ se envía al agente
 *   (`/revisar`, `/iniciar` y los comandos markdown del área).
 * - `error`: comando desconocido o no disponible; se avisa y NO se envía.
 * - `no-es-comando`: el texto es un mensaje normal.
 */

import {
  COMANDOS_BUILTIN,
  catalogoTexto,
  comandoDeProyecto,
  partirComando,
  uso,
  type ComandoProyecto,
} from '../dominio/comandosSlash';
import type { EstadoContexto } from './entradaTipos';
import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import type { ResumenCompactacion } from '../tauri/real';

export type ResultadoComando =
  | { tipo: 'no-es-comando' }
  | { tipo: 'consumido' }
  | {
      tipo: 'prompt';
      texto: string;
      /** [109A-4 F4] Este turno corre en política meta (solo lectura). */
      soloLectura?: boolean;
    }
  | { tipo: 'error'; mensaje: string };

export interface ComandosDeps {
  /** Bloque de aviso en el chat: texto principal, meta y detalle. */
  aviso(texto: string, meta: string, detalle: string): void;
  /** Comandos markdown del área activa. */
  comandosProyecto(): ComandoProyecto[];
  /** Expande la plantilla de un comando del área (backend/core). */
  expandirComando(nombre: string, argumentos: string): Promise<string>;
  /** Vacía el panel en borrador local (create-on-write). */
  limpiarPanel(): void;
  /** Último estado de contexto conocido del panel. */
  contexto(): EstadoContexto | null;
  /** Catálogo de modelos de la barra (para `/modelo <id>`). */
  proveedores(): ProveedorModelo[];
  modeloActual(): ModeloSeleccionado;
  /** Cambia el modelo activo (estado compartido M1). */
  cambiarModelo(modelo: ModeloSeleccionado): void;
  /** [109A-4 F3] Compacta el contexto de este panel (backend decide con el
   * historial real; `instruccion` dirige el resumen como argumento). */
  compactar(instruccion: string | null): Promise<ResumenCompactacion>;
  /** [109A-4 F4] ¿El transporte soporta un turno solo-lectura? Si no (modo
   * web), `/meta` se rechaza con motivo explícito en vez de degradar a un
   * turno normal que el usuario leería como de lectura. */
  soportaSoloLectura(): boolean;
}

export interface EjecutorComandos {
  resolver(texto: string): Promise<ResultadoComando>;
}

/** Instrucción de `/revisar` (los argumentos añaden foco). */
const PLANTILLA_REVISAR =
  'Revisa los cambios pendientes de este proyecto: revisa el diff sin commitear, ' +
  'señala errores reales, riesgos y deuda introducida, y propone la corrección mínima. ' +
  'No modifiques archivos sin pedirlo.';

/** Instrucción de `/iniciar` (los argumentos añaden foco). */
const PLANTILLA_INICIAR =
  'Crea o actualiza el AGENTS.md de este proyecto con las convenciones reales del ' +
  'repositorio: comandos de build/test, estructura, reglas de estilo y límites. ' +
  'Inspecciona el código antes de escribir y conserva lo que ya sea correcto.';

/** Instrucción de `/revisar` (los argumentos añaden foco). */
export function crearEjecutorComandos(deps: ComandosDeps): EjecutorComandos {
  /** Integrados + los del área, para `/ayuda` y para saber qué existe. */
  function catalogo() {
    return [...COMANDOS_BUILTIN, ...deps.comandosProyecto().map(comandoDeProyecto)];
  }

  function ayuda(argumentos: string): ResultadoComando {
    if (!argumentos) {
      deps.aviso(
        catalogoTexto(catalogo()),
        'comandos disponibles',
        'escribe «/» en el compositor para filtrarlos',
      );
      return { tipo: 'consumido' };
    }
    const nombre = argumentos.replace(/^\//, '').toLowerCase();
    const comando = catalogo().find((c) => c.nombre === nombre);
    if (!comando) return { tipo: 'error', mensaje: `no existe el comando «/${nombre}»` };
    deps.aviso(comando.detalle, uso(comando), comando.resumen);
    return { tipo: 'consumido' };
  }

  function modelo(argumentos: string): ResultadoComando {
    const actual = deps.modeloActual();
    if (!argumentos) {
      const lineas: string[] = [];
      for (const p of deps.proveedores()) {
        lineas.push(`${p.etiqueta}:`);
        for (const m of p.modelos) lineas.push(`  ${p.id}/${m.modelo} — ${m.nombre}`);
      }
      deps.aviso(lineas.join('\n') || 'no hay modelos configurados', 'modelos disponibles', `actual: ${actual.nombre || '(sin modelo)'}`);
      return { tipo: 'consumido' };
    }
    const buscado = argumentos.toLowerCase();
    for (const p of deps.proveedores()) {
      for (const m of p.modelos) {
        const id = `${p.id}/${m.modelo}`.toLowerCase();
        const corto = m.modelo.toLowerCase();
        if (id === buscado || corto === buscado || m.nombre.toLowerCase() === buscado) {
          deps.cambiarModelo({ proveedor: p.id, modelo: m.modelo, nombre: m.nombre });
          deps.aviso(`modelo activo: ${m.nombre}`, 'modelo', id);
          return { tipo: 'consumido' };
        }
      }
    }
    return { tipo: 'error', mensaje: `no encuentro el modelo «${argumentos}» (prueba /modelo)` };
  }

  function contexto(): ResultadoComando {
    const c = deps.contexto();
    if (!c) {
      deps.aviso('todavía no hay datos de contexto de este panel', 'contexto', 'envía un mensaje primero');
      return { tipo: 'consumido' };
    }
    const pct = c.pct === null ? 'sin dato' : `${Math.round(c.pct)}%`;
    deps.aviso(
      `ocupación: ${pct}\n` +
        `ventana máxima: ${c.maxVentana ?? 'sin dato'}\n` +
        `reserva de salida: ${c.reservaSalida ?? 'sin dato'}\n` +
        `entrada del último turno: ${c.totalEntrada ?? 'sin dato'}`,
      'contexto',
      'la compactación automática usa esta ocupación',
    );
    return { tipo: 'consumido' };
  }

  /** [109A-4 F3] `/compactar [instrucción]`: compactación por demanda. Solo
   * reporta lo que el backend hizo: si no había material, lo dice en vez de
   * mostrar un ahorro que no ocurrió. */
  async function compactarAhora(argumentos: string): Promise<ResultadoComando> {
    try {
      const r = await deps.compactar(argumentos || null);
      if (!r.compactado) {
        deps.aviso(r.motivo ?? 'no se compactó nada', 'compactar', 'el contexto no cambió');
        return { tipo: 'consumido' };
      }
      deps.aviso(
        `contexto compactado: ${r.tokens_antes} → ${r.tokens_despues} tokens (~${Math.round(r.ahorro_pct)}% menos)`,
        'compactar',
        `tramo ${r.tramos} · ocupación ${Math.round(r.ocupacion_pct)}% · el historial visible no cambia`,
      );
      return { tipo: 'consumido' };
    } catch (e: unknown) {
      /* `e.message` y no `String(e)`: los errores de capacidad ausente (web)
       * llevan `name` propio y `String()` lo antepondría al motivo. Un fallo
       * del backend llega como string, así que el `instanceof` cubre ambos. */
      const motivo = e instanceof Error ? e.message : String(e);
      return { tipo: 'error', mensaje: `no se pudo compactar: ${motivo}` };
    }
  }

  /** Comandos que se convierten en un turno normal hacia el agente. */
  function promptDe(nombre: string, argumentos: string): ResultadoComando {
    const plantilla = nombre === 'revisar' ? PLANTILLA_REVISAR : PLANTILLA_INICIAR;
    const texto = argumentos ? `${plantilla}\n\nFoco: ${argumentos}` : plantilla;
    return { tipo: 'prompt', texto };
  }

  /** [109A-4 F4] `/meta <texto>`: UN turno en política meta (solo lectura)
   * usando el texto como meta del turno. NO cambia el modo global: el backend
   * fuerza el modo de ese turno y lo revierte al terminar. */
  function metaDeTurno(argumentos: string): ResultadoComando {
    const texto = argumentos.trim();
    if (!texto) {
      return {
        tipo: 'error',
        mensaje: 'uso: «/meta <texto>» (un turno solo-lectura con esa meta)',
      };
    }
    if (!deps.soportaSoloLectura()) {
      return {
        tipo: 'error',
        mensaje: 'el turno solo-lectura requiere la aplicación de escritorio',
      };
    }
    deps.aviso(
      'turno en modo meta: solo lectura, ninguna tool con efecto se ejecuta',
      'meta',
      'queda activa con su reloj de persecución; el modo global de la sesión no cambia',
    );
    return { tipo: 'prompt', texto, soloLectura: true };
  }

  async function resolver(texto: string): Promise<ResultadoComando> {
    const partes = partirComando(texto);
    if (!partes) return { tipo: 'no-es-comando' };
    const { nombre, argumentos } = partes;

    switch (nombre) {
      case 'ayuda':
        return ayuda(argumentos);
      case 'modelo':
        return modelo(argumentos);
      case 'contexto':
        return contexto();
      case 'limpiar':
        deps.limpiarPanel();
        deps.aviso('conversación vaciada', 'limpiar', 'el historial no se ha borrado');
        return { tipo: 'consumido' };
      case 'revisar':
      case 'iniciar':
        return promptDe(nombre, argumentos);
      case 'compactar':
        return compactarAhora(argumentos);
      case 'meta':
        return metaDeTurno(argumentos);
      default:
        break;
    }

    // Comando markdown del área: la plantilla la expande el core.
    if (deps.comandosProyecto().some((c) => c.nombre.toLowerCase() === nombre)) {
      try {
        const expandido = await deps.expandirComando(nombre, argumentos);
        return { tipo: 'prompt', texto: expandido };
      } catch (e: unknown) {
        return { tipo: 'error', mensaje: `no se pudo expandir «/${nombre}»: ${String(e)}` };
      }
    }

    return { tipo: 'error', mensaje: `comando desconocido «/${nombre}»; prueba /ayuda` };
  }

  return { resolver };
}
