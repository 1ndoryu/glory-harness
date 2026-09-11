// Barrel de mensajes (split 089A-16 F2-resto): solo re-exports. Los bloques
// viven en `mensajesBloques.ts`, las utilidades en `mensajesUtil.ts` y los
// constructores de mensaje + render de historial en `mensajesNucleo.ts`.
export {
  crearAvisoSistema,
  crearHerramienta,
  crearHerramientaViva,
  crearRazonamientoCerrado,
  crearRazonamientoVivo,
  crearTarjetaAprobacion,
} from './mensajesBloques';
export type {
  AccionAviso,
  HerramientaViva,
  RazonamientoVivo,
  TarjetaAprobacionViva,
} from './mensajesBloques';
export {
  aplicarResultado,
  formatearResultadoHerramienta,
  ponerHtmlSeguro,
} from './mensajesUtil';
export {
  crearMensajeAsistente,
  crearMensajeAsistenteVivo,
  crearMensajeUsuario,
  crearPieTurno,
  pintarLogroEnPie,
  renderizarBloque,
  tokensCortos,
} from './mensajesNucleo';
export type { AsistenteVivo, PieTurno } from './mensajesNucleo';
