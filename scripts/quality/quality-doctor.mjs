import { spawnSync } from 'node:child_process';
import { sentinelInvocation } from './sentinel-cli.mjs';

const invocation = sentinelInvocation(process.cwd(), ['doctor', '--json', '--workspace', process.cwd()]);
const result = spawnSync(invocation.executable, invocation.args, {
  cwd: process.cwd(),
  encoding: 'utf8',
  windowsHide: true,
  stdio: 'inherit',
});

if (result.error) {
  process.stderr.write(`No se pudo ejecutar Sentinel doctor: ${result.error.message}\n`);
  process.exit(2);
}
process.exit(result.status ?? 2);
