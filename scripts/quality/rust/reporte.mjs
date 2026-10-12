import fs from 'node:fs';
import path from 'node:path';

import { MAX_HALLAZGOS, cola, hallazgoGeneral, porArchivo } from './hallazgos.mjs';

/* Escribe el reporte de la etapa (contrato con el gate: schemaVersion, entries,
   severityCounts) y el detalle de diagnostico junto a el. Devuelve el reporte
   para que el orquestador resuma en consola. */
export const escribirReporte = (reportPath, { workspace, modoFast, inicio, paquetes, detalle }) => {
  if (cola.length >= MAX_HALLAZGOS) {
    /* El tope puede llegar antes de que exista la entrada del workspace (el
       alcance va al final), asi que se crea aqui en vez de asumirla. */
    if (!porArchivo.has(workspace)) porArchivo.set(workspace, []);
    porArchivo
      .get(workspace)
      .push(
        hallazgoGeneral(
          'rust-hallazgos-truncados',
          `Se listan ${MAX_HALLAZGOS} hallazgos; hay mas en el log de la etapa (logs/rust.log).`,
          'information',
        ),
      );
  }

  const entries = [...porArchivo.entries()].map(([ruta, findings]) => ({ ruta, findings }));
  const contar = (severidad) =>
    entries.reduce((n, e) => n + e.findings.filter((f) => f.severity === severidad).length, 0);

  const reporte = {
    schemaVersion: '1',
    tool: { name: 'sentinel-rust', version: '1.1.0' },
    mode: modoFast ? 'fast' : 'full',
    scope: 'workspace',
    durationMs: Date.now() - inicio,
    severityCounts: {
      error: contar('error'),
      warning: contar('warning'),
      information: contar('information') + contar('hint'),
      hint: 0,
    },
    totalArchivos: paquetes.length,
    totalArchivosConViolaciones: entries.filter((e) =>
      e.findings.some((f) => f.severity === 'error' || f.severity === 'warning'),
    ).length,
    entries,
  };

  fs.writeFileSync(path.resolve(reportPath), JSON.stringify(reporte, null, 2));
  fs.writeFileSync(path.resolve(`${reportPath}.detalle.json`), JSON.stringify(detalle, null, 2));
  return reporte;
};
