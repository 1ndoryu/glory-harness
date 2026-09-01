import fs from 'node:fs';
import path from 'node:path';

function configuredCliPath(workspace) {
  const configPath = path.join(workspace, 'quality-tools.json');
  try {
    const config = JSON.parse(fs.readFileSync(configPath, 'utf8'));
    const sentinel = config?.tools?.sentinel;
    const configuredProvision = typeof sentinel?.provisionPath === 'string'
      ? sentinel.provisionPath
      : null;
    const provisionPath = configuredProvision
      ? path.resolve(workspace, configuredProvision)
      : path.resolve(
        workspace,
        config?.installRoot ?? '.quality-tools',
        'sentinel',
        'versions',
        String(sentinel?.version ?? ''),
      );
    return path.join(provisionPath, 'out', 'cli', 'index.js');
  } catch {
    return null;
  }
}

export function sentinelInvocation(workspace, args = []) {
  const cliPath = configuredCliPath(workspace);
  if (cliPath && fs.existsSync(cliPath)) {
    return { executable: process.execPath, args: [cliPath, ...args] };
  }
  const executable = process.platform === 'win32' ? 'sentinel.cmd' : 'sentinel';
  return { executable, args };
}
