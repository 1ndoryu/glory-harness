import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

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
     cuatro combinaciones de formato y wrapper de sccache). */

const reportPath = process.argv[2];
if (!reportPath) {
  process.stderr.write('sentinel-rust requiere {reportPath}\n');
  process.exit(2);
}

/* Modo rapido (179A-1, stages-fast.json): `cargo check --tests` + `cargo
   test --lib` filtrado por los modulos del alcance, sin clippy. Solo
   iteracion: el reporte sale con `mode: fast` y un veredicto explicito de
   "no valido para cierre". Se activa con `--fast` en argv (el manifiesto
   rapido lo pasa como argumento literal, que sobrevive al entorno saneado
   del gate; por env no llegaria). */
const MODO_FAST = process.argv.slice(2).includes('--fast');

const workspace = process.cwd();
const reportDir = path.dirname(path.resolve(reportPath));
const inicio = Date.now();

const fail = (mensaje) => {
  process.stderr.write(mensaje + '\n');
  process.exit(2);
};

if (!process.env.GLORY_QUALITY_GATE_TOKEN && !process.env.GLORY_QUALITY_GATE_LEASE) {
  fail(
    'rust: la etapa solo se ejecuta dentro del gate (falta GLORY_QUALITY_GATE_TOKEN). ' +
      'Usa `sentinel check <TareaId> --stages scripts/quality/stages.json` ' +
      '(o `scripts/quality/stages-fast.json` para el rapido).',
  );
}

/* El gate deja el alcance junto al reporte (changed-files.txt y
   scope-manifest.json). Sirve para no compilar el shell Tauri en cada tarea:
   su arbol (tauri/wry/webview2) es caro y solo se paga cuando se toca. */
const leerAlcance = () => {
  const manifiesto = path.join(reportDir, 'scope-manifest.json');
  try {
    const datos = JSON.parse(fs.readFileSync(manifiesto, 'utf8'));
    if (Array.isArray(datos?.files)) return datos.files.map(String);
  } catch {
    /* Sin manifiesto: se intenta con la lista plana. */
  }
  try {
    return fs
      .readFileSync(path.join(reportDir, 'changed-files.txt'), 'utf8')
      .split('\n')
      .map((linea) => linea.trim())
      .filter(Boolean);
  } catch {
    return [];
  }
};

const alcance = leerAlcance();
const tocaDesktop = alcance.some((archivo) => archivo.startsWith('desktop/src-tauri/'));
const tocaManifiesto = alcance.some((archivo) => /(^|\/)Cargo\.(toml|lock)$/.test(archivo));
const PAQUETES = ['glory-harness-core', 'glory-harness'];
if (tocaDesktop || tocaManifiesto) PAQUETES.push('glory-harness-desktop');

const banderasPaquetes = PAQUETES.flatMap((paquete) => ['-p', paquete]);

/* Fallo cerrado del modo rapido (179A-1 F3): el rapido solo vale para un
   cambio sencillo. Todo lo demas pide el completo con un error explicito
   (el gate rapido sale FAIL con el motivo, nunca verde por omision). */
const FAST_MAX_ARCHIVOS = 10;
const FAST_MAX_FILTROS = 5;
const esContrato = (archivo) =>
  archivo.startsWith('core/src/contrato/') || archivo.startsWith('cli/src/contrato/');
let motivoNoAplicaFast = null;
if (MODO_FAST) {
  if (alcance.length === 0) {
    motivoNoAplicaFast = 'sin alcance declarado (ni scope-manifest.json ni changed-files.txt)';
  } else if (tocaDesktop || tocaManifiesto) {
    motivoNoAplicaFast = 'el alcance toca desktop/src-tauri o manifiestos Cargo';
  } else if (alcance.some(esContrato)) {
    motivoNoAplicaFast = 'el alcance toca el contrato publico (core/src/contrato/ o cli/src/contrato/)';
  } else if (alcance.length > FAST_MAX_ARCHIVOS) {
    motivoNoAplicaFast = `el alcance tiene ${alcance.length} archivos (maximo ${FAST_MAX_ARCHIVOS} para el rapido)`;
  }
}

/* Heuristica alcance -> filtros de libtest (179A-1 F2): el nombre del modulo
   (directorio padre si el fichero es mod/lib/main.rs, si no el stem) como
   subcadena del filtro de cargo test. Medido en F1: `turno` corre 8 tests y
   filtra 328 en ~1,3 s en caliente. Solo .rs bajo core/src o cli/src; el
   resto (p. ej. .ts del front) no aporta filtros. */
