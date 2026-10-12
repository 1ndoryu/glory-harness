import path from 'node:path';

import { correrCargo, fail, prepararEntorno } from './rust/entorno.mjs';
import { acumular, hallazgoGeneral } from './rust/hallazgos.mjs';
import {
  colaUtil,
  diagnosticosDe,
  leerAlcance,
  leerStatsSccache,
  resumirSuites,
} from './rust/parseo.mjs';
import { planificarRapido } from './rust/rapido.mjs';
import { escribirReporte } from './rust/reporte.mjs';

/* Etapa rust del gate: compila y prueba el arbol Rust real, que hasta ahora
   quedaba sin via de validacion (los subcomandos pesados clippy/test estan en
   la lista de comandos directos bloqueados por el guard, y el gate solo
   analizaba).

   Decisiones:
   - El target vive fuera del arbol del proyecto (regla del area): se resuelve
     por CARGO_TARGET_DIR / GLORY_CARGO_TARGET_DIR / CARGO_TARGET_DIR_BASE y el
     respaldo es C:\tmp\glory-target\glory-harness, que es tambien lo que usa
     la purga horaria y lo que abarata sccache.
   - sccache va delante: la etapa `sccache` del gate solo comprueba la
     configuracion; aqui se usa de verdad (el wrapper llega por .cargo/config).
   - Se invoca el cargo real, no el shim del guard: el gate ya es la via
     sancionada y su politica de pesadas (--full/--ci + cooldown) la aplica
     `sentinel check`, no una segunda capa dentro de la etapa.
   - Sin el token/lease del gate la etapa no corre: no puede servir de puerta
     trasera para lanzar cargo pesado a mano.
   - Nunca falla en silencio: clippy, tests y errores de herramienta salen como
     findings del reporte; si clippy falla, los tests no se ejecutan y se
     declara la cobertura no ejecutada en vez de callarlo.
   - Formato de mensajes: `--message-format=json`. El valor
     `json-render-diagnostics` no emite `compiler-message`, con lo que un fallo
     real queda como texto suelto sin archivo ni linea (comprobado con las
     cuatro combinaciones de formato y wrapper de sccache).
   - Modulos: `rust/` (entorno, parseo, rapido, hallazgos, reporte). Este
     fichero solo orquesta; el contrato con el gate (argv, --fast, esquema del
     reporte, codigos de salida) vive en el.

   Modo rapido (179A-1, stages-fast.json): `cargo check --tests` + `cargo
   test --lib` filtrado por los modulos del alcance, sin clippy. Solo
   iteracion: el reporte sale con `mode: fast` y un veredicto explicito de
   "no valido para cierre". Se activa con `--fast` en argv (el manifiesto
   rapido lo pasa como argumento literal, que sobrevive al entorno saneado
   del gate; por env no llegaria). */

const reportPath = process.argv[2];
if (!reportPath) fail('sentinel-rust requiere {reportPath}');

const MODO_FAST = process.argv.slice(2).includes('--fast');

const workspace = process.cwd();
const reportDir = path.dirname(path.resolve(reportPath));
const inicio = Date.now();

if (!process.env.GLORY_QUALITY_GATE_TOKEN && !process.env.GLORY_QUALITY_GATE_LEASE) {
  fail(
    'rust: la etapa solo se ejecuta dentro del gate (falta GLORY_QUALITY_GATE_TOKEN). ' +
      'Usa `sentinel check <TareaId> --stages scripts/quality/stages.json` ' +
      '(o `scripts/quality/stages-fast.json` para el rapido).',
  );
}

const alcance = leerAlcance(reportDir);
const tocaDesktop = alcance.some((archivo) => archivo.startsWith('desktop/src-tauri/'));
const tocaManifiesto = alcance.some((archivo) => /(^|\/)Cargo\.(toml|lock)$/.test(archivo));
const PAQUETES = ['glory-harness-core', 'glory-harness'];
if (tocaDesktop || tocaManifiesto) PAQUETES.push('glory-harness-desktop');
const banderasPaquetes = PAQUETES.flatMap((paquete) => ['-p', paquete]);

/* Fallo cerrado del rapido (179A-1 F3): el motivo sale como error en el reporte
   y el gate rapido queda FAIL, nunca verde por omision (ver rapido.mjs). */
const { motivoNoAplicaFast, filtrosPorPaquete } = MODO_FAST
  ? planificarRapido(alcance, tocaDesktop, tocaManifiesto)
  : { motivoNoAplicaFast: null, filtrosPorPaquete: new Map() };

const { targetDir, volumenTarget, libreAntesGB, minimoLibreGB, entorno } =
  prepararEntorno(workspace);
const correr = (args) => correrCargo(workspace, entorno, args);

