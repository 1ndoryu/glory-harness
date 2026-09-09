// Barrel de entrada (split 089A-16 F2-resto): solo re-exports. Tipos y
// constantes en `entradaTipos.ts`, barras en `entradaBarras.ts`, indicador
// en `entradaContexto.ts` y la factoría `montarEntrada` en
// `entradaMontaje.ts`. Importadores intactos (`panelChat`, `modal`, …).
export {
  ETIQUETA_MODO,
  ETIQUETA_RAZONAMIENTO,
  MODOS_EJECUCION,
  VALORES_RAZONAMIENTO,
} from './entradaTipos';
export type {
  Entrada,
  EntradaOpciones,
  EstadoContexto,
  ModoEjecucion,
  VarianteEntrada,
} from './entradaTipos';
export { montarEntrada } from './entradaMontaje';