const filtroPara = (archivo) => {
  const coincidencia = /^(?:core|cli)\/src\/(.+)\.rs$/.exec(archivo);
  if (!coincidencia) return null;
  const partes = coincidencia[1].split('/');
  let base = partes[partes.length - 1];
  if ((base === 'mod' || base === 'lib' || base === 'main') && partes.length > 1) {
    base = partes[partes.length - 2];
  }
  if (!/^[a-z0-9_]+$/.test(base)) return null;
  return base;
};
const paquetePara = (archivo) =>
  archivo.startsWith('core/src/') ? 'glory-harness-core' : archivo.startsWith('cli/src/') ? 'glory-harness' : null;
const filtrosPorPaquete = new Map();
if (MODO_FAST && motivoNoAplicaFast === null) {
  for (const archivo of alcance) {
    const filtro = filtroPara(archivo);
    const paquete = paquetePara(archivo);
    if (!filtro || !paquete) continue;
    if (!filtrosPorPaquete.has(paquete)) filtrosPorPaquete.set(paquete, new Set());
    filtrosPorPaquete.get(paquete).add(filtro);
  }
  const totalFiltros = [...filtrosPorPaquete.values()].reduce((n, set) => n + set.size, 0);
  if (totalFiltros > FAST_MAX_FILTROS) {
    motivoNoAplicaFast = `el alcance deriva ${totalFiltros} filtros de test (maximo ${FAST_MAX_FILTROS} para el rapido)`;
    filtrosPorPaquete.clear();
  } else if (totalFiltros === 0) {
    motivoNoAplicaFast =
      'el alcance no incluye archivos `.rs` bajo core/src o cli/src (sin filtros de test)';
  }
}

const baseTarget = process.env.CARGO_TARGET_DIR_BASE || 'C:\\tmp\\glory-target';
const targetDir =
  process.env.CARGO_TARGET_DIR ||
  process.env.GLORY_CARGO_TARGET_DIR ||
  path.join(baseTarget, 'glory-harness');
if (path.resolve(targetDir).startsWith(path.resolve(workspace))) {
  fail(`rust: CARGO_TARGET_DIR (${targetDir}) queda dentro del arbol del proyecto; usa C:\\tmp.`);
}

/* Preflight de espacio (11-09). Un volumen lleno no se anuncia como tal: cargo
   devuelve `rustc-LLVM ERROR: IO failure on output stream: no space on device`,
   un `STATUS_STACK_BUFFER_OVERRUN` y un `could not compile` de un crate ajeno
   (webview2-com-sys), que se leen como fallo de codigo. La unica pista fiable es
   `os error 112`, y para entonces ya se pago el build entero. Medido con C: a
   0 GB libres. Se comprueba antes de invocar cargo y se sale como error de
   entorno (codigo 2), no como hallazgo del repo: no es deuda de codigo.
   El shell Tauri suma ~2,3 GB al corte CLI/core, de ahi el umbral. */
const MINIMO_LIBRE_GB = Number(process.env.GLORY_MIN_FREE_GB ?? 8);
const volumenTarget = path.parse(path.resolve(targetDir)).root;
const espacioLibreGB = (ruta) => {
  try {
    const stats = fs.statfsSync(ruta);
    return (Number(stats.bavail) * Number(stats.bsize)) / 1024 ** 3;
  } catch {
    /* statfs no disponible: se omite el dato en vez de inventarlo o bloquear. */
    return null;
  }
};
const libreAntesGB = espacioLibreGB(volumenTarget);
if (libreAntesGB !== null && libreAntesGB < MINIMO_LIBRE_GB) {
  fail(
    `rust: quedan ${libreAntesGB.toFixed(2)} GB libres en ${volumenTarget} ` +
      `(minimo ${MINIMO_LIBRE_GB} GB para el target + shell). Un build con el disco lleno ` +
      'falla con errores que parecen de codigo (`os error 112`, ' +
      '`IO failure on output stream`). Libera espacio (borra targets sin usar de ' +
      `${path.resolve(baseTarget)} o pasa GLORY_MIN_FREE_GB) y repite. ` +
      'Detalle: Agente/prevencion/prevencion-disco-lleno-build-2026-09-11.md.',
  );
}

const entorno = {
  ...process.env,
  CARGO_TARGET_DIR: targetDir,
  RUSTC_WRAPPER: process.env.RUSTC_WRAPPER || 'sccache',
  SCCACHE_CACHE_SIZE: process.env.SCCACHE_CACHE_SIZE || '5G',
  CARGO_TERM_COLOR: 'never',
  CARGO_TERM_PROGRESS_WHEN: 'never',
};

