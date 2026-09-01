import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';
import { sentinelInvocation } from './sentinel-cli.mjs';

const reportPath = process.argv[2];
if (!reportPath) {
  process.stderr.write('sentinel-stage requiere {reportPath}\n');
  process.exit(2);
}

const workspace = process.cwd();
const invocation = sentinelInvocation(workspace, [
  'analyze',
  '--workspace', workspace,
  '--format', 'json',
  '--output', reportPath,
]);
const result = spawnSync(invocation.executable, invocation.args, {
  cwd: workspace,
  encoding: 'utf8',
  windowsHide: true,
  stdio: ['ignore', 'pipe', 'pipe'],
});

if (result.error) {
  process.stderr.write(`No se pudo ejecutar Sentinel: ${result.error.message}\n`);
  process.exit(2);
}

if (!fs.existsSync(path.resolve(reportPath))) {
  process.stderr.write('Sentinel no produjo el reporte esperado.\n');
  process.exit(result.status === 0 ? 2 : result.status ?? 2);
}

// El analizador devuelve código 1 cuando hay hallazgos; el gate los clasifica
// desde el reporte estructurado y reserva los códigos no cero para errores.
if ((result.status ?? 0) > 1) {
  process.stderr.write(result.stderr || 'Sentinel terminó con error de herramienta.\n');
  process.exit(result.status ?? 2);
}
process.exit(0);
