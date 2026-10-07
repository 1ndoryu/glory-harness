/* Tipos de memoria/compactación/comandos del adaptador real.
 * Extraído de `realTipos.ts` (superaba el límite de 300 líneas): sin cambios
 * de contrato; `realTipos.ts` re-exporta este módulo. */

/** [109A-3] Un recuerdo del proyecto activo (DTO del backend). */
export interface RecuerdoMemoria {
  clave: string;
  contenido: string;
  origen: string;
  usos: number;
  ultimo_uso: string | null;
  actualizada_en: string;
  /** Archivado por el curador: se conserva para auditar, no se inyecta. */
  archivada: boolean;
}

/** [109A-3] Ámbito activo resuelto por el backend y sus recuerdos. El front
 * no elige el ámbito: el panel siempre muestra el proyecto abierto. */
export interface ListadoMemoria {
  /** `true` = ámbito global (la carpeta activa no es un área registrada). */
  global: boolean;
  /** Etiqueta del núcleo: `global` o `proyecto`. */
  ambito: string;
  /** Nombre del área activa; `null` en el ámbito global. */
  proyecto: string | null;
  ruta: string | null;
  /** Carpeta `.glory/memorias` del área activa; `null` sin área. */
  carpeta: string | null;
  recuerdos: RecuerdoMemoria[];
}

/** [109A-3] Resultado de exportar o importar la carpeta de memorias. */
export interface ResultadoCarpetaMemoria {
  carpeta: string;
  recuerdos: number;
  /** Archivos rechazados al importar, con motivo (vacío al exportar). */
  omitidos: string[];
}

/** [109A-4 F3] Resultado de `/compactar`. Sin el resumen del tramo: se
 * persiste como punto de compactación y NO se pinta en el chat (el historial
 * visible no cambia). `compactado: false` trae el `motivo` del no-op. */
export interface ResumenCompactacion {
  compactado: boolean;
  motivo: string | null;
  tokens_antes: number;
  tokens_despues: number;
  ahorro_pct: number;
  ocupacion_pct: number;
  tramos: number;
}

/** [109A-4] Comando `/` definido por el área activa (`.glory/comandos/*.md`). */
export interface ComandoArea {
  nombre: string;
  descripcion: string;
}
