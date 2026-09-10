/* Catálogo de comandos `/` del escritorio (plan 109A-4, catálogo v1 cerrado).
 *
 * Datos puros, sin DOM: el menú (`componentes/menuComandos.ts`) filtra y
 * pinta, y el ejecutor (`componentes/panelChatComandos.ts`) aplica el efecto.
 * Mantener los tres separados permite probar el filtrado sin montar UI.
 *
 * Los comandos del PROYECTO (`.glory/comandos/*.md`, formato del núcleo) no
 * viven aquí: se piden al backend y se distinguen por `origen: 'proyecto'`.
 */

export type CategoriaComando = 'ayuda' | 'sesion' | 'contexto' | 'proyecto' | 'modelo' | 'meta';

/** Comando disponible en el menú `/`. */
export interface ComandoSlash {
  /** Nombre sin la barra (`ayuda`, `compactar`). */
  nombre: string;
  /** Una línea para el menú. */
  resumen: string;
  /** Ayuda extendida (la muestra `/ayuda <nombre>`). */
  detalle: string;
  categoria: CategoriaComando;
  /** true si espera argumentos: al elegirlo el menú deja listo `/nombre `. */
  admiteArgumentos: boolean;
  /** Argumentos de uso, p. ej. `[id]`; vacío si no admite. */
  usoArgumentos: string;
  /** De dónde sale el comando (el menú los agrupa por esto). */
  origen: 'integrado' | 'proyecto';
}

/** Etiqueta visible de cada grupo del menú y de `/ayuda`. */
export const ETIQUETA_CATEGORIA: Record<CategoriaComando, string> = {
  ayuda: 'Ayuda',
  sesion: 'Sesión',
  contexto: 'Contexto',
  proyecto: 'Proyecto',
  modelo: 'Modelo',
  meta: 'Meta',
};

/** Catálogo v1 integrado (8 comandos, cerrado en la fase F1 del plan 109A-4). */
export const COMANDOS_BUILTIN: ComandoSlash[] = [
  {
    nombre: 'ayuda',
    resumen: 'lista los comandos y explica uno',
    detalle:
      'Sin argumentos muestra el catálogo agrupado. Con un nombre muestra el uso y el detalle de ese comando.',
    categoria: 'ayuda',
    admiteArgumentos: true,
    usoArgumentos: '[comando]',
    origen: 'integrado',
  },
  {
    nombre: 'modelo',
    resumen: 'muestra o cambia el modelo activo',
    detalle:
      'Sin argumentos lista los modelos disponibles. Con un id (o `proveedor/modelo`) cambia el modelo del panel y del resto (estado compartido M1).',
    categoria: 'modelo',
    admiteArgumentos: true,
    usoArgumentos: '[id]',
    origen: 'integrado',
  },
  {
    nombre: 'compactar',
    resumen: 'resume el contexto ahora (no espera al umbral)',
    detalle:
      'Fuerza la compactación del contexto del panel. Respeta el gancho previo a la compactación y avisa si no hay material que resumir.',
    categoria: 'contexto',
    admiteArgumentos: true,
    usoArgumentos: '[instrucción]',
    origen: 'integrado',
  },
  {
    nombre: 'contexto',
    resumen: 'muestra la presión de contexto y el umbral',
    detalle:
      'Muestra la ocupación de la ventana, la ventana máxima configurada y la reserva de salida del panel enfocado.',
    categoria: 'contexto',
    admiteArgumentos: false,
    usoArgumentos: '',
    origen: 'integrado',
  },
  {
    nombre: 'limpiar',
    resumen: 'vacía la conversación de este panel',
    detalle:
      'Deja el panel en borrador local: la conversación actual no se borra del historial, y la nueva fila solo se crea al enviar el primer mensaje (create-on-write).',
    categoria: 'sesion',
    admiteArgumentos: false,
    usoArgumentos: '',
    origen: 'integrado',
  },
  {
    nombre: 'revisar',
    resumen: 'pide una revisión de los cambios sin commitear',
    detalle:
      'Envía un turno con la instrucción de revisar los cambios pendientes del área activa. Los argumentos se añaden como foco de la revisión.',
    categoria: 'proyecto',
    admiteArgumentos: true,
    usoArgumentos: '[foco]',
    origen: 'integrado',
  },
  {
    nombre: 'iniciar',
    resumen: 'prepara o actualiza el AGENTS.md del proyecto',
    detalle:
      'Envía un turno guiado para crear o actualizar el `AGENTS.md` del área activa con las convenciones reales del repositorio.',
    categoria: 'proyecto',
    admiteArgumentos: true,
    usoArgumentos: '[foco]',
    origen: 'integrado',
  },
  {
    nombre: 'meta',
    resumen: 'ejecuta un turno con la meta indicada',
    detalle:
      'Ejecuta UN turno con política de solo lectura usando la meta del texto, sin cambiar el modo global.',
    categoria: 'meta',
    admiteArgumentos: true,
    usoArgumentos: '<texto>',
    origen: 'integrado',
  },
];