/* sccache: la diferencia de contadores antes/despues dice si la cache trabajo. */
const statsSccache = leerStatsSccache(workspace, entorno);

/* [1/2] compilacion: en modo completo, clippy compila todo el alcance
   (incluidos tests y ejemplos) y trata cualquier warning como fallo, igual
   que el resto de la casa. En modo rapido, `check --tests` compila el mismo
   alcance sin lint (medido 179A-1 F1: ~1,5 s en caliente). Si el rapido no
   aplica (motivoNoAplicaFast), no se compila nada: se pide el completo. */
const argsCompilacion = MODO_FAST
  ? ['check', ...banderasPaquetes, '--tests', '--locked', '--message-format=json']
  : [
      'clippy',
      ...banderasPaquetes,
      '--all-targets',
      '--locked',
      '--message-format=json',
      '--',
      '-D',
      'warnings',
    ];
const etiquetaCompilacion = MODO_FAST ? 'cargo check' : 'cargo clippy';
const clippy = MODO_FAST && motivoNoAplicaFast !== null ? null : correr(argsCompilacion);
if (clippy?.error) fail(`rust: no se pudo ejecutar cargo: ${clippy.error.message}`);

let diagnosticos = 0;
for (const { ruta, finding } of diagnosticosDe(clippy?.salida ?? '', workspace)) {
  diagnosticos += 1;
  acumular(ruta, finding);
}

if (clippy !== null && clippy.codigo !== 0 && diagnosticos === 0) {
  acumular(
    workspace,
    hallazgoGeneral(
      'rust-cargo-fallo',
      `${etiquetaCompilacion} termino con codigo ${clippy.codigo} sin diagnostico parseable:\n${colaUtil(clippy.salida).join('\n')}`,
      'error',
    ),
  );
}

/* [2/2] tests: solo si la compilacion paso. Si no, se declara la cobertura no
   ejecutada (warning explicito) en vez de dar por bueno lo que no corrio. */
let testsCorridos = false;
let diagnosticosTests = 0;
const resumenTests = { suites: 0, pasados: 0, fallidos: 0, ignorados: 0 };
if (clippy !== null && clippy.codigo === 0) {
  /* En modo completo, una sola pasada con todos los paquetes. En modo rapido,
     una pasada `--lib` con filtros por paquete (solo paquetes con .rs en el
     alcance; sin filtros no hay nada que ejecutar y check ya valido). */
  const pasadasTests = MODO_FAST
    ? [...filtrosPorPaquete.entries()].map(([paquete, filtros]) => ({
        args: [
          'test',
          '-p',
          paquete,
          '--lib',
          '--locked',
          '--no-fail-fast',
          '--message-format=json',
          '--',
          ...filtros,
        ],
      }))
    : [
        {
          args: ['test', ...banderasPaquetes, '--locked', '--no-fail-fast', '--message-format=json'],
        },
      ];
  for (const pasada of pasadasTests) {
    const tests = correr(pasada.args);
    if (tests.error) fail(`rust: no se pudo ejecutar cargo test: ${tests.error.message}`);
    testsCorridos = true;

    /* El mismo formato deja los errores de compilacion del arbol de tests como
       findings con archivo y linea, en vez de un volcado de texto. */
    for (const { ruta, finding } of diagnosticosDe(tests.salida, workspace)) {
      diagnosticosTests += 1;
      acumular(ruta, finding);
    }

    const { resumenFallos, falloSinResumen } = resumirSuites(tests.salida, resumenTests);
    if (resumenFallos !== null) {
      acumular(workspace, hallazgoGeneral('rust-test-fallido', resumenFallos, 'error'));
    }
    if (tests.codigo !== 0 && falloSinResumen) {
      acumular(
        workspace,
        hallazgoGeneral(
          'rust-tests-fallo',
          `cargo test termino con codigo ${tests.codigo} sin resumen de fallos:\n${colaUtil(tests.salida).join('\n')}`,
          'error',
        ),
      );
    }
  }
  if (MODO_FAST && pasadasTests.length === 0) {
    acumular(
      workspace,
      hallazgoGeneral(
        'rust-tests-filtrados-vacios',
        'Sin tests filtrados: el alcance no toca .rs de core/cli ' +
          '(p. ej. solo front). `cargo check --tests` ya valido la compilacion.',
        'information',
      ),
    );
  }
} else if (clippy !== null) {
  acumular(
    workspace,
    hallazgoGeneral(
      'rust-tests-no-ejecutados',
      `Tests no ejecutados: ${etiquetaCompilacion} fallo primero. La cobertura funcional de esta ` +
        'ejecucion queda pendiente hasta que la compilacion pase.',
      'warning',
    ),
  );
}

