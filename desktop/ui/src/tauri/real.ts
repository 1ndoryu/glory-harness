// Barrel del adaptador real (split 089A-16 F2-resto): solo re-exports.
// Tipos en `realTipos.ts`, transporte Tauri en `transporteTauri.ts`,
// descripción de tools en `descripcionHerramientas.ts`, render de eventos
// en `aplicarEventos.ts`, núcleo del turno en `turnoReal.ts` y fachada de
// sesión en `adaptadorReal.ts`. Los ~16 importadores quedan intactos.
export * from './realTipos';
export { transporteTauri } from './transporteTauri';
export { compacto, descripcionDeTool, iconoDeTool, rutaDeArgumentos } from './descripcionHerramientas';
export { crearTurnoReal, type TurnoReal } from './turnoReal';
export { crearAdaptadorReal, type AdaptadorReal } from './adaptadorReal';

