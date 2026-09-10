/* Disparador del menú `/` del compositor (plan 109A-4 F2).
 *
 * Encapsula el pegamento entre el textarea y `menuComandos`, para que
 * `entradaMontaje.ts` solo aporte dos llamadas (`alTecla` y la elección) y no
 * crezca por encima del límite de líneas.
 *
 * Regla de disparo: el menú aparece mientras el texto empieza por `/` y aún
 * no hay espacio ni salto (es decir, se está escribiendo el NOMBRE). Al
 * escribir el primer argumento se cierra, porque ya no hay nada que filtrar.
 */

import { cuerpo } from '../util/dom';
import { alRedimensionar } from '../plataforma/ventana';
import { crearMenuComandos } from './menuComandos';
import {
  COMANDOS_BUILTIN,
  comandoDeProyecto,
  consultaMenu,
  filtrarComandos,
  type ComandoProyecto,
  type ComandoSlash,
} from '../dominio/comandosSlash';

export interface EntradaComandosDeps {
  textarea: HTMLTextAreaElement;
  /** Comandos del área activa (`.glory/comandos`); puede estar vacío. */
  comandosProyecto(): ComandoProyecto[];
  /** El usuario eligió un comando del menú. */
  onElegir(comando: ComandoSlash, admiteArgumentos: boolean): void;
}

export interface EntradaComandos {
  /** `true` si el menú consumió la tecla (el compositor no debe actuar). */
  alTecla(e: KeyboardEvent): boolean;
  /** Cierra el menú (al enviar, al perder foco o al cambiar de panel). */
  cerrar(): void;
}

export function crearEntradaComandos(deps: EntradaComandosDeps): EntradaComandos {
  const { textarea } = deps;

  /** Catálogo vigente: integrados + los del área activa. */
  function catalogo(): ComandoSlash[] {
    return [...COMANDOS_BUILTIN, ...deps.comandosProyecto().map(comandoDeProyecto)];
  }

  const menu = crearMenuComandos({
    ancla: textarea,
    onElegir(comando) {
      deps.onElegir(comando, comando.admiteArgumentos);
    },
  });
  // El menú es `position: fixed`: vive en el body para no verse recortado por
  // el overflow del chat. El compositor solo aporta el ancla geométrica.
  cuerpo().appendChild(menu.raiz);

  /** Repinta a partir del texto actual del textarea. */
  function refrescar(): void {
    const consulta = consultaMenu(textarea.value);
    if (consulta === null) {
      menu.cerrar();
      return;
    }
    const lista = filtrarComandos(catalogo(), consulta);
    // Sin coincidencias NO se muestra un menú vacío: se cierra y el texto
    // sigue siendo editable (el error explícito lo da el envío del comando).
    menu.mostrar(lista);
  }

  textarea.addEventListener('input', refrescar);
  // Al perder el foco (clic fuera, cambio de panel) el menú sobra.
  textarea.addEventListener('blur', () => menu.cerrar());
  // El menú se ancla al rect del textarea: si la ventana cambia de tamaño se
  // cierra en vez de quedar descolgado (se reabre al seguir escribiendo).
  alRedimensionar(() => menu.cerrar());

  return {
    alTecla(e) {
      if (!menu.abierto()) return false;
      return menu.alTecla(e);
    },
    cerrar() {
      menu.cerrar();
    },
  };
}
