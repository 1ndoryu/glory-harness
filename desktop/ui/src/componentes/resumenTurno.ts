/* [129A-8] Resumen de cambios al cerrar el turno (estilo Synara/VSCode):
 * cabecera única + filas plegables por archivo. Determinista desde eventos
 * (nada redactado por el modelo): el llamador aporta los cambios y el
 * callback que abre el archivo en la tab Files. Sin cambios → `null`
 * (el turno sin cambios no muestra bloque, como en Synara).
 *
 * [20-09-2026] Sin secciones intermedias: la cabecera resume el total
 * (`Cambios -N +M` con el conteo de todos los archivos) y cada fila es un
 * `details` con la misma fila `git-entrada` del panel Git (estado + nombre
 * + stats + botón Files); al abrir muestra el mismo resumen del cambio que
 * las tarjetas de herramienta. Reutiliza clases `git-*` y el formateo de
 * resultados: nada nuevo que aprender ni que mantener dos veces. */
import { el } from '../util/dom';
import { icono } from './iconos';
import '../estilos/git.css';
import type { CambioArchivoPanel } from './panelChatTipos';
import {
  contarDiff,
  formatearResultadoHerramienta,
  ponerHtmlSeguro,
} from './mensajesUtil';

function nombreDeRuta(ruta: string): string {
  const i = Math.max(ruta.lastIndexOf('/'), ruta.lastIndexOf('\\'));
  return i < 0 ? ruta : ruta.slice(i + 1);
}

/** Letra de estado como en Git: `A` = creado (`file_write`), `M` = parche. */
function codigoCambio(c: CambioArchivoPanel): string {
  return c.tool === 'file_write' ? 'A' : 'M';
}

export function crearResumenTurno(
  cambios: CambioArchivoPanel[],
  alAbrir: (ruta: string) => void,
): HTMLElement | null {
  if (cambios.length === 0) return null;
  let adiciones = 0;
  let eliminaciones = 0;
  for (const c of cambios) {
    const conteo = contarDiff(c.diff);
    adiciones += conteo.añadidas;
    eliminaciones += conteo.eliminadas;
  }

  const raiz = el('div', 'resumen-turno');
  const cab = el('div', 'resumen-turno-cabecera');
  const titulo = el('span', 'resumen-turno-titulo');
  titulo.textContent = 'Cambios';
  cab.appendChild(titulo);
  /* Totales de todos los archivos (borradas y luego agregadas, como el
   * contador de las tarjetas): solo aparecen los lados con cambios. */
  if (eliminaciones > 0) {
    cab.appendChild(document.createTextNode(' '));
    const del = el('span', 'rotulo-del');
    del.textContent = `-${eliminaciones}`;
    cab.appendChild(del);
  }
  if (adiciones > 0) {
    cab.appendChild(document.createTextNode(' '));
    const add = el('span', 'rotulo-add');
    add.textContent = `+${adiciones}`;
    cab.appendChild(add);
  }
  raiz.appendChild(cab);

  const filas = el('div', 'resumen-turno-lista');
  filas.setAttribute('role', 'list');
  for (const c of cambios) filas.appendChild(crearFila(c, alAbrir));
  raiz.appendChild(filas);
  return raiz;
}

/** Archivo plegable: la fila resume (estado + nombre + stats + botón Files)
 * y al abrir muestra el mismo resumen del cambio que las tarjetas de
 * herramienta (`resumen` + rótulo `-N +M` + líneas). */
function crearFila(c: CambioArchivoPanel, alAbrir: (ruta: string) => void): HTMLElement {
  const detalle = el('details', 'resumen-turno-archivo');
  const fila = el('summary', 'git-entrada');
  fila.setAttribute('role', 'listitem');
  fila.title = c.titulo || c.ruta;
  const codigo = el('span', 'git-codigo');
  codigo.textContent = codigoCambio(c);
  const ruta = el('span', 'git-ruta');
  ruta.textContent = nombreDeRuta(c.ruta);
  ruta.title = c.ruta;
  const conteo = contarDiff(c.diff);
  const statArchivo = el('span', 'git-entrada-estadistica');
  statArchivo.append(
    crearStat('+', conteo.añadidas, 'git-adiciones'),
    crearStat('−', conteo.eliminadas, 'git-eliminaciones'),
  );
  /* Icono junto a las stats (no abre el plegable: preventDefault, como el
   * botón de acción de los avisos de sistema). Icono `abrir`, el mismo que
   * el botón "abrir con" del panel Files. */
  const abrir = el('button', 'resumen-turno-abrir') as HTMLButtonElement;
  abrir.type = 'button';
  abrir.appendChild(icono('abrir', true));
  abrir.title = `abrir ${c.ruta} en la tab Files`;
  abrir.setAttribute('aria-label', `abrir ${c.ruta} en la tab Files`);
  abrir.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    alAbrir(c.ruta);
  });
  fila.append(codigo, ruta, statArchivo, abrir);
  detalle.appendChild(fila);

  const cuerpo = el('div', 'resumen-turno-detalle');
  /* Reutiliza el cuadro de resultado de herramienta (mismo markup, mismos
   * estilos y colores por tema): es el mismo contenido que su tarjeta. */
  const cuadro = el('div', 'herramienta');
  const resultado = el('div', 'resultado');
  ponerHtmlSeguro(resultado, formatearResultadoHerramienta(c.resumen, c.diff));
  cuadro.appendChild(resultado);
  cuerpo.appendChild(cuadro);
  detalle.appendChild(cuerpo);
  return detalle;
}

function crearStat(marca: string, cantidad: number, clase: string): HTMLElement {
  const nodo = el('span', clase);
  nodo.textContent = `${marca}${cantidad}`;
  return nodo;
}
