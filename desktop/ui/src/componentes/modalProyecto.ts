// ============================================================
// Modal "Nuevo proyecto" (069A-Proyectos): nombre + carpeta.
// NO reutiliza modal.ts (config): es un modal autocontenido con
// botón de guardar explícito. Desktop: picker nativo para la
// carpeta vía `elegir_carpeta_proyecto`. Web: input texto.
// ============================================================

import { esEntornoTauri } from '../tauri/real';
import { icono } from './iconos';
import { el } from '../util/dom';

export interface ModalProyecto {
  raiz: HTMLElement;
  abrir(): void;
  cerrar(): void;
}

export interface ModalProyectoOpciones {
  /** Se invoca al guardar con nombre y ruta válidos. */
  onGuardar: (nombre: string, ruta: string) => void;
  /** En Tauri, invoke('elegir_carpeta_proyecto') devuelve ruta o vacío. */
  invoke?: (comando: string, args?: Record<string, unknown>) => Promise<unknown>;
}

export function montarModalProyecto(opts: ModalProyectoOpciones): ModalProyecto {
  const esTauri = esEntornoTauri();

  const fondo = el('div', 'modal-fondo');
  fondo.id = 'modal-proyecto-fondo';
  fondo.hidden = true;

  const modal = el('div', 'modal');
  modal.setAttribute('role', 'dialog');
  modal.setAttribute('aria-modal', 'true');
  modal.setAttribute('aria-label', 'nuevo proyecto');

  // ---- contenido ----
  const titulo = el('div', 'proyecto-modal-titulo');
  titulo.textContent = 'Nuevo proyecto';
  modal.appendChild(titulo);

  const cuerpo = el('div', 'proyecto-modal-cuerpo');

  // Nombre
  const lblNombre = el('label', 'proyecto-modal-label');
  lblNombre.textContent = 'Nombre';
  const inputNombre = el('input', 'proyecto-modal-input') as HTMLInputElement;
  inputNombre.type = 'text';
  inputNombre.placeholder = 'p. ej. Mi Proyecto';
  lblNombre.appendChild(inputNombre);
  cuerpo.appendChild(lblNombre);

  // Carpeta (ruta)
  const lblRuta = el('label', 'proyecto-modal-label');
  lblRuta.textContent = 'Carpeta';
  const filaRuta = el('div', 'proyecto-modal-fila-ruta');
  const inputRuta = el('input', 'proyecto-modal-input') as HTMLInputElement;
  inputRuta.type = 'text';
  inputRuta.placeholder = 'selecciona una carpeta...';
  inputRuta.readOnly = esTauri; // en web el usuario completa la ruta
  filaRuta.appendChild(inputRuta);

  // Nota web: informa que la ruta debe escribirse manualmente.
  // Se declara siempre pero solo se agrega al DOM en modo web.
  const notaRutaWeb = el('div', 'proyecto-modal-label');
  if (!esTauri) {
    notaRutaWeb.textContent = 'El navegador no revela la ruta. Escribe la ruta absoluta (p. ej. C:/carpeta).';
    // Oculto por defecto, se muestra si el usuario usó el picker
    notaRutaWeb.style.display = 'none';
    lblRuta.appendChild(notaRutaWeb);
  }

  // Selector de carpeta oculto (Tauri via invoke, web via input file)
  const abrirPickerRuta = async () => {
    if (esTauri) {
      try {
        const invokeFn = opts.invoke ?? (window as unknown as Record<string, unknown>).__TAURI_INVOKE__ as
          | ((cmd: string, args?: Record<string, unknown>) => Promise<unknown>)
          | undefined;
        if (invokeFn) {
          const ruta = (await invokeFn('elegir_carpeta_proyecto')) as string;
          if (ruta) inputRuta.value = ruta;
        }
      } catch {
        /* si no hay invoke, el usuario escribe a mano */
        inputRuta.readOnly = false;
      }
    } else {
      // Modo web: input file con webkitdirectory como selector visual
      pickerFile.click();
    }
  };

  // Web: input file oculto para seleccionar carpeta (webkitdirectory)
  const pickerFile = el('input') as HTMLInputElement;
  pickerFile.type = 'file';
  pickerFile.style.display = 'none';
  /* @ts-ignore - webkitdirectory es Chromium/WebKit */
  pickerFile.webkitdirectory = true;
  pickerFile.addEventListener('change', () => {
    if (pickerFile.files && pickerFile.files.length > 0) {
      // webkitdirectory expone la ruta relativa; la absoluta no está
      // disponible por seguridad. Extraemos el nombre de la carpeta elegida
      // como referencia y dejamos que el usuario escriba la ruta real.
      const primera = pickerFile.files[0];
      const carpeta = primera.webkitRelativePath
        ? primera.webkitRelativePath.split('/')[0]
        : primera.name;
      inputRuta.value = carpeta;
      inputRuta.readOnly = false;
      inputRuta.focus();
      notaRutaWeb.style.display = '';
      inputRuta.placeholder = `escribe la ruta absoluta (carpeta «${carpeta}» seleccionada)`;
    }
  });
  filaRuta.appendChild(pickerFile);

  const btnElegir = el('button', 'proyecto-modal-boton-ruta') as HTMLButtonElement;
  btnElegir.type = 'button';
  btnElegir.title = 'Elegir carpeta';
  btnElegir.appendChild(icono('lupa', true));
  btnElegir.addEventListener('click', abrirPickerRuta);
  // Solo en Tauri (input readOnly) el campo abre el picker al hacer clic; en
  // web el input es editable y el clic debe dejar escribir (no abrir el
  // selector cada vez que se intenta teclear la ruta).
  if (esTauri) inputRuta.addEventListener('click', abrirPickerRuta);
  filaRuta.appendChild(btnElegir);
  lblRuta.appendChild(filaRuta);
  cuerpo.appendChild(lblRuta);

  // Botones
  const botones = el('div', 'proyecto-modal-botones');
  const btnCancelar = el('button', 'proyecto-modal-btn') as HTMLButtonElement;
  btnCancelar.type = 'button';
  btnCancelar.textContent = 'Cancelar';
  btnCancelar.addEventListener('click', cerrar);

  const btnGuardar = el('button', 'proyecto-modal-btn proyecto-modal-btn-primario') as HTMLButtonElement;
  btnGuardar.type = 'button';
  btnGuardar.textContent = 'Guardar';
  btnGuardar.addEventListener('click', () => {
    const nombre = inputNombre.value.trim();
    const ruta = inputRuta.value.trim();
    if (!nombre) {
      inputNombre.focus();
      return;
    }
    if (!ruta) {
      inputRuta.focus();
      return;
    }
    opts.onGuardar(nombre, ruta);
    cerrar();
  });

  botones.appendChild(btnCancelar);
  botones.appendChild(btnGuardar);
  cuerpo.appendChild(botones);

  modal.appendChild(cuerpo);
  fondo.appendChild(modal);

  // ---- comportamiento ----
  function abrir(): void {
    inputNombre.value = '';
    inputRuta.value = '';
    fondo.hidden = false;
    setTimeout(() => inputNombre.focus(), 50);
  }

  function cerrar(): void {
    fondo.hidden = true;
  }

  fondo.addEventListener('click', (e) => {
    if (e.target === fondo) cerrar();
  });
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && !fondo.hidden) cerrar();
  });
  inputNombre.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && inputNombre.value.trim()) inputRuta.focus();
  });
  inputRuta.addEventListener('keydown', (e) => {
    if (e.key === 'Enter' && inputRuta.value.trim()) btnGuardar.click();
  });

  return { raiz: fondo, abrir, cerrar };
}