import { spawnSync } from 'node:child_process';
import { sentinelInvocation } from './sentinel-cli.mjs';

const workspace = process.cwd();
const invocation = sentinelInvocation(workspace, [
  'analyze',
  '--workspace', workspace,
  '--format', 'json',
  '--output', '.quality-reports/analyze.json',
]);
const result = spawnSync(invocation.executable, invocation.args, {
  cwd: workspace,
  stdio: 'inherit',
  windowsHide: true,
});

if (result.error) {
  process.stderr.write(`No se pudo ejecutar Sentinel: ${result.error.message}\n`);
  process.exit(2);
}
process.exit(result.status ?? 2);