const correr = (args) => {
  const resultado = spawnSync('cargo', args, {
    cwd: workspace,
    encoding: 'utf8',
    windowsHide: true,
    env: entorno,
    maxBuffer: 64 * 1024 * 1024,
  });
  return {
    codigo: resultado.status ?? -1,
    salida: `${resultado.stdout ?? ''}${resultado.stderr ?? ''}`,
    error: resultado.error,
  };
};

const MAX_HALLAZGOS = 60;
const porArchivo = new Map();
const cola = [];

/* sccache no expone un flag "se uso"; se mide por diferencia de contadores. Si
   el binario no responde se devuelve null y la etapa solo omite el dato: la
   politica de sccache la decide su propia etapa, aqui se informa. */
const leerStatsSccache = () => {
  const resultado = spawnSync('sccache', ['--show-stats', '--stats-format=json'], {
    cwd: workspace,
    encoding: 'utf8',
    windowsHide: true,
    env: entorno,
    timeout: 30000,
  });
  if (resultado.status !== 0 || !resultado.stdout) return null;
  try {
    const estadisticas = (JSON.parse(resultado.stdout).stats ?? {}) ;
    return {
      peticiones: Number(estadisticas.compile_requests ?? 0),
      ejecutadas: Number(estadisticas.requests_executed ?? 0),
      fallidas: Number(estadisticas.compile_fails ?? 0),
      noCacheables: Number(estadisticas.non_cacheable_compilations ?? 0),
      errores: Number(estadisticas.cache_errors ?? 0),
    };
  } catch {
    return null;
  }
};

/* Resume las lineas `test result:` de la salida de cargo test en el
   acumulador compartido. Devuelve el resumen de fallos (si hubo suites FAILED)
   y si el proceso fallo sin resumen parseable. */
const resumirSuites = (salida, resumenTests) => {
  const lineas = salida.split('\n');
  let enFallos = false;
  let nombresFallos = [];
  let resumenFallos = null;
  for (const linea of lineas) {
    const limpia = linea.replace(/\r$/, '');
    if (/^failures:$/.test(limpia.trim())) {
      enFallos = true;
      nombresFallos = [];
      continue;
    }
    if (enFallos && /^\s{4}\S/.test(limpia)) {
      nombresFallos.push(limpia.trim());
      continue;
    }
    const suite = /^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored/.exec(limpia);
    if (suite) {
      resumenTests.suites += 1;
      resumenTests.pasados += Number(suite[2]);
      resumenTests.fallidos += Number(suite[3]);
      resumenTests.ignorados += Number(suite[4]);
      const nombres = nombresFallos.slice(0, 20);
      enFallos = false;
      nombresFallos = [];
      if (suite[1] === 'FAILED') {
        resumenFallos = `${suite[2]} ok, ${suite[3]} fallidos, ${suite[4]} ignorados. Casos: ${
          nombres.length > 0 ? nombres.join(', ') : '(ver el log de la etapa)'
        }`;
      }
    }
  }
  const falloSinResumen = !lineas.some((l) => /^test result: FAILED\./.test(l.replace(/\r$/, '')));
  return { resumenFallos, falloSinResumen };
};

const statsSccache = leerStatsSccache();
const acumular = (ruta, finding) => {
  if (cola.length >= MAX_HALLAZGOS) return false;
  cola.push(ruta);
  if (!porArchivo.has(ruta)) porArchivo.set(ruta, []);
  porArchivo.get(ruta).push(finding);
  return true;
};

const rango0 = (span) => ({
  start: {
    line: Math.max(0, Number(span?.line_start ?? 1) - 1),
    character: Math.max(0, Number(span?.column_start ?? 1) - 1),
  },
  end: {
    line: Math.max(0, Number(span?.line_end ?? span?.line_start ?? 1) - 1),
    character: Math.max(0, Number(span?.column_end ?? 1) - 1),
  },
});

/* Los diagnosticos de rustc llegan como lineas JSON en la salida de cargo. Ojo
   con el formato: `--message-format=json-render-diagnostics` NO emite
   `compiler-message` (comprobado con las cuatro combinaciones de formato y
   wrapper de sccache en este repo). Sin JSON no hay archivo ni linea, solo un
   texto suelto, asi que aqui se usa `json`. */
