// ============================================================
// Panel meta REAL (plan 039A-1 §10.5.2): sustituye al boceto 03-09.
// Misma fila única (meta + estado · tiempo · tokens + play/pausa) y
// mismo montaje DENTRO de #entrada, pero sin simulación: la meta la
// fija el usuario y viaja al backend (`actualizar_meta`); el tiempo
// mide el turno en curso y los tokens son los del núcleo (`usage`).
// La pausa detiene el turno (`cancelar_turno`); reanudar reenvía el
// último mensaje como turno nuevo (el abortado no se puede continuar).
// ============================================================

import { el } from '../util/dom';

/** Estado visible: inactivo (sin turno) · corriendo · pausado. */
export type EstadoMeta = 'inactivo' | 'corriendo' | 'pausado';

export interface PanelMeta {
  raiz: HTMLElement;
  /** Recalcula la altura del campo meta (llamar tras montar al DOM). */
  medir(): void;
  setEstado(estado: EstadoMeta): void;
  setTiempo(segundos: number): void;
  setTokens(n: number): void;
  setMeta(texto: string): void;
  getMeta(): string;
  /** [039A-1 04-09 H1] Muestra/oculta el panel (modo meta o hay meta). */
  mostrar(visible: boolean): void;
  /** ¿Está visible el panel? */
  visible(): boolean;
}

export interface PanelMetaOpciones {
  /** El usuario terminó de editar la meta (blur o Ctrl+Enter). */
  onMetaCambiada: (meta: string) => void;
  /** Pausar el turno en curso. */
  onPausar: () => void;
  /** Reanudar: reenvía el último mensaje como turno nuevo. */
  onReanudar: () => void;
}

const ICONO_PAUSA =
  '<svg class="ic ic-xs" viewBox="0 0 24 24" aria-hidden="true"><rect x="6" y="4" width="4" height="16"></rect><rect x="14" y="4" width="4" height="16"></rect></svg>';
const ICONO_PLAY =
  '<svg class="ic ic-xs" viewBox="0 0 24 24" aria-hidden="true"><polygon points="6 3 20 12 6 21 6 3"></polygon></svg>';

const ETIQUETA_ESTADO: Record<EstadoMeta, string> = {
  inactivo: 'inactivo',
  corriendo: 'corriendo',
  pausado: 'pausado',
};

export function montarPanelMeta(opts: PanelMetaOpciones): PanelMeta {
  const raiz = el('div', 'panel-meta inactivo');

  // ---- fila única: meta (izquierda, resto del ancho) + info (derecha) ----
  const fila = el('div', 'pm-fila');

  const meta = el('textarea', 'pm-meta') as HTMLTextAreaElement;
  meta.id = 'pm-meta';
  meta.rows = 1;
  meta.spellcheck = false;
  meta.placeholder = 'meta…';
  meta.value = '';
  const campoMeta = el('label', 'pm-campo');
  campoMeta.appendChild(meta);

  const info = el('span', 'pm-info');

  const estado = el('span', 'pm-estado');
  const punto = el('span', 'punto');
  const txtEstado = el('span');
  txtEstado.id = 'pm-estado-texto';
  txtEstado.textContent = 'inactivo';
  estado.appendChild(punto);
  estado.appendChild(txtEstado);

  const tiempo = el('span', 'pm-metrica');
  const txtTiempo = el('span');
  txtTiempo.id = 'pm-tiempo';
  txtTiempo.textContent = '00:00';
  tiempo.appendChild(el('span')).textContent = '⏱';
  tiempo.appendChild(txtTiempo);

  const tokens = el('span', 'pm-metrica');
  const txtTokens = el('span');
  txtTokens.id = 'pm-tokens';
  txtTokens.textContent = '0';
  tokens.appendChild(el('span')).textContent = '≈';
  tokens.appendChild(txtTokens);
  tokens.appendChild(el('span')).textContent = 'tok';

  info.appendChild(estado);
  info.appendChild(tiempo);
  info.appendChild(tokens);

  const btnPlay = el('button', 'pm-play') as HTMLButtonElement;
  btnPlay.id = 'pm-play';
  btnPlay.type = 'button';

  fila.appendChild(campoMeta);
  fila.appendChild(info);
  fila.appendChild(btnPlay);
  raiz.appendChild(fila);

  let estadoActual: EstadoMeta = 'inactivo';
  let oculto = false;

  /** [039A-1 04-09 H1] Aplica la clase `.oculto` (display:none) sin colisión
   * con las clases de estado (corriendo/pausado/inactivo) ni margin colgando. */
  function pintarVisible(): void {
    raiz.classList.toggle('oculto', oculto);
    // El panel oculto no debe dejar el hueco del margin-bottom en #entrada.
    raiz.style.marginBottom = oculto ? '0' : '';
  }

  function pintar(): void {
    txtEstado.textContent = ETIQUETA_ESTADO[estadoActual];
    raiz.classList.toggle('corriendo', estadoActual === 'corriendo');
    raiz.classList.toggle('pausado', estadoActual === 'pausado');
    raiz.classList.toggle('inactivo', estadoActual === 'inactivo');
    const esPausa = estadoActual === 'corriendo';
    btnPlay.innerHTML = esPausa ? ICONO_PAUSA : ICONO_PLAY;
    btnPlay.setAttribute('aria-label', esPausa ? 'pausar' : 'reanudar');
    btnPlay.title = esPausa ? 'pausar' : 'reanudar';
  }

  btnPlay.addEventListener('click', () => {
    if (estadoActual === 'corriendo') opts.onPausar();
    else opts.onReanudar();
  });

  // ---- expansión: colapsado 1 línea → al enfocar crece hasta 3 líneas ----
  function pintarAltura(): void {
    if (raiz.classList.contains('expandido')) {
      meta.style.height = 'auto';
      const lh = parseFloat(getComputedStyle(meta).lineHeight) || 18;
      const max = lh * 3;
      if (meta.scrollHeight > max) {
        meta.style.height = max + 'px';
        meta.style.overflowY = 'auto';
      } else {
        meta.style.height = meta.scrollHeight + 'px';
        meta.style.overflowY = 'hidden';
      }
    } else {
      meta.style.height = '';
      meta.style.overflowY = 'hidden';
    }
  }
  meta.addEventListener('input', pintarAltura);
  meta.addEventListener('focus', () => {
    raiz.classList.add('expandido');
    pintarAltura();
  });
  function confirmarEdicion(): void {
    raiz.classList.remove('expandido');
    pintarAltura();
    opts.onMetaCambiada(meta.value);
  }
  meta.addEventListener('blur', confirmarEdicion);
  meta.addEventListener('keydown', (e) => {
    e.stopPropagation();
    if (e.key === 'Enter' && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      meta.blur();
    }
  });

  pintar();
  pintarVisible();

  return {
    raiz,
    medir() {
      pintarAltura();
    },
    setEstado(e: EstadoMeta) {
      estadoActual = e;
      pintar();
    },
    setTiempo(segundos: number) {
      const s = Math.max(0, Math.floor(segundos));
      txtTiempo.textContent = `${String(Math.floor(s / 60)).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
    },
    setTokens(n: number) {
      txtTokens.textContent = Math.max(0, Math.round(n)).toLocaleString('es');
    },
    setMeta(texto: string) {
      if (meta.value !== texto) {
        meta.value = texto;
        pintarAltura();
      }
    },
    getMeta() {
      return meta.value;
    },
    mostrar(v: boolean) {
      oculto = !v;
      pintarVisible();
    },
    visible() {
      return !oculto;
    },
  };
}
