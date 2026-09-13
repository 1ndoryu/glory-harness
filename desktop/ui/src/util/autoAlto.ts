/* [139A-4] Textarea autoexpandible con tope de líneas y SIN scrollbar
 * fantasma: el alto nace de una medida (`scrollHeight`), no del diseño, y se
 * publica como `--entrada-alto` (lo aplica `entrada.css`). Con `overflow-y:
 * auto` fijo, cualquier discrepancia subpíxel entre el alto publicado y el
 * contenido mostraba scroll con una sola línea; por eso el overflow se
 * conmuta aquí: `hidden` mientras crece, `auto` solo al llegar al tope. */

export function ajustarTextArea(textarea: HTMLTextAreaElement, maxLineas = 5): void {
  textarea.style.setProperty('--entrada-alto', 'auto');
  const lh = getComputedStyle(textarea).lineHeight;
  const linea = lh === 'normal' ? 18 : parseFloat(lh);
  const max = linea * maxLineas;
  /* +1px de holgura: el line-height fraccional (1.45) deja scrollHeight
     con decimales y publicar el valor exacto recortaba la última línea. */
  const desbordado = textarea.scrollHeight > max;
  const alto = desbordado ? max : textarea.scrollHeight + 1;
  textarea.style.setProperty('--entrada-alto', `${alto}px`);
  textarea.style.overflowY = desbordado ? 'auto' : 'hidden';
}
