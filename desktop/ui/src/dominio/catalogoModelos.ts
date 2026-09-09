// Catálogo de proveedores/modelos (port 1:1 del mockup).

import type { ModeloSeleccionado, ProveedorModelo } from './tipos';

export const PROVEEDORES: ProveedorModelo[] = [
  {
    id: 'glory',
    etiqueta: 'Glory API',
    modelos: [
      { modelo: 'auto', nombre: 'Auto (recomendado)' },
      { modelo: 'commandcode', nombre: 'Commandcode (alias auto)' },
      { modelo: 'deepseek-v4-flash', nombre: 'DeepSeek V4 Flash' },
      { modelo: 'deepseek-v4-flash-free', nombre: 'DeepSeek V4 Flash Free' },
      { modelo: 'glm-5.3-flash', nombre: 'GLM 5.3 Flash (alias)' },
    ],
  },
  {
    id: 'commandcode',
    etiqueta: 'Command Code Provider',
    modelos: [
      { modelo: 'poolside/laguna-s-2.1-free', nombre: 'Laguna S 2.1 Free' },
      { modelo: 'meta/muse-spark-1.2-contributor', nombre: 'Muse Spark 1.2 Contributor' },
      { modelo: 'stealth/ox-alpha', nombre: 'OX Alpha' },
    ],
  },
  {
    id: 'deepseek',
    etiqueta: 'DeepSeek (directo)',
    modelos: [{ modelo: 'deepseek-v4-flash', nombre: 'DeepSeek V4 Flash' }],
  },
];

/** Modelo activo al abrir la app (el mismo que muestra el mockup). */
export const MODELO_INICIAL: ModeloSeleccionado = {
  proveedor: 'commandcode',
  modelo: 'poolside/laguna-s-2.1-free',
  nombre: 'Laguna S 2.1 Free',
};
