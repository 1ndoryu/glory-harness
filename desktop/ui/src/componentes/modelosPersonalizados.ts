/* [129A-9 bloque Modelos] Editor de modelos personalizados del modal.
 *
 * Réplica adaptada de Synara `ModelsSettingsPanel` (sección "Custom models"
 * + `validateCustomModelInput`). GH ya elige modelo con `selectorModelo`
 * (menú doble proveedor → modelo); lo nuevo es guardar slugs propios por
 * proveedor, que se fusionan al catálogo vivo y aparecen en ese menú.
 * Guardado automático al añadir/quitar (igual que el resto de Ajustes).
 * El esquema declarativo de `dominio/opciones.ts` no admite listas gestoras,
 * así que es custom y se monta como sección sin grupos (igual que
 * "Memorias" y "Proveedores").
 */

import '../estilos/modelosPersonalizados.css';

import type { ProveedorModelo } from '../dominio/tipos';
import {
  fusionarEnSitio,
  retirarDeSitio,
  validarModeloPersonalizado,
  type ModeloPersonalizado,
} from '../dominio/modelosPersonalizados';
import { el, vaciar } from '../util/dom';

/** Deps del panel (el aviso lo aporta el modal desde `avisar`). */
export interface ModelosPersonalizadosDeps {
  /** Catálogo vivo compartido (etiquetas + modelos; se fusiona in place). */
  catalogo: ProveedorModelo[];
  /** Lista inicial (vacía: el arranque real la restaura vía `fijarLista`). */
  iniciales: ModeloPersonalizado[];
  /** Persiste la lista (config `modelos_personalizados`; mock = no-op). */
  guardar(lista: ModeloPersonalizado[]): Promise<void>;
  /** Aviso global del orquestador (toast + registro). */
  avisar(texto: string, meta: string, detalle: string): void;
}

export interface ModelosPersonalizadosPanel {
  raiz: HTMLElement;
  /** Reemplaza la lista (restaurar del backend) sin volver a guardar. */
  fijarLista(lista: ModeloPersonalizado[]): void;
}

export function montarModelosPersonalizados(
  deps: ModelosPersonalizadosDeps,
): ModelosPersonalizadosPanel {
  const raiz = el('div', 'mp');

  const titulo = el('div', 'mp-titulo');
  titulo.textContent = 'Modelos personalizados';
  const cabecera = el('div', 'mp-cabecera');
  cabecera.appendChild(titulo);

  const forma = el('div', 'mp-forma');
  const selProv = el('select', 'select') as HTMLSelectElement;
  selProv.setAttribute('aria-label', 'proveedor del modelo personalizado');
  const entrada = el('input', 'input-texto') as HTMLInputElement;
  entrada.type = 'text';
  entrada.placeholder = 'slug del modelo (p. ej. poolside/otro-modelo)';
  entrada.spellcheck = false;
  entrada.setAttribute('aria-label', 'slug del modelo personalizado');
  const btnAnadir = el('button', 'mp-anadir') as HTMLButtonElement;
  btnAnadir.type = 'button';
  btnAnadir.textContent = 'añadir';
  forma.append(selProv, entrada, btnAnadir);

  const error = el('div', 'mp-error');
  const lista = el('div', 'mp-lista');
  const nota = el('div', 'mp-nota');
  nota.textContent =
    'los slugs se añaden al menú de modelo de su proveedor y se guardan ' +
    'en la configuración (clave `modelos_personalizados`).';

  raiz.append(cabecera, forma, error, lista, nota);

  let guardados: ModeloPersonalizado[] = [...deps.iniciales];
  // Foto de los slugs integrados al montar (antes de fusionar): `quitar`
  // solo retira del catálogo lo que la fusión añadió, nunca la base.
  const baseIntegrados = new Map<string, Set<string>>(
    deps.catalogo.map((p) => [p.id, new Set(p.modelos.map((m) => m.modelo))]),
  );

  function etiquetaProveedor(id: string): string {
    return deps.catalogo.find((p) => p.id === id)?.etiqueta ?? id;
  }

  function pintarProveedores(): void {
    vaciar(selProv);
    deps.catalogo.forEach((p) => {
      const opt = el('option') as HTMLOptionElement;
      opt.value = p.id;
      opt.textContent = p.etiqueta;
      selProv.appendChild(opt);
    });
    if (deps.catalogo.length === 0) {
      const opt = el('option') as HTMLOptionElement;
      opt.value = '';
      opt.textContent = 'sin proveedores';
      selProv.appendChild(opt);
    }
  }

  function pintarError(mensaje: string): void {
    vaciar(error);
    if (!mensaje) return;
    const linea = el('div', 'mp-linea-error');
    linea.textContent = mensaje;
    error.appendChild(linea);
  }

  function pintarLista(): void {
    vaciar(lista);
    if (guardados.length === 0) {
      const vacio = el('div', 'mp-vacio');
      vacio.textContent = 'sin modelos personalizados.';
      lista.appendChild(vacio);
      return;
    }
    guardados.forEach((m) => {
      const filaEl = el('div', 'mp-fila');
      const prov = el('span', 'mp-prov');
      prov.textContent = etiquetaProveedor(m.proveedor);
      const slug = el('code', 'mp-slug');
      slug.textContent = m.modelo;
      const quitar = el('button', 'mp-quitar') as HTMLButtonElement;
      quitar.type = 'button';
      quitar.textContent = '×';
      quitar.title = `quitar ${m.modelo}`;
      quitar.setAttribute('aria-label', `quitar ${m.modelo}`);
      quitar.addEventListener('click', () => quitarModelo(m.proveedor, m.modelo));
      filaEl.append(prov, slug, quitar);
      lista.appendChild(filaEl);
    });
  }

  function pintar(): void {
    pintarProveedores();
    pintarLista();
  }

  /** Persiste best-effort (el aviso ya explica; la lista en memoria manda). */
  function persistir(nueva: ModeloPersonalizado[]): void {
    void deps
      .guardar(nueva)
      .catch((e: unknown) =>
        deps.avisar(`no se pudo guardar modelos personalizados: ${String(e)}`, '', ''),
      );
  }

  function aplicar(nueva: ModeloPersonalizado[]): void {
    guardados = nueva;
    fusionarEnSitio(deps.catalogo, guardados);
    pintar();
  }

  function anadir(): void {
    const resultado = validarModeloPersonalizado({
      catalogo: deps.catalogo,
      guardados,
      proveedor: selProv.value,
      valor: entrada.value,
    });
    if (resultado.error !== undefined) {
      pintarError(resultado.error);
      return;
    }
    pintarError('');
    entrada.value = '';
    aplicar([...guardados, resultado.modelo]);
    persistir(guardados);
  }

  function quitarModelo(proveedor: string, slug: string): void {
    retirarDeSitio(deps.catalogo, baseIntegrados, proveedor, slug);
    aplicar(guardados.filter((m) => !(m.proveedor === proveedor && m.modelo === slug)));
    persistir(guardados);
  }

  btnAnadir.addEventListener('click', anadir);
  entrada.addEventListener('keydown', (e) => {
    if (e.key !== 'Enter') return;
    e.preventDefault();
    anadir();
  });
  // Escribir limpia el error (precedente: el input custom de Synara).
  entrada.addEventListener('input', () => {
    if (error.firstChild) pintarError('');
  });

  pintar();

  return {
    raiz,
    fijarLista(nueva: ModeloPersonalizado[]) {
      // Restaurar del backend: fusiona y pinta sin guardar (ya persistido).
      aplicar([...nueva]);
    },
  };
}