/** Comando del proyecto tal y como lo devuelve el backend. */
export interface ComandoProyecto {
  nombre: string;
  descripcion: string;
}

/** Convierte un comando del área (`tipo: comando` en `.glory/comandos`) en
 * entrada de menú. La plantilla la expande el backend al ejecutarlo. */
export function comandoDeProyecto(c: ComandoProyecto): ComandoSlash {
  return {
    nombre: c.nombre,
    resumen: c.descripcion,
    detalle: `Comando del proyecto (\`.glory/comandos/${c.nombre}.md\`): la plantilla se expande con tus argumentos.`,
    categoria: 'proyecto',
    admiteArgumentos: true,
    usoArgumentos: '[argumentos]',
    origen: 'proyecto',
  };
}

/**
 * Filtra por consulta con el algoritmo de la referencia (`grok-cli`
 * `filterSlashMenuItems`): coincidencia exacta primero, luego prefijo, luego
 * "contiene" en el nombre y, por último, en la descripción. Empate → orden
 * alfabético, para que el resultado sea estable entre pulsaciones.
 */
export function filtrarComandos(comandos: ComandoSlash[], consulta: string): ComandoSlash[] {
  const q = consulta.trim().toLowerCase();
  if (!q) return [...comandos];
  const puntuados: Array<{ c: ComandoSlash; p: number }> = [];
  for (const c of comandos) {
    const nombre = c.nombre.toLowerCase();
    const resumen = c.resumen.toLowerCase();
    let p: number | null;
    if (nombre === q) p = 0;
    else if (nombre.startsWith(q)) p = 1;
    else if (nombre.includes(q)) p = 2;
    else if (resumen.includes(q)) p = 3;
    else p = null;
    if (p !== null) puntuados.push({ c, p });
  }
  puntuados.sort((a, b) => a.p - b.p || a.c.nombre.localeCompare(b.c.nombre));
  return puntuados.map((x) => x.c);
}

/** `true` si el texto completo es un comando `/` (con o sin argumentos). */
export function esComando(texto: string): boolean {
  return /^\/\S/.test(texto.trimStart());
}

/**
 * Descompone `/nombre argumentos` en sus partes. `null` si el texto no es un
 * comando. El nombre se normaliza a minúsculas sin la barra.
 */
export function partirComando(texto: string): { nombre: string; argumentos: string } | null {
  const limpio = texto.trim();
  if (!limpio.startsWith('/')) return null;
  const [cabecera, ...resto] = limpio.slice(1).split(/\s+/);
  if (!cabecera) return null;
  return { nombre: cabecera.toLowerCase(), argumentos: resto.join(' ').trim() };
}

/**
 * Consulta del menú para el texto actual del textarea: el trozo tras la `/`
 * inicial SIEMPRE que aún no haya espacio (una vez hay argumentos el menú ya
 * no filtra). Devuelve `null` si no toca mostrar el menú.
 */
export function consultaMenu(texto: string): string | null {
  if (!texto.startsWith('/')) return null;
  const resto = texto.slice(1);
  if (resto.includes(' ') || resto.includes('\n')) return null;
  return resto;
}

/** Busca un comando integrado por nombre exacto. */
export function buscarBuiltin(nombre: string): ComandoSlash | null {
  const n = nombre.toLowerCase();
  return COMANDOS_BUILTIN.find((c) => c.nombre === n) ?? null;
}

/** Línea de uso del comando (`/modelo [id]`). */
export function uso(comando: ComandoSlash): string {
  return `/${comando.nombre}${comando.usoArgumentos ? ` ${comando.usoArgumentos}` : ''}`;
}

/** Catálogo agrupado por categoría (para `/ayuda`), en el orden del catálogo. */
export function agruparPorCategoria(
  comandos: ComandoSlash[],
): Array<{ categoria: CategoriaComando; comandos: ComandoSlash[] }> {
  const orden: CategoriaComando[] = [
    'ayuda',
    'sesion',
    'modelo',
    'contexto',
    'proyecto',
    'meta',
  ];
  return orden
    .map((categoria) => ({
      categoria,
      comandos: comandos.filter((c) => c.categoria === categoria),
    }))
    .filter((g) => g.comandos.length > 0);
}

/** Texto plano del catálogo (lo que muestran `/ayuda` y el aviso del panel). */
export function catalogoTexto(comandos: ComandoSlash[]): string {
  const lineas: string[] = [];
  for (const grupo of agruparPorCategoria(comandos)) {
    lineas.push(`${ETIQUETA_CATEGORIA[grupo.categoria]}:`);
    for (const c of grupo.comandos) {
      lineas.push(`  ${uso(c)} — ${c.resumen}`);
    }
  }
  return lineas.join('\n');
}
