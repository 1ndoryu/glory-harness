// Barrel de mensajes (split 089A-16 F2-resto): solo re-exports. Los bloques
// viven en `mensajesBloques.ts`, las utilidades en `mensajesUtil.ts`, los
// constructores de mensaje + render de historial en `mensajesNucleo.ts` y el
// resumen de cambios del turno en `resumenTurno.ts` ([129A-8]).
export { crearResumenTurno } from './resumenTurno';
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
  crearMensajePendiente,
  crearMensajeUsuario,
  crearPieTurno,
  pintarLogroEnPie,
  renderizarBloque,
  tokensCortos,
} from './mensajesNucleo';
export type { AsistenteVivo, PieTurno } from './mensajesNucleo';
