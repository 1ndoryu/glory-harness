# Plan 129A-10 — El agente controla GH (navegador + abrir archivos)

Fecha: 2026-09-12. Estado: activo. Roadmap: `129A-10`.

## Estado real verificado (la mitad ya existe)

- El agente YA maneja la webview embebida: tool `navegador_reflejo`
  (`core/src/herramientas/navegador/reflejo.rs:27`, operaciones
  abrir/navegar/capturar/js/cdp/click/rellenar/snapshot/cerrar) →
  `NavegadorPort` → `NavegadorTauri`
  (`desktop/src-tauri/src/navegador/puerto.rs`, inyectado en
  `main.rs:324-326`). Evento `tool_navegador` llega al front
  (`tauri/realTipos.ts:74`, `chat/turno.rs:263`).
- Hueco 1: el front refleja URL/captura pero NO abre la tab
  (`orquestador/ganchos.ts:50-57`): si está cerrada, "ve a esa página" ocurre
  a ciegas para el usuario. Apertura manual existe
  (`orquestador/navegadorVista.ts:55 abrirNavegador`, tab `navegador` con
  webview nativa + `navegador_mostrar/posicionar`).
- Hueco 2: no hay tool para mostrar un archivo arbitrario en GH. El preview
  de Files existe y el agente ya registra en él sus cambios
  (`onCambioArchivo`, `aplicarEventos.ts:131`), pero "abre el archivo X" no
  tiene camino agente→UI.

## Decisiones del usuario (12-09, registradas)

- Navegación del agente: abrir la tab SIN robar el foco del chat.
- Cierre intencional a mitad de turno: se respeta (solo toast, no reabre).
- `mostrar_archivo`: cualquier ruta que el agente pueda leer.
- Navegar/mostrar = lectura libre, sin aprobación.

## Fases verificables

- F1: al recibir `tool_navegador` con abrir/navegar ok, auto-abrir la tab
  navegador SIN robar el foco del chat (visible + aviso; la webview exige
  reposicionarse al mostrarse: `panelDerecho.ts:107-119`). Si el usuario la
  cerró a propósito a mitad de turno, no reabrir en cada evento (decisión de
  diseño en F1: reabrir solo en abrir/navegar, no en click/capturar).
- F2: nueva tool de solo-UI `mostrar_archivo {ruta}` (lectura: abre Files +
  preview + selecciona; no edita, no necesita sandbox nuevo; pasa por el
  permiso que corresponda a lectura). Front: evento → `abrirFiles` +
  selección del archivo.
  [DISEÑO 13-09 — backend `core`, aditivo; `efecto()` por defecto `false`
  = lectura libre sin aprobación (`tool.rs:157-159`):
  1. `contrato/evento.rs`: variante `MostrarArchivo { ruta, descripcion }`
     (espejo de `ToolNavegador`).
  2. `herramientas/archivo/tools_archivo.rs`: `ToolMostrarArchivo`
     (`id "mostrar_archivo"`, schema `{ruta required}`); `ejecutar`
     valida con `sandbox.resolver(ruta)` (existe + dentro del workspace)
     y `sandbox.es_secreto(ruta)` (respeta `.glory-harness/`, `.git/`,
     secretos); devuelve `ok_con_evento(texto, resumen, MostrarArchivo)`.
  3. Registro en `registrar_tools_archivo` (solo local con sandbox,
     fail-closed como `file_read`).
  4. `desktop/src-tauri/.../chat/turno.rs:263`: brazo
     `MostrarArchivo{..} => "mostrar_archivo"`. CLI/web serializa
     genérico, sin toques.
  Front `desktop/ui`, aditivo:
  5. `tauri/realTipos.ts`: unión `mostrar_archivo {ruta, descripcion}` +
     hook `onMostrarArchivo?`. 6. `tauri/aplicarEventos.ts`:
     `case 'mostrar_archivo'`. 7. `componentes/panelFiles.ts`:
     `mostrarArchivo(ruta)` público (reusa `abrirArchivo`; NO toca el
     mapa de cambios: es vista, no cambio). 8. `orquestador/panelDerecho.ts`:
     `abrirFilesEn(ruta)` (espejo de `abrirCambiosEn`). 9. `ganchos.ts`:
     `onMostrarArchivo` → toast + `deps.mostrarArchivoEnFiles(ruta)`;
     cableado en `depsCrearPanel.ts`/`main.ts`. 10. Sin robo de foco
     (`abrirTab` solo conmuta); si el usuario cerró el panel a mitad de
     turno se respeta la supresión F1 (solo toast).]
- F3: smoke `tauri dev`: "ve a <url>" abre y muestra la página; "abre
  <archivo>" lo muestra en el preview.

## No alcance

- Devolver control fino (scroll/click por coordenadas del usuario) ni
  multi-tab de navegador: una sola webview como hoy.

## DoD

- Con la tab cerrada, el agente navegando la abre y se ve la página; pedir un
  archivo lo muestra en el preview; gate PASS + `tsc` EXIT 0.
