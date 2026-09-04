// ============================================================
// Simulación local desmontable. Cuando llegue el backend real
// (Tauri + contrato AgenteEvento vía IPC), esta capa se sustituye
// por un adaptador que consume eventos; la UI de bloques no cambia.
//
// Conserva 1:1 el turno de ejemplo del mockup: el modelo razona
// (streaming), emite dos mensajes, ejecuta file_search (ok) y
// propone file_patch → tarjeta de aprobación (o ejecución directa
// en modo autónomo). Enter dispara un turno; el botón detiene.
// ============================================================

import type { ResultadoHerramienta } from '../dominio/tipos';
import {
  crearMensajeAsistenteVivo,
  crearMensajeUsuario,
  crearRazonamientoVivo,
  crearHerramientaViva,
  crearAvisoSistema,
  crearTarjetaAprobacion,
} from '../componentes/mensajes';
import { scrollAlFinal } from '../util/dom';

/** Temporizadores vivos (para poder cancelar a mitad de turno). */
interface Simulacion {
  montar: (msgs: HTMLElement, texto: string, auto: boolean, onFin: () => void) => void;
  detener: () => void;
}

export function crearSimulacion(): Simulacion {
  let timers: number[] = [];
  let msgs: HTMLElement | null = null;
  let corriendo = false;
  let onFin: () => void = () => undefined;

  function later(fn: () => void, ms: number): void {
    timers.push(window.setTimeout(fn, ms));
  }

  function limpiarTimers(): void {
    timers.forEach((t) => window.clearTimeout(t));
    timers = [];
  }

  function anadir(nodo: HTMLElement): void {
    if (msgs) {
      msgs.appendChild(nodo);
      scrollAlFinal(msgs);
    }
  }

  /** Escribe texto en un nodo en tiempo real sin cursor. */
  function streamEn(nodo: HTMLElement, texto: string, paso: number, cb: () => void): void {
    let i = 0;
    (function pasoStream() {
      if (!corriendo) return;
      if (i >= texto.length) {
        nodo.textContent = texto;
        cb();
        return;
      }
      nodo.textContent = texto.slice(0, ++i);
      if (msgs) scrollAlFinal(msgs);
      later(pasoStream, paso);
    })();
  }

  /** Stream con cursor (mensaje de asistente). */
  function stream(m: { nodo: HTMLElement; cursor: HTMLElement }, texto: string, paso: number, cb: () => void): void {
    let i = 0;
    (function pasoStream() {
      if (!corriendo) return;
      if (i >= texto.length) {
        cb();
        return;
      }
      m.nodo.textContent = texto.slice(0, ++i);
      m.nodo.appendChild(m.cursor);
      if (msgs) scrollAlFinal(msgs);
      later(pasoStream, paso);
    })();
  }

  // Turno de ejemplo; `auto` = modo autónomo (sin aprobación).
  function turnoEjemplo(texto: string, auto: boolean): void {
    if (!msgs) return;
    corriendo = true;

    anadir(crearMensajeUsuario(texto));

    // 1) el modelo razona: bloque "Razonando" abierto, texto fluyendo
    const raz = crearRazonamientoVivo();
    anadir(raz.raiz);
    streamEn(
      raz.nodoResultado,
      'El pedido pide extraer construir_harness y procesar_turno a una lib compartida sin cambiar comportamiento. Reviso run.rs/chat.rs para ver qué depende de la lógica de sesión y confirmo que la TUI y el daemon puedan consumirla desde src/lib.rs sin duplicar.',
      12,
      () => {
        if (!corriendo) return;
        raz.terminar('1.4 s');
        cuerpoTurno();
      },
    );

    function cuerpoTurno(): void {
      const m1 = crearMensajeAsistenteVivo('Perfecto. Reviso los módulos y preparo el cambio.');
      anadir(m1.raiz);
      stream(
        m1,
        'Perfecto. Reviso la estructura del crate cli: main.rs es despachador, la lógica vive en run.rs/chat.rs. Localizo los símbolos a exponer y verifico que la TUI y el daemon los reutilicen sin duplicar.',
        14,
        () => {
          if (!corriendo) return;

          // 2) tool 1: file_search ejecutándose
          const h1 = crearHerramientaViva('lupa', 'Se buscó "construir_harness" en el crate cli');
          anadir(h1.raiz);
          h1.ejecutando();
          later(() => {
            if (!corriendo) return;
            h1.completada('ok · 12 ms', { tipo: 'texto', texto: '3 coincidencias:\nrun.rs:91 · chat.rs:38 · tui.rs:52' });

            // 3) tool 2: file_patch → aprobación o ejecución directa
            const titulo = 'Se propone modificar cli/src/run.rs';
            if (auto) {
              const h2 = crearHerramientaViva('lapiz', titulo);
              anadir(h2.raiz);
              h2.ejecutando();
              later(() => {
                if (!corriendo) return;
                h2.completada('ok · 4 ms', diffAuto());
                cierre();
              }, 900);
            } else {
              const tarjeta = crearTarjetaAprobacion({
                titulo,
                argsTexto:
                  '{ "ruta": "cli/src/run.rs", "buscar": "pub(crate) fn construir_harness", "reemplazar": "pub fn construir_harness" }',
                onDecidir(decision, tarjeta) {
                  if (!corriendo) return;
                  if (decision !== 'denegar') {
                    tarjeta.ponerEstado(
                      decision === 'permitir' ? 'aprobada · se recordará' : 'aprobada · ejecutando…',
                    );
                    tarjeta.quitarAcciones();
                    later(() => {
                      if (!corriendo) return;
                      tarjeta.ponerEstado('ok · 4 ms');
                      tarjeta.ponerArgsHtml(diffTarjetaHtml());
                      cierre();
                    }, 900);
                  } else {
                    tarjeta.ponerEstado('denegada por el usuario');
                    tarjeta.quitarAcciones();
                    tarjeta.marcarDenegada();
                    cierreDenegado();
                  }
                },
              });
              anadir(tarjeta.raiz);
            }
          }, 900);
        },
      );
    }

    function cierre(): void {
      const m2 = crearMensajeAsistenteVivo(
        'Listo. La lib queda en cli/src/lib.rs y main.rs despacha; el CLI y la desktop usan el mismo código. Sin cambios de comportamiento: tests 49/49.',
      );
      anadir(m2.raiz);
      stream(
        m2,
        'Listo. La lib queda en cli/src/lib.rs y main.rs despacha; el CLI y la desktop usan el mismo código. Sin cambios de comportamiento: tests 49/49.',
        12,
        () => finTurno('turno_done'),
      );
    }

    function cierreDenegado(): void {
      const m2 = crearMensajeAsistenteVivo(
        'Entendido, no aplico el cambio. Puedes revisar el fragmento en la solicitud y volver a pedírmelo cuando quieras.',
      );
      anadir(m2.raiz);
      stream(
        m2,
        'Entendido, no aplico el cambio. Puedes revisar el fragmento en la solicitud y volver a pedírmelo cuando quieras.',
        12,
        () => finTurno('permiso_denegado'),
      );
    }

    function finTurno(motivo: string): void {
      if (!corriendo || !msgs) return;
      corriendo = false;
      limpiarTimers();
      onFin();
      anadir(
        crearAvisoSistema(
          'Sistema · cierre de turno (evento AgenteEvento)',
          motivo,
          'evento emitido por el runtime al terminar el turno · ' +
            motivo +
            ' · la secuencia de eventos sigue el contrato AgenteEvento',
        ),
      );
    }
  }

  function detener(): void {
    if (!corriendo || !msgs) return;
    corriendo = false;
    limpiarTimers();
    onFin();

    const m = crearMensajeAsistenteVivo('');
    m.nodo.removeChild(m.cursor);
    m.nodo.textContent = '[turno cancelado por el usuario]';
    anadir(m.raiz);
    anadir(
      crearAvisoSistema(
        'Sistema · turno cancelado por el usuario',
        'cancelado',
        'receiver dropeado → el runtime cortó el stream y las tools (tx.is_closed)',
      ),
    );
  }

  return {
    montar(contenedor: HTMLElement, texto: string, auto: boolean, alFin: () => void) {
      msgs = contenedor;
      onFin = alFin;
      turnoEjemplo(texto, auto);
    },
    detener,
  };
}

/** Resultado file_patch (html) para modo autónomo (igual que el mockup). */
function diffAuto(): ResultadoHerramienta {
  return {
    tipo: 'html',
    html:
      'cli/src/run.rs · 1 reemplazo (buscar único)\n\n' +
      '<span class="del">pub(crate) fn construir_harness</span>\n' +
      '<span class="add">pub fn construir_harness</span>',
  };
}

/** HTML de diff con .del/.add (mismo markup que el mockup en la tarjeta). */
function diffTarjetaHtml(): string {
  return (
    'cli/src/run.rs · 1 reemplazo (buscar único)<br><br>' +
    '<span class="del">pub(crate) fn construir_harness</span><br>' +
    '<span class="add">pub fn construir_harness</span>'
  );
}
