/* Descripción e icono de tools + helpers de argumentos/compactado.
 * Presentación pura: sin DOM ni IPC. */
import type { IconoNombre } from '../dominio/tipos';

type ArgumentosTool = Record<string, unknown>;

function argumentosDeTool(argumentos: unknown): ArgumentosTool | null {
  return argumentos && typeof argumentos === 'object' && !Array.isArray(argumentos)
    ? argumentos as ArgumentosTool
    : null;
}

export function rutaDeArgumentos(argumentos: unknown): string | null {
  const objeto = argumentosDeTool(argumentos);
  const ruta = objeto?.ruta;
  return typeof ruta === 'string' && ruta.trim() ? ruta : null;
}

function textoDeArgumento(argumentos: unknown, clave: string, max = 72): string | null {
  const valor = argumentosDeTool(argumentos)?.[clave];
  if (typeof valor !== 'string' || !valor.trim()) return null;
  const texto = valor.trim().replace(/\s+/g, ' ');
  return texto.length > max ? `${texto.slice(0, max - 1)}…` : texto;
}

function rangoDeLectura(argumentos: unknown): string {
  const objeto = argumentosDeTool(argumentos);
  const inicio = objeto?.offset_linea;
  const limite = objeto?.limite_lineas;
  if (
    typeof inicio === 'number' && Number.isInteger(inicio) && inicio >= 1 &&
    typeof limite === 'number' && Number.isInteger(limite) && limite >= 1
  ) {
    return `líneas ${inicio}-${inicio + limite - 1}`;
  }
  if (typeof inicio === 'number' && Number.isInteger(inicio) && inicio >= 1) {
    return `desde línea ${inicio}`;
  }
  return '';
}

export function descripcionDeTool(tool: string, argumentos?: unknown): string {
  const ruta = rutaDeArgumentos(argumentos);
  switch (tool) {
    case 'file_write':
      return ruta ? `Modificando ${ruta} · archivo completo` : 'Modificando archivo completo';
    case 'file_patch':
      return ruta ? `Modificando ${ruta} · líneas modificadas` : 'Modificando archivo · líneas modificadas';
    case 'file_read': {
      const rango = rangoDeLectura(argumentos);
      return ruta ? `Leyendo ${ruta}${rango ? ` · ${rango}` : ''}` : `Leyendo archivo${rango ? ` · ${rango}` : ''}`;
    }
    case 'file_search': {
      const patron = textoDeArgumento(argumentos, 'patron');
      return patron ? `Buscando archivos: ${patron}` : 'Buscando archivos';
    }
    case 'web_search': {
      const consulta = textoDeArgumento(argumentos, 'query');
      return consulta ? `Buscando en la web: ${consulta}` : 'Buscando en la web';
    }
    case 'web_fetch': {
      const url = textoDeArgumento(argumentos, 'url');
      return url ? `Leyendo web: ${url}` : 'Leyendo página web';
    }
    case 'comando':
      return 'Ejecutando comando';
    case 'comando_status':
      return 'Consultando comando';
    case 'comando_matar':
      return 'Deteniendo comando';
    case 'navegador_reflejo':
      return 'Usando navegador';
    case 'task':
      return 'Ejecutando tarea';
    case 'todo':
      return 'Actualizando tareas';
    default:
      return `Ejecutando ${tool.replaceAll('_', ' ')}`;
  }
}

export function iconoDeTool(tool: string): IconoNombre {
  if (tool.startsWith('file_')) return 'archivo';
  if (tool === 'web_search') return 'globo';
  if (tool.startsWith('comando')) return 'terminal';
  if (tool === 'task' || tool === 'todo') return 'flujo';
  return 'lupa';
}

export function compacto(valor: unknown, max = 500): string {
  const s = typeof valor === 'string' ? valor : JSON.stringify(valor);
  return s.length > max ? s.slice(0, max) + '…' : s;
}
