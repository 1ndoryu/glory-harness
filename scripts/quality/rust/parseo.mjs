import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

/* Lectores y parsers de la etapa rust (sentinel-rust.mjs). Sin estado propio:
   cada funcion recibe lo que necesita y el orquestador acumula. */

/* El gate deja el alcance junto al reporte (changed-files.txt y
   scope-manifest.json). Sirve para no compilar el shell Tauri en cada tarea:
   su arbol (tauri/wry/webview2) es caro y solo se paga cuando se toca. */
export const leerAlcance = (reportDir) => {
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

/* Espacio libre del volumen en GB; null si statfs no esta disponible (se omite
   el dato en vez de inventarlo o bloquear). */
export const espacioLibreGB = (ruta) => {
  try {
    const stats = fs.statfsSync(ruta);
    return (Number(stats.bavail) * Number(stats.bsize)) / 1024 ** 3;
  } catch {
    return null;
  }
};

/* sccache no expone un flag "se uso"; se mide por diferencia de contadores. Si
   el binario no responde se devuelve null y la etapa solo omite el dato: la
   politica de sccache la decide su propia etapa, aqui se informa. */
export const leerStatsSccache = (workspace, entorno) => {
  const resultado = spawnSync('sccache', ['--show-stats', '--stats-format=json'], {
    cwd: workspace,
    encoding: 'utf8',
    windowsHide: true,
    env: entorno,
    timeout: 30000,
  });
  if (resultado.status !== 0 || !resultado.stdout) return null;
  try {
    const estadisticas = JSON.parse(resultado.stdout).stats ?? {};
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
export const resumirSuites = (salida, resumenTests) => {
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
export const diagnosticosDe = (salida, workspace) => {
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
export const colaUtil = (salida) =>
  salida
    .split('\n')
    .map((linea) => linea.replace(/\r$/, ''))
    .filter((linea) => linea.trim() && !linea.startsWith('{'))
    .slice(-12);
