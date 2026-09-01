/* quality:setup — genera la evidencia release real de los analyzers.
 *
 * Para cada tool declarada en quality-tools.json verifica que el checkout de
 * sourcePath esté en el commit fijado y limpio, compila (buildScript) y corre
 * la suite (testScript) en ese checkout. Solo si todo pasa escribe
 * .sentinel/release-evidence/<tool>.json (gitignored por convención del
 * proyecto: la evidencia es por máquina). Nunca fabrica evidencia: ante
 * cualquier fallo no escribe el archivo y sale con error claro.
 */
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import path from 'node:path';

const workspace = process.cwd();

function run(cmd, args, cwd, label) {
  // En Windows `npm` es un shim (npm.cmd): spawnSync solo lo ejecuta con shell.
  // Args simples (nombres de script sin espacios); un solo string evita DEP0190.
  const result = spawnSync([cmd, ...args].join(' '), { cwd, stdio: 'inherit', windowsHide: true, shell: true });
  if (result.error) {
    process.stderr.write(`[quality:setup] no se pudo ejecutar ${label}: ${result.error.message}\n`);
    process.exit(2);
  }
  if (result.status !== 0) {
    process.stderr.write(`[quality:setup] ${label} falló (exit ${result.status}); no se escribe evidencia\n`);
    process.exit(1);
  }
}

function git(cwd, ...args) {
  const result = spawnSync('git', args, { cwd, encoding: 'utf8', windowsHide: true });
  if (result.error || result.status !== 0) return null;
  return result.stdout.trim();
}

const manifest = JSON.parse(fs.readFileSync(path.join(workspace, 'quality-tools.json'), 'utf8'));
const tools = manifest?.tools;
if (!tools || typeof tools !== 'object') {
  process.stderr.write('[quality:setup] quality-tools.json sin sección tools\n');
  process.exit(2);
}

const evidenceDir = path.join(workspace, '.sentinel', 'release-evidence');

for (const [name, config] of Object.entries(tools)) {
  const source = path.resolve(workspace, config.sourcePath);
  const commit = config.commit;
  const buildScript = config.buildScript;
  const testScript = config.testScript;
  process.stdout.write(`[quality:setup] ${name}: staging ${source} (${commit})\n`);

  const head = git(source, 'rev-parse', 'HEAD');
  if (head !== commit) {
    process.stderr.write(`[quality:setup] ${name}: checkout en ${head ?? '??'} != commit fijado ${commit}; sin evidencia\n`);
    process.exit(1);
  }
  const porcelain = git(source, 'status', '--porcelain') ?? '';
  const unexpected = porcelain
    .split(/\r?\n/)
    .filter(Boolean)
    .map(line => line.slice(3))
    .filter(change => change !== '.quality-install.json');
  if (unexpected.length > 0) {
    process.stderr.write(`[quality:setup] ${name}: staging sucio (${unexpected.join(', ')}); sin evidencia\n`);
    process.exit(1);
  }

  if (buildScript) {
    process.stdout.write(`[quality:setup] ${name}: compilando (npm run ${buildScript})\n`);
    run('npm', ['run', buildScript], source, `${name} compile`);
  }
  if (testScript) {
    process.stdout.write(`[quality:setup] ${name}: suite (npm run ${testScript})\n`);
    run('npm', ['run', testScript], source, `${name} suite`);
  }

  fs.mkdirSync(evidenceDir, { recursive: true });
  const evidence = {
    schemaVersion: 1,
    tool: name,
    commit,
    compile: 'passed',
    suite: testScript ? 'passed' : 'not-configured',
    cleanStaging: true,
    at: new Date().toISOString(),
  };
  fs.writeFileSync(path.join(evidenceDir, `${name}.json`), `${JSON.stringify(evidence, null, 2)}\n`);
  process.stdout.write(`[quality:setup] ${name}: evidencia escrita\n`);
}

process.stdout.write('[quality:setup] evidencia release generada para todos los analyzers\n');
