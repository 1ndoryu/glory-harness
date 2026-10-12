import { spawnSync } from 'node:child_process';
import path from 'node:path';

import { espacioLibreGB } from './parseo.mjs';

/* Entorno de cargo de la etapa: target fuera del arbol, sccache y preflight de
   disco. Se resuelve una vez y el orquestador recibe el resultado. */

export const fail = (mensaje) => {
  process.stderr.write(mensaje + '\n');
  process.exit(2);
};

/* El target vive fuera del arbol del proyecto (regla del area): se resuelve
   por CARGO_TARGET_DIR / GLORY_CARGO_TARGET_DIR / CARGO_TARGET_DIR_BASE y el
   respaldo es C:\tmp\glory-target\glory-harness, que es tambien lo que usa la
   purga horaria y lo que abarata sccache. */
export const prepararEntorno = (workspace) => {
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
  const minimoLibreGB = Number(process.env.GLORY_MIN_FREE_GB ?? 8);
  const volumenTarget = path.parse(path.resolve(targetDir)).root;
  const libreAntesGB = espacioLibreGB(volumenTarget);
  if (libreAntesGB !== null && libreAntesGB < minimoLibreGB) {
    fail(
      `rust: quedan ${libreAntesGB.toFixed(2)} GB libres en ${volumenTarget} ` +
        `(minimo ${minimoLibreGB} GB para el target + shell). Un build con el disco lleno ` +
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
    SCCACHE_CACHE_SIZE: process.env.SCCACHE_CACHE_SIZE || '10G',
    CARGO_TERM_COLOR: 'never',
    CARGO_TERM_PROGRESS_WHEN: 'never',
  };
  return { targetDir, volumenTarget, libreAntesGB, minimoLibreGB, entorno };
};

export const correrCargo = (workspace, entorno, args) => {
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
