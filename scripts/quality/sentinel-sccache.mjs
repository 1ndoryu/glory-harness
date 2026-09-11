import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

/* Etapa sccache del gate: ningún proyecto Rust compila sin caché en
   silencio. Si hay fuentes Rust trackeadas (Cargo.toml o .rs) y no hay
   evidencia de rustc-wrapper=sccache (ni en entorno ni en .cargo/config),
   produce un error sccache-no-configurado. Sin Rust, pasa vacío. */

const reportPath = process.argv[2];
if (!reportPath) {
  process.stderr.write('sentinel-sccache requiere {reportPath}\n');
  process.exit(2);
}

const workspace = process.cwd();
const inicio = Date.now();
const fail = (mensaje) => {
  process.stderr.write(mensaje + '\n');
  process.exit(2);
};

const git = spawnSync('git', ['ls-files'], {
  cwd: workspace,
  encoding: 'utf8',
  windowsHide: true,
});
if (git.error || git.status !== 0) {
  fail(`sccache: git ls-files fallo: ${git.error?.message ?? git.stderr}`);
}
const rastreados = git.stdout.split('\n').map((l) => l.trim()).filter(Boolean);
const esRust = rastreados.some((r) => {
  const base = r.split('/').pop().toLowerCase();
  return base === 'cargo.toml' || base.endsWith('.rs');
});

const hallazgo = (ruleId, message, severity) => ({
  ruleId,
  message,
  severity,
  range: {
    start: { line: 0, character: 0 },
    end: { line: 0, character: 0 },
  },
  source: 'sccache',
  confidence: 1,
});

const findings = [];
if (esRust) {
  const envWrapper = [process.env.RUSTC_WRAPPER, process.env.CARGO_BUILD_RUSTC_WRAPPER]
    .filter(Boolean)
    .join(' ')
    .toLowerCase();
  const configs = [
    path.join(workspace, '.cargo', 'config.toml'),
    path.join(workspace, '.cargo', 'config'),
  ];
  const home = process.env.CARGO_HOME || path.join(process.env.USERPROFILE || '', '.cargo');
  if (home) {
    configs.push(path.join(home, 'config.toml'), path.join(home, 'config'));
  }
  const wrapperEnConfig = configs.some((ruta) => {
    try {
      const texto = fs.readFileSync(ruta, 'utf8').toLowerCase();
      return /rustc-wrapper\s*=\s*"[^"]*sccache[^"]*"/.test(texto);
    } catch {
      return false;
    }
  });
  if (!envWrapper.includes('sccache') && !wrapperEnConfig) {
    findings.push(
      hallazgo(
        'sccache-no-configurado',
        'Proyecto Rust sin rustc-wrapper=sccache: ni RUSTC_WRAPPER/CARGO_BUILD_RUSTC_WRAPPER ' +
          'en el entorno ni rustc-wrapper en .cargo/config apuntan a sccache. ' +
          'Cada rebuild tras la purga de C:\\tmp recompila desde cero. ' +
          'Fija RUSTC_WRAPPER=sccache (con SCCACHE_CACHE_SIZE acotado).',
        'error',
      ),
    );
  } else {
    const bin = spawnSync('sccache', ['--version'], {
      encoding: 'utf8',
      windowsHide: true,
    });
    if (bin.error || bin.status !== 0) {
      findings.push(
        hallazgo(
          'sccache-binario-ausente',
          'Hay wrapper sccache configurado pero el binario `sccache` no resuelve en PATH: ' +
            'la compilación fallará al invocar el wrapper.',
          'warning',
        ),
      );
    }
  }
}

const errores = findings.filter((f) => f.severity === 'error').length;
const avisos = findings.filter((f) => f.severity === 'warning').length;

const reporte = {
  schemaVersion: '1',
  tool: { name: 'sentinel-sccache', version: '1.0.0' },
  scope: 'workspace',
  durationMs: Date.now() - inicio,
  severityCounts: { error: errores, warning: avisos, information: 0, hint: 0 },
  totalArchivos: esRust ? 1 : 0,
  totalArchivosConViolaciones: errores > 0 || avisos > 0 ? 1 : 0,
  entries: findings.length > 0 ? [{ ruta: workspace, findings }] : [],
};

fs.writeFileSync(path.resolve(reportPath), JSON.stringify(reporte, null, 2));

process.stdout.write(
  `sccache: proyecto ${esRust ? 'Rust' : 'sin Rust'}, ` +
    `${errores} errores, ${avisos} avisos.\n`,
);
process.exit(0);