const diagnosticosDe = (salida) => {
  const encontrados = [];
  for (const linea of salida.split('\n')) {
    if (!linea.startsWith('{')) continue;
    let mensaje;
    try {
      mensaje = JSON.parse(linea);
    } catch {
      continue;
    }
    if (mensaje?.reason !== 'compiler-message' || !mensaje.message) continue;
    const nivel = mensaje.message.level;
    if (nivel !== 'error' && nivel !== 'warning') continue;
    const span = mensaje.message.spans?.find((s) => s.is_primary) ?? mensaje.message.spans?.[0];
    encontrados.push({
      ruta: span?.file_name ? path.resolve(workspace, span.file_name) : workspace,
      finding: {
        ruleId: mensaje.message.code?.code ?? (nivel === 'error' ? 'rust-error' : 'rust-warning'),
        message: `${mensaje.message.message}${
          mensaje.message.code?.code ? ` (${mensaje.message.code.code})` : ''
        }`,
        severity: nivel === 'error' ? 'error' : 'warning',
        range: rango0(span),
        source: 'rust',
        confidence: 1,
      },
    });
  }
  return encontrados;
};

/* Cuando cargo falla sin diagnostico parseable queda texto suelto: se recorta a
   las ultimas lineas utiles, sin las lineas JSON, para que el hallazgo se lea. */
const colaUtil = (salida) =>
  salida
    .split('\n')
    .map((linea) => linea.replace(/\r$/, ''))
    .filter((linea) => linea.trim() && !linea.startsWith('{'))
    .slice(-12);

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
for (const { ruta, finding } of diagnosticosDe(clippy?.salida ?? '')) {
  diagnosticos += 1;
  acumular(ruta, finding);
}

if (clippy !== null && clippy.codigo !== 0 && diagnosticos === 0) {
  const colaSalida = colaUtil(clippy.salida);
  acumular(workspace, {
    ruleId: 'rust-cargo-fallo',
    message: `${etiquetaCompilacion} termino con codigo ${clippy.codigo} sin diagnostico parseable:\n${colaSalida.join('\n')}`,
    severity: 'error',
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    source: 'rust',
    confidence: 1,
  });
}

/* [2/2] tests: solo si la compilacion paso. Si no, se declara la cobertura no
   ejecutada (warning explicito) en vez de dar por bueno lo que no corrio. */
let testsCorridos = false;
let diagnosticosTests = 0;
let resumenTests = { suites: 0, pasados: 0, fallidos: 0, ignorados: 0 };
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
          args: [
            'test',
            ...banderasPaquetes,
            '--locked',
            '--no-fail-fast',
            '--message-format=json',
          ],
        },
      ];
  for (const pasada of pasadasTests) {
    const tests = correr(pasada.args);
    if (tests.error) fail(`rust: no se pudo ejecutar cargo test: ${tests.error.message}`);
    testsCorridos = true;

    /* El mismo formato deja los errores de compilacion del arbol de tests como
       findings con archivo y linea, en vez de un volcado de texto. */
    for (const { ruta, finding } of diagnosticosDe(tests.salida)) {
      diagnosticosTests += 1;
      acumular(ruta, finding);
    }

    const { resumenFallos, falloSinResumen } = resumirSuites(tests.salida, resumenTests);
    if (resumenFallos !== null) {
      acumular(workspace, {
        ruleId: 'rust-test-fallido',
        message: resumenFallos,
        severity: 'error',
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        source: 'rust',
        confidence: 1,
      });
    }
    if (tests.codigo !== 0 && falloSinResumen) {
      const colaSalida = colaUtil(tests.salida);
      acumular(workspace, {
        ruleId: 'rust-tests-fallo',
        message: `cargo test termino con codigo ${tests.codigo} sin resumen de fallos:\n${colaSalida.join('\n')}`,
        severity: 'error',
        range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
        source: 'rust',
        confidence: 1,
      });
    }
  }
  if (MODO_FAST && pasadasTests.length === 0) {
    acumular(workspace, {
      ruleId: 'rust-tests-filtrados-vacios',
      message:
        'Sin tests filtrados: el alcance no toca .rs de core/cli ' +
        '(p. ej. solo front). `cargo check --tests` ya valido la compilacion.',
      severity: 'information',
      range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
      source: 'rust',
      confidence: 1,
    });
  }
} else if (clippy !== null) {
  acumular(workspace, {
    ruleId: 'rust-tests-no-ejecutados',
    message:
      `Tests no ejecutados: ${etiquetaCompilacion} fallo primero. La cobertura funcional de esta ` +
      'ejecucion queda pendiente hasta que la compilacion pase.',
    severity: 'warning',
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    source: 'rust',
    confidence: 1,
  });
}

/* Fallo cerrado (179A-1 F3): si el rapido no aplica, el reporte sale con un
   error que nombra el motivo y pide el completo. El gate rapido queda FAIL;
   nunca verde por omision. */
