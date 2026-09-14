import { el } from './dom';

/** [139A-6] Sección colapsable compartida por los repos (`panelCambios`) y
 * el vault (`vaultCambios`): la MISMA estructura y aspecto en ambos —
 * plana como el resto del diseño (los divisores los pone `git.css`), sin
 * cajas. Nace minimizada; cada dueño conserva el gesto del usuario entre
 * pintados como prefiera (los repos reutilizan wraps; el vault lo guarda
 * en su clausura porque repinta).
 *
 * [Un solo repo] Con `fijarFijo(true)` la sección queda expandida y sin
 * gesto de colapso (minimizar un único repo no ahorra nada): se oculta el
 * marcador, el click/teclado no pliegan y la cabecera pierde el cursor y
 * el hover de botón. */
export interface SeccionColapsable {
  seccion: HTMLElement;
  cab: HTMLButtonElement;
  titulo: HTMLElement;
  contador: HTMLElement;
  colapsado: boolean;
  fijo: boolean;
  fijarColapso(colapsado: boolean): void;
  fijarFijo(fijo: boolean): void;
}

export function crearSeccionColapsable(): SeccionColapsable {
  const seccion = el('section', 'git-seccion colapsado');
  const cab = el('button', 'git-seccion-cabecera git-seccion-boton');
  cab.type = 'button';
  cab.setAttribute('aria-expanded', 'false');
  const marcador = el('span', 'git-seccion-marcador');
  marcador.textContent = '▸';
  marcador.setAttribute('aria-hidden', 'true');
  const titulo = el('span', 'git-seccion-titulo');
  const contador = el('span', 'git-seccion-contador');
  contador.textContent = '0';
  cab.append(marcador, titulo, contador);
  seccion.appendChild(cab);
  const estado: SeccionColapsable = {
    seccion,
    cab,
    titulo,
    contador,
    colapsado: true,
    fijo: false,
    fijarColapso(colapsado: boolean): void {
      // Fija no se pliega por ningún camino (click, teclado o revelado).
      if (estado.fijo && colapsado) return;
      estado.colapsado = colapsado;
      seccion.classList.toggle('colapsado', colapsado);
      cab.setAttribute('aria-expanded', String(!colapsado));
      marcador.textContent = colapsado ? '▸' : '▾';
    },
    fijarFijo(fijo: boolean): void {
      estado.fijo = fijo;
      seccion.classList.toggle('git-seccion-fija', fijo);
      marcador.hidden = fijo;
      // Sin gesto no hay foco de teclado que llevar a ninguna parte.
      cab.tabIndex = fijo ? -1 : 0;
      if (fijo) estado.fijarColapso(false);
    },
  };
  cab.addEventListener('click', () => estado.fijarColapso(!estado.colapsado));
  return estado;
}
