// ============================================================
// Utilidades de portapapeles compartidas (plan 039A-3).
// `copiarAlPortapapeles` estaba privada en sidebar.ts; se extrae
// aquí porque el pie de turno (P1), las acciones por mensaje (P2)
// y el menú ⋯ de cabecera (P4) también copian texto.
// ============================================================

import { el } from './dom';

/** Copia texto al portapapeles (fallback a execCommand si no hay API). */
export async function copiarAlPortapapeles(texto: string): Promise<void> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(texto);
      return;
    }
  } catch {
    // fallthrough al fallback
  }
  // fallback para contextos no seguros / permisos denegados
  const ta = el('textarea') as HTMLTextAreaElement;
  ta.value = texto;
  ta.style.position = 'fixed';
  ta.style.opacity = '0';
  document.body.appendChild(ta);
  ta.select();
  document.execCommand?.('copy');
  ta.remove();
}
