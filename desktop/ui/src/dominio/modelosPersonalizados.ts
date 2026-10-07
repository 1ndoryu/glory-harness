// Modelos personalizados por proveedor (réplica adaptada de Synara
// `ModelsSettingsPanel` + `validateCustomModelInput`: solo la parte de
// "Custom models". "Generation defaults" (modelo para commits/PRs) no tiene
// contraparte en GH — GH no genera commits ni PRs — y no se replica.
//
// Persistencia: clave de config `modelos_personalizados` con JSON
// `Array<{proveedor, modelo}>`, por la misma ruta que `tema`/`nombre_perfil`
// (Tauri `config_guardar` genérico, web localStorage). Cero cambios Rust.
//
// Fusión: el catálogo (`PROVEEDORES`) es UN array vivo compartido por
// referencia entre el modal, las entradas y los comandos (`main.ts` pasa la
// misma referencia a vistaModal y a la fábrica de paneles).
// `fusionarEnSitio` lo actualiza IN PLACE (splice): los selectores ya
// montados construyen su menú al abrirlo leyendo esa referencia, así que ven
// los personalizados sin re-montar ni APIs nuevas en la cadena
// panel→entrada→selector. Solo se reemplazan los objetos proveedor de alto
// nivel; los `modelos[]` del estático nunca se mutan.

import type { ProveedorModelo } from './tipos';

export interface ModeloPersonalizado {
  proveedor: string;
  modelo: string;
}

/** Clave de config donde vive el JSON `Array<{proveedor, modelo}>`. */
export const CLAVE_MODELOS_PERSONALIZADOS = 'modelos_personalizados';

/** Tope de slug (Synara usa `MAX_CUSTOM_MODEL_LENGTH`; aquí valor propio). */
export const MAX_SLUG_PERSONALIZADO = 120;

export type ValidacionModelo =
  | { modelo: ModeloPersonalizado; error?: never }
  | { modelo?: never; error: string };

export function normalizarSlug(valor: string): string {
  // Solo trim: los slugs GH llevan `/`, puntos y mayúsculas
  // ('poolside/laguna-s-2.1-free') — no se minusculiza ni se recorta.
  return valor.trim();
}

export function validarModeloPersonalizado(args: {
  catalogo: ProveedorModelo[];
  guardados: ModeloPersonalizado[];
  proveedor: string;
  valor: string;
}): ValidacionModelo {
  const slug = normalizarSlug(args.valor);
  if (!slug) return { error: 'escribe el slug del modelo.' };
  const prov = args.catalogo.find((p) => p.id === args.proveedor);
  if (!prov) return { error: 'proveedor desconocido.' };
  // El duplicado va antes que el integrado: `fusionarEnSitio` añade los
  // guardados al catálogo vivo, así que un slug propio reenviado también
  // cumpliría la condición de abajo y el mensaje mentiría.
  if (args.guardados.some((g) => g.proveedor === args.proveedor && g.modelo === slug)) {
    return { error: 'ese modelo ya está guardado.' };
  }
  if (prov.modelos.some((m) => m.modelo === slug)) {
    return { error: 'ese modelo ya viene integrado.' };
  }
  if (slug.length > MAX_SLUG_PERSONALIZADO) {
    return { error: `el slug admite ${MAX_SLUG_PERSONALIZADO} caracteres o menos.` };
  }
  return { modelo: { proveedor: args.proveedor, modelo: slug } };
}

export function serializarModelos(lista: ModeloPersonalizado[]): string {
  return JSON.stringify(lista.map((m) => ({ proveedor: m.proveedor, modelo: m.modelo })));
}

/** Parseo tolerante del persistido: lo no válido se ignora, nunca revienta. */
export function parsearModelos(raw: string | null): ModeloPersonalizado[] {
  if (!raw) return [];
  try {
    const valor: unknown = JSON.parse(raw);
    if (!Array.isArray(valor)) return [];
    const salida: ModeloPersonalizado[] = [];
    for (const e of valor) {
      if (typeof e !== 'object' || e === null) continue;
      const { proveedor, modelo } = e as { proveedor?: unknown; modelo?: unknown };
      if (typeof proveedor !== 'string' || typeof modelo !== 'string') continue;
      const p = proveedor.trim();
      const m = modelo.trim();
      if (!p || !m) continue;
      if (salida.some((o) => o.proveedor === p && o.modelo === m)) continue;
      salida.push({ proveedor: p, modelo: m });
    }
    return salida;
  } catch {
    return [];
  }
}

/** Fusiona los personalizados en el catálogo vivo (in place; ver cabecera). */
export function fusionarEnSitio(
  catalogo: ProveedorModelo[],
  lista: ModeloPersonalizado[],
): void {
  const porProveedor = new Map<string, string[]>();
  for (const m of lista) {
    // Defensa: el editor solo ofrece proveedores del catálogo, pero el
    // persistido puede traer restos de un catálogo anterior.
    if (!catalogo.some((p) => p.id === m.proveedor)) continue;
    const slugs = porProveedor.get(m.proveedor) ?? [];
    if (!slugs.includes(m.modelo)) slugs.push(m.modelo);
    porProveedor.set(m.proveedor, slugs);
  }
  const fusionado: ProveedorModelo[] = catalogo.map((p) => {
    const extras = porProveedor.get(p.id) ?? [];
    if (extras.length === 0) return p;
    const base = new Set(p.modelos.map((m) => m.modelo));
    const nuevos = extras.filter((s) => !base.has(s));
    if (nuevos.length === 0) return p;
    return { ...p, modelos: [...p.modelos, ...nuevos.map((s) => ({ modelo: s, nombre: s }))] };
  });
  catalogo.splice(0, catalogo.length, ...fusionado);
}

/**
 * Retira un slug del catálogo vivo (in place, mismo patrón que fusionar).
 * Solo toca entradas NO integradas: `base` trae los slugs del estático al
 * montar el panel; un resto persistido que coincida con un integrado se quita
 * de la lista pero nunca del catálogo base.
 */
export function retirarDeSitio(
  catalogo: ProveedorModelo[],
  base: Map<string, Set<string>>,
  proveedor: string,
  slug: string,
): void {
  if (base.get(proveedor)?.has(slug)) return;
  const i = catalogo.findIndex((p) => p.id === proveedor);
  if (i < 0) return;
  const actual = catalogo[i];
  if (!actual.modelos.some((m) => m.modelo === slug)) return;
  catalogo.splice(i, 1, {
    ...actual,
    modelos: actual.modelos.filter((m) => m.modelo !== slug),
  });
}
