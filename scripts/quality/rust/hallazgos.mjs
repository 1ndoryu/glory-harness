/* Acumulador de hallazgos de la etapa rust. Modulo con estado a proposito: los
   modulos ES son singleton, asi el orquestador y el reporte comparten la misma
   cola sin pasarla por parametro. */

export const MAX_HALLAZGOS = 60;
export const porArchivo = new Map();
export const cola = [];

const RANGO_GENERAL = { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } };

/* Hallazgo sin linea (estado de la etapa): va al workspace, no a un archivo. */
export const hallazgoGeneral = (ruleId, message, severity) => ({
  ruleId,
  message,
  severity,
  range: RANGO_GENERAL,
  source: 'rust',
  confidence: 1,
});

/* Devuelve false si el tope de hallazgos ya se alcanzo; el caller decide. */
export const acumular = (ruta, finding) => {
  if (cola.length >= MAX_HALLAZGOS) return false;
  cola.push(ruta);
  if (!porArchivo.has(ruta)) porArchivo.set(ruta, []);
  porArchivo.get(ruta).push(finding);
  return true;
};
