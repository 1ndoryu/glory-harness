// ============================================================
// Historial de ejemplo de la conversación activa (port 1:1 del
// mockup: estados que el boceto debe revisar). Cuando llegue el
// backend real, esta función se sustituye por el historial
// persistido.
// ============================================================

import type { Bloque } from '../dominio/tipos';

export function historialEjemplo(): Bloque[] {
  return [
    {
      tipo: 'usuario',
      texto: 'extrae construir_harness y procesar_turno a una lib compartida, sin cambiar comportamiento',
    },
    {
      tipo: 'razonamiento',
      texto:
        'El crate cli es binario puro; si muevo la lógica a src/lib.rs, el CLI y la app desktop comparten el mismo código. Necesito exponer construir_harness y procesar_turno como pub y dejar main.rs despachando. Primero localizo todas las referencias para no cambiar comportamiento.',
      meta: '1.6 s',
    },
    {
      tipo: 'asistente',
      texto:
        'Voy a revisar la estructura del crate cli y localizar los módulos que dependen de la lógica de sesión.',
    },
    {
      tipo: 'herramienta',
      icono: 'archivo',
      titulo: 'Se leyó el contenido de cli\\src\\main.rs, run.rs y chat.rs',
      detalle: {
        estado: 'completada',
        meta: 'ok · 3.1 KB · 2 ms',
        resultado: {
          tipo: 'html',
          html:
            'pub(crate) fn construir_harness(opciones: &amp;OpcionesRun) -&gt; HarnessCli {\n' +
            '    let persistencia = Arc::new(PersistenciaMemoria::nuevo());\n' +
            '<span class="del">    let user_id = Uuid::new_v4();</span>\n' +
            '<span class="add">    let user_id = Uuid::new_v4(); // se conserva en la lib</span>\n' +
            '    persistencia.con_skills_base(user_id);',
        },
      },
    },
    {
      tipo: 'herramienta',
      icono: 'lupa',
      titulo: 'Se está buscando "construir_harness" en el crate cli',
      detalle: { estado: 'ejecutando' },
    },
    {
      tipo: 'herramienta',
      icono: 'globo',
      titulo: 'Se buscó en la web y falló: no se pudo contactar el proveedor (503)',
      detalle: {
        estado: 'error',
        meta: '',
        resultado: { tipo: 'texto', texto: 'reintentable: sí · el modelo cambió de plan' },
      },
    },
    {
      tipo: 'asistente',
      texto:
        'El crate cli es binario puro; moveré la sesión a src/lib.rs y dejaré main.rs como despachador fino. El CLI y la app desktop compartirán la misma implementación.',
    },
  ];
}
