/* Tipos de consola del adaptador real (sub-barra + transcript + creación).
 * Extraído de `realTipos.ts` (superaba el límite de 300 líneas): sin cambios
 * de contrato; `realTipos.ts` re-exporta este módulo. */

/** [219A-3] Entrada de la sub-barra de la tab Consola: fiel al endpoint
 * `GET /consolas` y al comando Tauri `consolas_listar` (vivas + recientes).
 * `codigo_salida` `null` = viva (aún sin código).
 * [219A-4] `origen` = dueño (`agente` = la abrió el modelo; `usuario` = la
 * abrió el operador con [+ Nueva]). */
export interface InfoConsolaLista {
  id_ejecucion: string;
  comando: string;
  viva: boolean;
  codigo_salida: number | null;
  origen: 'agente' | 'usuario';
}

/** [219A-3] Línea del transcript retenido (`salida`): mismos literales de
 * `flujo` que `consola_chunk`. */
export interface LineaConsola {
  flujo: 'stdout' | 'stderr';
  linea: string;
}

/** [219A-3] Transcript retenido por consola: fiel al endpoint
 * `GET /consolas/:eid/salida` y al comando Tauri `consola_salida`.
 * [219A-4] Incluye `origen` (dueño) como la lista. */
export interface TranscriptConsola {
  id_ejecucion: string;
  comando: string;
  viva: boolean;
  codigo_salida: number | null;
  origen: 'agente' | 'usuario';
  lineas: LineaConsola[];
}

/** [219A-4] Nueva consola propia: fiel a `POST /consolas` y al comando
 * Tauri `consola_crear` (`origen` siempre `usuario`). */
export interface NuevaConsola {
  id_ejecucion: string;
  comando: string;
  origen: 'agente' | 'usuario';
}
