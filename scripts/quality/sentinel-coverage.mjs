import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

/* Etapa coverage del gate (089A-15): ninguna fuente trackeada queda fuera
   del analisis en silencio. Cruza git ls-files contra los include/exclude
   de sentinel.config.json y varsense.config.json. Cada fuente analizable
   sin cubrir produce un error sin-cobertura; lo que ningun motor sabe
   analizar se lista en el sidecar, nunca se oculta. */

const reportPath = process.argv[2];
if (!reportPath) {
  process.stderr.write('sentinel-coverage requiere {reportPath}\n');
  process.exit(2);
}

const workspace = process.cwd();
const inicio = Date.now();
const fail = (mensaje) => {
  process.stderr.write(mensaje + '\n');
  process.exit(2);
};

const leerJson = (relativa) => {
  try {
    return JSON.parse(fs.readFileSync(path.join(workspace, relativa), 'utf8'));
  } catch (error) {
    fail(`coverage: no se pudo leer ${relativa}: ${error.message}`);
  }
};

const sentinelConfig = leerJson('sentinel.config.json');
const varsenseConfig = leerJson('varsense.config.json');
const includes = [
  ...(sentinelConfig?.analyzers?.sentinel?.config?.includePatterns ?? []),
  ...(varsenseConfig?.includePatterns ?? []),
];
const excludes = [
  ...(sentinelConfig?.analyzers?.sentinel?.config?.excludePatterns ?? []),
  ...(varsenseConfig?.excludePatterns ?? []),
];

/* Mini-glob: cubre los patrones que usamos (**, *, nombres exactos).
   Un patron con sintaxis no soportada se marca no evaluable en vez de
   asumir cobertura: el vigilante no puede tener puntos ciegos. */
const noEvaluables = [];
const aRegex = (patron) => {
  const p = String(patron).replace(/\\/g, '/');
  if (/[[{?+]/.test(p)) {
    noEvaluables.push(p);
    return null;
  }
  let re = '';
  let i = 0;
  while (i < p.length) {
    if (p.startsWith('**/', i)) {
      re += '(?:.*/)?';
      i += 3;
    } else if (p.startsWith('**', i)) {
      re += '.*';
      i += 2;
    } else if (p[i] === '*') {
      re += '[^/]*';
      i += 1;
    } else {
      re += p[i].replace(/[.+^${}()|\\]/g, '\\$&');
      i += 1;
    }
  }
  if (!p.includes('/')) {
    if (p.includes('*')) {
      return new RegExp(`^(?:.*/)?${re}$`);
    }
    return new RegExp(`^${re}$`);
  }
  return new RegExp(`^${re}$`);
};

const incluyeRx = includes.map(aRegex).filter(Boolean);
const excluyeRx = excludes.map(aRegex).filter(Boolean);
const coincide = (relativa, rxs) => rxs.some((rx) => rx.test(relativa));

const git = spawnSync('git', ['ls-files'], {
  cwd: workspace,
  encoding: 'utf8',
  windowsHide: true,
});
if (git.error || git.status !== 0) {
  fail(`coverage: git ls-files fallo: ${git.error?.message ?? git.stderr}`);
}
const rastreados = git.stdout.split('\n').map((l) => l.trim()).filter(Boolean);

const EXT_ANALIZABLE = new Set(['.rs', '.ts', '.tsx', '.js', '.mjs', '.cjs']);
const NOMBRE_ANALIZABLE = new Set(['Cargo.toml', 'Cargo.lock', 'package.json']);
const EXT_VISIBLE = new Set([
  '.css', '.html', '.json', '.toml', '.yaml', '.yml',
  '.ps1', '.bat', '.py', '.sh', '.xml', '.vue', '.svelte',
]);

const esAnalizable = (relativa) => {
  const base = relativa.split('/').pop();
  if (NOMBRE_ANALIZABLE.has(base)) return true;
  return EXT_ANALIZABLE.has(path.posix.extname(base).toLowerCase());
};

const sinCubrir = [];
const noAnalizables = [];
let analizables = 0;
for (const relativa of rastreados) {
  if (!esAnalizable(relativa)) {
    const ext = path.posix.extname(relativa).toLowerCase();
    if (EXT_VISIBLE.has(ext)) noAnalizables.push(relativa);
    continue;
  }
  analizables += 1;
  if (coincide(relativa, excluyeRx)) continue;
  if (!coincide(relativa, incluyeRx)) sinCubrir.push(relativa);
}

const entries = sinCubrir.map((relativa) => ({
  ruta: path.join(workspace, ...relativa.split('/')),
  findings: [
    {
      ruleId: 'sin-cobertura',
      message:
        `Fuente analizable fuera de todos los includePatterns: ${relativa}. ` +
        'Anadirla a includes o justificarla en excludes.',
      severity: 'error',
      range: {
        start: { line: 0, character: 0 },
        end: { line: 0, character: 0 },
      },
      source: 'coverage',
      confidence: 1,
    },
  ],
}));

if (noEvaluables.length > 0) {
  entries.push({
    ruta: workspace,
    findings: noEvaluables.map((patron) => ({
      ruleId: 'cobertura-no-evaluable',
      message:
        `Patron con sintaxis no soportada por coverage, no se pudo verificar: ${patron}.`,
      severity: 'warning',
      range: {
        start: { line: 0, character: 0 },
        end: { line: 0, character: 0 },
      },
      source: 'coverage',
      confidence: 1,
    })),
  });
}

const errores = entries.reduce(
  (n, e) => n + e.findings.filter((f) => f.severity === 'error').length,
  0,
);
const avisos = entries.reduce(
  (n, e) => n + e.findings.filter((f) => f.severity === 'warning').length,
  0,
);

const reporte = {
  schemaVersion: '1',
  tool: { name: 'sentinel-coverage', version: '1.0.0' },
  scope: 'workspace',
  durationMs: Date.now() - inicio,
  severityCounts: { error: errores, warning: avisos, information: 0, hint: 0 },
  totalArchivos: analizables,
  totalArchivosConViolaciones: sinCubrir.length,
  entries,
};

fs.writeFileSync(path.resolve(reportPath), JSON.stringify(reporte, null, 2));
fs.writeFileSync(
  path.resolve(`${reportPath}.detalle.json`),
  JSON.stringify(
    {
      totalRastreados: rastreados.length,
      analizables,
      cubiertos: analizables - sinCubrir.length,
      sinCubrir,
      noAnalizablesPorMotor: noAnalizables,
      patronesNoEvaluables: noEvaluables,
    },
    null,
    2,
  ),
);

process.stdout.write(
  `coverage: ${analizables} analizables, ${analizables - sinCubrir.length} cubiertas, ` +
    `${sinCubrir.length} sin cubrir, ${noAnalizables.length} visibles-no-analizables.\n`,
);
process.exit(0);
