/* Modo rapido de la etapa rust (179A-1): decide si el alcance cabe en
   `cargo check --tests` + `cargo test --lib` filtrado, y de que modulos. Es
   fallo cerrado: si no cabe, devuelve el motivo y el orquestador pide el
   completo. Solo se invoca en modo rapido. */

/* Limites del rapido (179A-1 F3): por encima, el alcance pide el completo. */
const FAST_MAX_ARCHIVOS = 10;
const FAST_MAX_FILTROS = 5;
const esContrato = (archivo) =>
  archivo.startsWith('core/src/contrato/') || archivo.startsWith('cli/src/contrato/');

/* Heuristica alcance -> filtros de libtest (179A-1 F2): el nombre del modulo
   (directorio padre si el fichero es mod/lib/main.rs, si no el stem) como
   subcadena del filtro de cargo test. Medido en F1: `turno` corre 8 tests y
   filtra 328 en ~1,3 s en caliente. Solo .rs bajo core/src o cli/src; el
   resto (p. ej. .ts del front) no aporta filtros. */
const filtroPara = (archivo) => {
  const coincidencia = /^(?:core|cli)\/src\/(.+)\.rs$/.exec(archivo);
  if (!coincidencia) return null;
  const partes = coincidencia[1].split('/');
  let base = partes[partes.length - 1];
  if ((base === 'mod' || base === 'lib' || base === 'main') && partes.length > 1) {
    base = partes[partes.length - 2];
  }
  if (!/^[a-z0-9_]+$/.test(base)) return null;
  return base;
};
const paquetePara = (archivo) =>
  archivo.startsWith('core/src/') ? 'glory-harness-core' : archivo.startsWith('cli/src/') ? 'glory-harness' : null;

/* Devuelve { motivoNoAplicaFast, filtrosPorPaquete }; motivo nulo = el rapido aplica. */
export const planificarRapido = (alcance, tocaDesktop, tocaManifiesto) => {
  let motivoNoAplicaFast = null;
  if (alcance.length === 0) {
    motivoNoAplicaFast = 'sin alcance declarado (ni scope-manifest.json ni changed-files.txt)';
  } else if (tocaDesktop || tocaManifiesto) {
    motivoNoAplicaFast = 'el alcance toca desktop/src-tauri o manifiestos Cargo';
  } else if (alcance.some(esContrato)) {
    motivoNoAplicaFast = 'el alcance toca el contrato publico (core/src/contrato/ o cli/src/contrato/)';
  } else if (alcance.length > FAST_MAX_ARCHIVOS) {
    motivoNoAplicaFast = `el alcance tiene ${alcance.length} archivos (maximo ${FAST_MAX_ARCHIVOS} para el rapido)`;
  }

  const filtrosPorPaquete = new Map();
  if (motivoNoAplicaFast === null) {
    for (const archivo of alcance) {
      const filtro = filtroPara(archivo);
      const paquete = paquetePara(archivo);
      if (!filtro || !paquete) continue;
      if (!filtrosPorPaquete.has(paquete)) filtrosPorPaquete.set(paquete, new Set());
      filtrosPorPaquete.get(paquete).add(filtro);
    }
    const totalFiltros = [...filtrosPorPaquete.values()].reduce((n, set) => n + set.size, 0);
    if (totalFiltros > FAST_MAX_FILTROS) {
      motivoNoAplicaFast = `el alcance deriva ${totalFiltros} filtros de test (maximo ${FAST_MAX_FILTROS} para el rapido)`;
      filtrosPorPaquete.clear();
    } else if (totalFiltros === 0) {
      motivoNoAplicaFast =
        'el alcance no incluye archivos `.rs` bajo core/src o cli/src (sin filtros de test)';
    }
  }
  return { motivoNoAplicaFast, filtrosPorPaquete };
};
