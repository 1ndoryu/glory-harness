/* Tipos del adaptador real (contrato AgenteEvento, sesión, transporte).
 * Solo tipos + `esEntornoTauri`; sin runtime salvo esa guarda.
 *
 * [Partición límite-300] Los tipos viven en módulos por dominio bajo `./tipos/`
 * (subdirectorio: `tauri/` tiene tope de 10 archivos) y este fichero es el
 * barrel que preserva la ruta de importación (`./realTipos` y `export *` en
 * `real.ts`): ningún consumidor cambia.
 * - `tipos/realTiposEventos.ts`: `AgenteEvento` + `EventoTurnoLog`.
 * - `tipos/realTiposConsola.ts`: sub-barra/transcript/creación de consolas.
 * - `tipos/realTiposSesion.ts`: sesión/conversación/turno.
 * - `tipos/realTiposMemoria.ts`: memorias/compactación/comandos de área.
 * - `tipos/realTiposTransporte.ts`: `Transporte` + `HooksAdaptador` + `esEntornoTauri`. */
export * from './tipos/realTiposEventos';
export * from './tipos/realTiposConsola';
export * from './tipos/realTiposSesion';
export * from './tipos/realTiposMemoria';
export * from './tipos/realTiposTransporte';
