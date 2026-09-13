/* [129A-8] Resumen de cambios al cerrar el turno (estilo Synara/VSCode):
 * conteo + lista de archivos con enlace al diff. Determinista desde eventos
 * (nada redactado por el modelo): el llamador aporta los cambios y el
 * callback que abre la tab Cambios en el archivo. Sin cambios → `null`
 * (el turno sin cambios no muestra bloque, como en Synara). */
import { el } from '../util/dom';
import type { CambioArchivoPanel } from './panelChatTipos';

function contar(n: number, uno: string, varios: string): string {
  return n === 1 ? `1 ${uno}` : `${n} ${varios}`;
}

function nombreDeRuta(ruta: string): string {
  const i = Math.max(ruta.lastIndexOf('/'), ruta.lastIndexOf('\\'));
  return i < 0 ? ruta : ruta.slice(i + 1);
}

export function crearResumenTurno(
  cambios: CambioArchivoPanel[],
  alVer: (ruta: string) => void,
): HTMLElement | null {
  if (cambios.length === 0) return null;
  const creados = cambios.filter((c) => c.tool === 'file_write').length;
  const modificados = cambios.length - creados;
  const partes = [contar(cambios.length, 'cambio', 'cambios')];
  if (creados > 0) partes.push(contar(creados, 'creado', 'creados'));
  if (modificados > 0) partes.push(contar(modificados, 'modificado', 'modificados'));

  const raiz = el('div', 'resumen-turno');
  const cab = el('div', 'resumen-turno-cabecera');
  cab.textContent = `${partes[0]}: ${partes.slice(1).join(' · ')}`;
  raiz.appendChild(cab);
  const lista = el('div', 'resumen-turno-lista');
  lista.setAttribute('role', 'list');
  for (const c of cambios) {
    const fila = el('div', 'resumen-turno-fila');
    fila.setAttribute('role', 'listitem');
    const nombre = el('span', 'resumen-turno-ruta');
    nombre.textContent = nombreDeRuta(c.ruta);
    nombre.title = c.ruta;
    const clase = el('span', 'resumen-turno-clase');
    clase.textContent = c.tool === 'file_write' ? 'creado' : 'modificado';
    const ver = el('button', 'resumen-turno-ver') as HTMLButtonElement;
    ver.type = 'button';
    ver.textContent = 'ver en Cambios';
    ver.title = `abrir ${c.ruta} en la tab Cambios`;
    ver.setAttribute('aria-label', `ver ${c.ruta} en Cambios`);
    ver.addEventListener('click', () => alVer(c.ruta));
    fila.append(nombre, clase, ver);
    lista.appendChild(fila);
  }
  raiz.appendChild(lista);
  return raiz;
}
