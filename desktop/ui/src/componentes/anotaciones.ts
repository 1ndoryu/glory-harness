// ============================================================
// Anotaciones sobre el navegador (plan 069A-1, F4).
//
// [Riesgo documentado] El HWND nativo de la webview child
// (Windows) está en una capa superior que un canvas position:
// absolute NO puede cubrir. Por eso las anotaciones funcionan
// sobre la CAPTURA (imagen PNG) y no en vivo sobre la webview.
// El resaltado CDP momentáneo (via DevTools) es la alternativa
// para señalar elementos sin overlay DOM.
//
// Estrategias disponibles (seleccionar según contexto):
//   a) Anotaciones sobre captura (imagen) — implementado.
//   b) Resaltado CDP momentáneo mediante
//      `Runtime.evaluate` con highlight/outline — implementado.
//   c) Capa via SetHostObject+JS (futuro).
// ============================================================

import { invoke } from '@tauri-apps/api/core';

// ---------- Tipos ----------

/** Una anotación sobre la captura (rectángulo + etiqueta). */
export interface Anotacion {
  /** Selector CSS del elemento (para resaltado CDP). */
  selector: string;
  /** Etiqueta breve (ej: "input", "botón"). */
  etiqueta: string;
  /** Color del borde (CSS). */
  color?: string;
  /** Coordenadas relativas a la imagen (0-1 en x,y,w,h). */
  rect?: { x: number; y: number; w: number; h: number };
}

/** API pública del módulo de anotaciones. */
export interface AnotacionesUI {
  canvas: HTMLCanvasElement;
  /** Dibuja las anotaciones sobre el canvas (limpia primero). */
  dibujar(anotaciones: Anotacion[], imagenNaturalW: number, imagenNaturalH: number): void;
  /** Limpia el canvas. */
  limpiar(): void;
}

// ---------- Constantes ----------
const COLOR_DEFAULT = '#ff0000';

// ---------- Fábrica ----------

export function crearAnotacionesUI(ancho = 800, alto = 600): AnotacionesUI {
  const canvas = document.createElement('canvas');
  canvas.width = ancho;
  canvas.height = alto;
  canvas.className = 'nav-anotaciones';
  canvas.style.width = '100%';
  canvas.style.height = 'auto';
  canvas.style.aspectRatio = `${ancho} / ${alto}`;
  canvas.style.display = 'none'; // oculto hasta que haya anotaciones

  const ctx = canvas.getContext('2d');
  if (!ctx) throw new Error('no se pudo crear el contexto 2D del canvas');

  function dibujar(anotaciones: Anotacion[], imgW: number, imgH: number): void {
    canvas.width = imgW;
    canvas.height = imgH;
    canvas.style.aspectRatio = `${imgW} / ${imgH}`;
    canvas.style.display = '';

    if (!ctx) return;
    ctx.clearRect(0, 0, imgW, imgH);

    for (const a of anotaciones) {
      if (!a.rect) continue;
      const { x, y, w, h } = a.rect;
      const color = a.color ?? COLOR_DEFAULT;

      // Rectángulo semitransparente
      ctx.strokeStyle = color;
      ctx.lineWidth = 2;
      ctx.strokeRect(x * imgW, y * imgH, w * imgW, h * imgH);

      // Etiqueta
      ctx.fillStyle = color;
      ctx.font = `12px monospace`;
      ctx.fillText(a.etiqueta, x * imgW + 4, y * imgH - 4);
    }
  }

  function limpiar(): void {
    if (!ctx) return;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    canvas.style.display = 'none';
  }

  return { canvas, dibujar, limpiar };
}

// ---------- Helpers de resaltado CDP ----------

/**
 * Resalta momentáneamente un elemento vía CDP Runtime.evaluate.
 * Inyecta un outline rojo durante `duracionMs` y lo quita.
 */
export async function resaltarElementoCDP(
  selector: string,
  duracionMs = 2000,
): Promise<void> {
  try {
    await invoke('navegador_js', {
      codigo: `(function(){
        var e=document.querySelector(${JSON.stringify(selector)});
        if(!e)return;
        e.style.outline='2px solid red';
        e.style.outlineOffset='-1px';
        setTimeout(function(){e.style.outline='';e.style.outlineOffset=''},${duracionMs});
      })()`,
    });
  } catch {
    // silencioso: el resaltado es una mejora visual, no crítica
  }
}

/**
 * Resalta múltiples elementos en secuencia.
 */
export async function resaltarVariosCDP(
  anotaciones: Anotacion[],
  duracionMs = 2000,
): Promise<void> {
  for (let i = 0; i < anotaciones.length; i++) {
    await resaltarElementoCDP(anotaciones[i].selector, duracionMs);
    // Pequeña pausa entre resaltados
    if (i < anotaciones.length - 1) {
      await new Promise((r) => setTimeout(r, 300));
    }
  }
}