if (MODO_FAST && motivoNoAplicaFast !== null) {
  acumular(workspace, {
    ruleId: 'rust-pide-completo',
    message:
      `Modo rapido no aplica (${motivoNoAplicaFast}). ` +
      'Ejecuta el gate completo: sentinel check <TareaId> --stages scripts/quality/stages.json.',
    severity: 'error',
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    source: 'rust',
    confidence: 1,
  });
}

/* Veredicto del rapido (179A-1): constancia de que este reporte NO autoriza
   cierre. Va como information para que quede en el reporte aunque todo salga
   verde. */
if (MODO_FAST && motivoNoAplicaFast === null) {
  const filtrosTexto = [...filtrosPorPaquete.entries()]
    .map(([paquete, filtros]) => `${paquete} [${[...filtros].join(', ')}]`)
    .join('; ');
  acumular(workspace, {
    ruleId: 'rust-modo-rapido',
    message:
      'Modo rapido (stages-fast.json): `cargo check --tests` + `cargo test --lib` filtrado' +
      `${filtrosTexto ? ` (${filtrosTexto})` : ' (sin filtros: solo check)'}, sin clippy. ` +
      'NO valido para commit, integrate ni cierre de tarea: ' +
      'el ultimo gate antes de integrar es el completo (stages.json).',
    severity: 'information',
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    source: 'rust',
    confidence: 1,
  });
}

/* Nota de alcance: el operador tiene que poder leer que se valido de verdad
   (paquetes, target y cache) sin deducirlo del codigo de la etapa. El conteo de
   invocaciones de sccache distingue "el wrapper esta declarado" de "el wrapper
   se uso de verdad": si es 0 tras una compilacion real, la cache no trabajo. */
const statsDespues = leerStatsSccache();
const invocacionesSccache =
  statsSccache !== null && statsDespues !== null
    ? Math.max(0, statsDespues.peticiones - statsSccache.peticiones)
    : null;
acumular(workspace, {
  ruleId: 'rust-alcance',
  message:
    `Paquetes: ${PAQUETES.join(', ')} (shell Tauri ${tocaDesktop || tocaManifiesto ? 'incluido' : 'excluido: el alcance no toca desktop/src-tauri ni los manifiestos'}). ` +
    `Target: ${targetDir}. sccache: ${entorno.RUSTC_WRAPPER}` +
    `${invocacionesSccache === null ? '' : ` (${invocacionesSccache} invocaciones)`}. ` +
    `Espacio libre: ${libreAntesGB === null ? 'sin dato' : `${libreAntesGB.toFixed(2)} GB`}. ` +
    `Tests: ${
      testsCorridos
        ? `ejecutados (${resumenTests.pasados} ok, ${resumenTests.fallidos} fallidos, ${resumenTests.ignorados} ignorados)`
        : 'no ejecutados'
    }.`,
  severity: 'information',
  range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
  source: 'rust',
  confidence: 1,
});

if (cola.length >= MAX_HALLAZGOS) {
  porArchivo.get(workspace).push({
    ruleId: 'rust-hallazgos-truncados',
    message: `Se listan ${MAX_HALLAZGOS} hallazgos; hay mas en el log de la etapa (logs/rust.log).`,
    severity: 'information',
    range: { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } },
    source: 'rust',
    confidence: 1,
  });
}

const entries = [...porArchivo.entries()].map(([ruta, findings]) => ({ ruta, findings }));
const contar = (severidad) =>
  entries.reduce((n, e) => n + e.findings.filter((f) => f.severity === severidad).length, 0);

const reporte = {
  schemaVersion: '1',
  tool: { name: 'sentinel-rust', version: '1.1.0' },
  mode: MODO_FAST ? 'fast' : 'full',
  scope: 'workspace',
  durationMs: Date.now() - inicio,
  severityCounts: {
    error: contar('error'),
    warning: contar('warning'),
    information: contar('information') + contar('hint'),
    hint: 0,
  },
  totalArchivos: PAQUETES.length,
  totalArchivosConViolaciones: entries.filter((e) =>
    e.findings.some((f) => f.severity === 'error' || f.severity === 'warning'),
  ).length,
  entries,
};

fs.writeFileSync(path.resolve(reportPath), JSON.stringify(reporte, null, 2));
fs.writeFileSync(
  path.resolve(`${reportPath}.detalle.json`),
  JSON.stringify(
    {
      paquetes: PAQUETES,
      targetDir,
      volumenTarget,
      libreAntesGB,
      minimoLibreGB: MINIMO_LIBRE_GB,
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
      sccache: {
        antes: statsSccache,
        despues: statsDespues,
        invocaciones: invocacionesSccache,
      },
      alcanceLeido: alcance.length,
    },
    null,
    2,
  ),
);

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