/* Fallo cerrado (179A-1 F3): si el rapido no aplica, el reporte sale con un
   error que nombra el motivo y pide el completo. */
if (MODO_FAST && motivoNoAplicaFast !== null) {
  acumular(
    workspace,
    hallazgoGeneral(
      'rust-pide-completo',
      `Modo rapido no aplica (${motivoNoAplicaFast}). ` +
        'Ejecuta el gate completo: sentinel check <TareaId> --stages scripts/quality/stages.json.',
      'error',
    ),
  );
}

/* Veredicto del rapido (179A-1): constancia de que este reporte NO autoriza
   cierre. Va como information para que quede en el reporte aunque todo salga
   verde. */
if (MODO_FAST && motivoNoAplicaFast === null) {
  const filtrosTexto = [...filtrosPorPaquete.entries()]
    .map(([paquete, filtros]) => `${paquete} [${[...filtros].join(', ')}]`)
    .join('; ');
  acumular(
    workspace,
    hallazgoGeneral(
      'rust-modo-rapido',
      'Modo rapido (stages-fast.json): `cargo check --tests` + `cargo test --lib` filtrado' +
        `${filtrosTexto ? ` (${filtrosTexto})` : ' (sin filtros: solo check)'}, sin clippy. ` +
        'NO valido para commit, integrate ni cierre de tarea: ' +
        'el ultimo gate antes de integrar es el completo (stages.json).',
      'information',
    ),
  );
}

/* Nota de alcance: el operador tiene que poder leer que se valido de verdad
   (paquetes, target y cache) sin deducirlo del codigo de la etapa. El conteo de
   invocaciones de sccache distingue "el wrapper esta declarado" de "el wrapper
   se uso de verdad": si es 0 tras una compilacion real, la cache no trabajo. */
const statsDespues = leerStatsSccache(workspace, entorno);
const invocacionesSccache =
  statsSccache !== null && statsDespues !== null
    ? Math.max(0, statsDespues.peticiones - statsSccache.peticiones)
    : null;
acumular(
  workspace,
  hallazgoGeneral(
    'rust-alcance',
    `Paquetes: ${PAQUETES.join(', ')} (shell Tauri ${tocaDesktop || tocaManifiesto ? 'incluido' : 'excluido: el alcance no toca desktop/src-tauri ni los manifiestos'}). ` +
      `Target: ${targetDir}. sccache: ${entorno.RUSTC_WRAPPER}` +
      `${invocacionesSccache === null ? '' : ` (${invocacionesSccache} invocaciones)`}. ` +
      `Espacio libre: ${libreAntesGB === null ? 'sin dato' : `${libreAntesGB.toFixed(2)} GB`}. ` +
      `Tests: ${
        testsCorridos
          ? `ejecutados (${resumenTests.pasados} ok, ${resumenTests.fallidos} fallidos, ${resumenTests.ignorados} ignorados)`
          : 'no ejecutados'
      }.`,
    'information',
  ),
);

const detalle = {
  paquetes: PAQUETES,
  targetDir,
  volumenTarget,
  libreAntesGB,
  minimoLibreGB,
  rustcWrapper: entorno.RUSTC_WRAPPER,
  clippy: { codigo: clippy?.codigo ?? null, diagnosticos },
  tests: { ejecutados: testsCorridos, diagnosticos: diagnosticosTests, ...resumenTests },
  modo: MODO_FAST ? 'fast' : 'full',
  motivoNoAplicaFast,
  filtrosFast: MODO_FAST
    ? [...filtrosPorPaquete.entries()].map(([paquete, filtros]) => ({
        paquete,
        filtros: [...filtros],
      }))
    : undefined,
  sccache: { antes: statsSccache, despues: statsDespues, invocaciones: invocacionesSccache },
  alcanceLeido: alcance.length,
};
const reporte = escribirReporte(reportPath, {
  workspace,
  modoFast: MODO_FAST,
  inicio,
  paquetes: PAQUETES,
  detalle,
});

process.stdout.write(
  `rust${MODO_FAST ? ' (rapido)' : ''}: ${PAQUETES.join(', ')} · ${etiquetaCompilacion} codigo ${clippy?.codigo ?? 'no-ejecutado'} (${diagnosticos} diagnosticos) · ` +
    `tests ${
      testsCorridos
        ? `ejecutados (${resumenTests.pasados} ok, ${resumenTests.fallidos} fallidos)`
        : 'no ejecutados'
    } · ` +
    `sccache ${invocacionesSccache === null ? 'sin datos' : `${invocacionesSccache} invocaciones`} · ` +
    `${reporte.severityCounts.error} errores, ${reporte.severityCounts.warning} avisos.\n`,
);
process.exit(0);
