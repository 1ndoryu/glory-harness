/* [129A-9 bloque Proveedores] Estado de proveedores del modal de ajustes.
 *
 * Réplica adaptada de Synara `ProvidersSettingsPanel` (lista de visibilidad
 * + estado por proveedor + enlaces a docs). GH no tiene install-settings en
 * ajustes (las claves viven en `~/.glory-harness.env`), ni orden arrastrable
 * ni checks de actualización: el panel es una lista de ESTADO (disponible
 * según nº de claves + marca del proveedor en uso) con recarga. El esquema
 * declarativo de `dominio/opciones.ts` no admite listas gestoras, así que es
 * custom y se monta como sección sin grupos (igual que "Memorias").
 */

import '../estilos/proveedores.css';

import type { ModeloSeleccionado, ProveedorModelo } from '../dominio/tipos';
import type { ProveedorInfo } from '../tauri/real';
import { el, vaciar } from '../util/dom';

/** Deps del panel (el aviso lo aporta el modal desde `avisar`). */
export interface ProveedoresDeps {
  /** Catálogo estático (etiquetas + modelos) para pintar sin backend. */
  catalogo: ProveedorModelo[];
  /** Estado vivo del backend (`adaptador.sesion.proveedores`). */
  listar(): Promise<ProveedorInfo[]>;
  /** Modelo activo ahora mismo (marca "en uso"). */
  modeloActual(): ModeloSeleccionado;
  /** Aviso global del orquestador (toast + registro). */
  avisar(texto: string, meta: string, detalle: string): void;
}

export interface ProveedoresPanel {
  raiz: HTMLElement;
  /** Recarga desde el backend (al abrir el modal o la sección). */
  refrescar(): void;
}

export function montarProveedores(deps: ProveedoresDeps): ProveedoresPanel {
  const raiz = el('div', 'proveedores');

  const titulo = el('div', 'proveedores-titulo');
  titulo.textContent = 'Proveedores';
  const subtitulo = el('div', 'proveedores-subtitulo');
  const identidad = el('div', 'proveedores-identidad');
  identidad.append(titulo, subtitulo);

  const acciones = el('div', 'proveedores-acciones');
  const cabecera = el('div', 'proveedores-cabecera');
  cabecera.append(identidad, acciones);

  const lista = el('div', 'proveedores-lista');
  const estado = el('div', 'proveedores-estado');
  const nota = el('div', 'proveedores-nota');
  // Adaptación documentada: Synara edita API keys/OAuth en ajustes; GH las
  // lee de `~/.glory-harness.env`, así que aquí no se editan ni se resetean.
  nota.textContent =
    'las claves se configuran en ~/.glory-harness.env (aquí no se editan): ' +
    'un proveedor sin claves figura como no disponible hasta recargar el backend.';

  raiz.append(cabecera, estado, lista, nota);

  let ocupado = false;

  function textoEstado(texto: string, clase = ''): void {
    vaciar(estado);
    if (!texto) return;
    const linea = el('div', clase ? `proveedores-linea ${clase}` : 'proveedores-linea');
    linea.textContent = texto;
    estado.appendChild(linea);
  }

  function fila(
    cat: ProveedorModelo,
    vivo: Map<string, ProveedorInfo> | null,
    enUso: string,
  ): HTMLElement {
    const contenedor = el('div', 'proveedor-fila');
    const nombre = el('span', 'proveedor-nombre');
    nombre.textContent = cat.etiqueta;
    const modelos = el('span', 'proveedor-modelos');
    modelos.textContent = `${cat.modelos.length} modelo(s)`;
    const distintivo = el('span', 'proveedores-chip');
    const info = vivo?.get(cat.id);
    if (!vivo) {
      distintivo.textContent = 'sin estado';
    } else if (info && info.claves > 0) {
      distintivo.textContent = `disponible · ${info.claves} clave(s)`;
    } else {
      distintivo.textContent = 'sin claves';
      distintivo.classList.add('vacio');
    }
    contenedor.append(nombre, modelos, distintivo);
    if (cat.id === enUso) {
      const marca = el('span', 'proveedores-chip en-uso');
      marca.textContent = 'en uso';
      contenedor.appendChild(marca);
    }
    return contenedor;
  }

  function pintar(vivo: ProveedorInfo[] | null, motivo: string | null): void {
    const mapa = vivo === null ? null : new Map(vivo.map((p) => [p.id, p]));
    const enUso = deps.modeloActual().proveedor;
    vaciar(lista);
    deps.catalogo.forEach((cat) => lista.appendChild(fila(cat, mapa, enUso)));
    if (motivo !== null) {
      textoEstado(`estado no disponible (${motivo}): se muestra el catálogo`, 'error');
      return;
    }
    // El vivo puede traer proveedores fuera del catálogo (p. ej. cerebras,
    // groq): el conteo solo cubre filas visibles para no prometer "6 de 4".
    const disponibles = deps.catalogo.filter(
      (cat) => (mapa?.get(cat.id)?.claves ?? 0) > 0,
    ).length;
    textoEstado(`${disponibles} de ${deps.catalogo.length} proveedor(es) disponible(s)`);
  }

  function cargar(): void {
    if (ocupado) return;
    ocupado = true;
    textoEstado('cargando…');
    void deps
      .listar()
      .then((vivo) => pintar(vivo, null))
      .catch((e: unknown) => {
        // Sin backend (modo mock) o transporte caído: catálogo sin estado,
        // con el motivo a la vista (precedente: memorias hace lo mismo).
        const motivo = e instanceof Error ? e.message : String(e);
        pintar(null, motivo);
        deps.avisar(`no se pudo leer el estado de proveedores: ${motivo}`, '', '');
      })
      .finally(() => {
        ocupado = false;
      });
  }

  const recargar = el('button', 'proveedores-accion') as HTMLButtonElement;
  recargar.type = 'button';
  recargar.textContent = 'recargar';
  recargar.addEventListener('click', cargar);
  acciones.appendChild(recargar);

  return { raiz, refrescar: cargar };
}
