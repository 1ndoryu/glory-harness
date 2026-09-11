# Roadmap — glory-harness

> Proyecto: Glory Harness (Rust workspace: `core` + `cli` + `desktop/src-tauri`; UI Tauri 2 +
> TypeScript vanilla en `desktop/ui`). Rama primaria: `main`. Gate: Sentinel/VarSense en
> `.quality-tools-harness` (autoridad de cierre). Convención documental del área: planes en
> `Agente/planes/`, evidencia en `Agente/completados/`, documentación en `Agente/documentacion/`.

## Contexto y fuentes

- **Plan maestro de escritorio (UI real + backend Tauri):** `Agente/planes/plan-glory-harness-desktop-2026-09-03.md`
  (ID **039A-1**). Contiene fases F1–F6 y el anexo **§10**: mapa de cada acción de la UI →
  backend real (nueva conversación, acciones del agente, selector de modelo real,
  configuraciones, cancelar ejecución, modos) + bloqueos de integración B1–B4.
- Estado por fase (detalle en el plan 039A-1): F1 ☑ · F2 ☑ (318A-16) · F3 ☑ · F4 ☑ (backend, 17 comandos) · F5 ☑ (cableado) · F6 ☑ (bundle). Correcciones del primer `tauri dev` (H1–H7) cerradas el 04-09 (ver `Agente/completados/tareas-2026-09-04.md`).

## Siguiente bloque ejecutable

**Bloque A — Poner a funcionar la UI con la app Tauri real (debug primero).**
Orden propuesto (dependencias de abajo arriba):

1. **F3 PersistenciaSqlite** — historial durable (prerrequisito de conversaciones reales; B4). ✔ (04-09)
2. **Comandos de conversación** en `desktop/src-tauri` (anexo §10.1). ✔ (04-09)
3. **Bloqueos B1 (capabilities) y B2 (detección `__TAURI__`)**. ✔ (verificados en el primer `tauri dev`)
4. **Cablear la UI** (`main.ts`/`sidebar.ts`/`modal.ts`). ✔ (04-09, F5) + panel meta real ✔ (`5dcefe4`)
5. **Primer `tauri dev` en debug** (build en `C:\tmp`): verificar ciclo real completo. ◐ hecho —
   el arranque real señaló 7 hallazgos (H1–H7), ya corregidos. 
6. **Cierre del bloque:** HECHO 06-09 — E2E en la ventana real Todo OK (enviar →
   streaming en vivo, tools → aprobación → `turno-fin`, P3 rewind+restaurar, P5 2
   paneles, P6 150k tras reiniciar) + `tsc --noEmit` + build UI + `cargo test
   --workspace` (284 verdes) + gate 039A-3 PASS + commit. Evidencia en
   `Agente/completados/tareas-2026-09-06.md`.

> Confirmado con el usuario: **revisar/ajustar el plan primero** (hecho, anexo §10) y **debug
> primero** (no release). El panel «modo meta» real (anexo §10.5.2) entró en este Bloque A y ya
> quedó implementado (commit `5dcefe4`).

## Tareas pendientes

- [x] **F3 — PersistenciaSqlite** (`cli/src/persistencia_sqlite.rs`, 04-09): `AgentPersistence` +
      `ProgramadorTareas` sobre rusqlite bundled (WAL, `%APPDATA%/glory-harness/glory-harness.db`).
      CRUD de conversaciones, mensaje de usuario persistido por el consumidor en `enviar_turno`,
      `cancelado` al abortar (B5/B6 cerrados). Sesión Tauri usa SQLite con `user_id` estable
      (tabla `config`); respaldo en memoria con aviso si la BD no abre.
- [x] **F4 — Comandos IPC de conversación + workspace + config** (backend, 04-09): 16 comandos
      (`abrir_sesion`, `enviar_turno`, `cancelar_turno`, `responder_aprobacion`,
      `pendientes_aprobacion` + `conversacion_nueva`, `listar_conversaciones`,
      `cargar_conversacion`, `renombrar_conversacion`, `archivar_conversacion`,
      `eliminar_conversacion`, `proveedores_disponibles` (allowlist real del núcleo vía
      `catalogo_proveedores`), `config_leer`/`config_guardar`, `elegir_workspace` (rfd),
      `actualizar_meta`). `cargo check` + clippy limpios, 188 tests verdes (29 cli + 159 core).
- [x] **Panel «modo meta» real** (04-09, commit `5dcefe4`): `actualizar_meta` en caliente,
      tiempo de turno, tokens desde `AgenteEvento.Usage`, política de play/pausa; retirado el
      boceto temporal `panelMetaBoceto.*` (anexo §10.5.2).
- [x] **B1 — capabilities** `desktop/src-tauri/capabilities/default.json` (`core:default`, ventana `main`) — verificadas en el primer `tauri dev` (04-09).
- [x] **B2 — detección de entorno** (`esEntornoTauri`/`__TAURI__`) — verificada en el primer `tauri dev` (04-09).
- [x] **F5 — Cablear UI → backend** (04-09, commit `8a1310c`): sidebar real (nueva/listar/
      cargar/renombrar/archivar/eliminar + `sustituir`), historial persistido al arrancar
      (reabre donde se quedó), modelo/modo/razonamiento en config persistida, deriva
      allowlist commandcode→glory, meta del boceto → `actualizar_meta` con puerta de modo en
      backend, `reconfigurar_sesion` al cambiar modelo/modo. Tipos `AgenteEvento` fieles al
      núcleo.
- [x] **F6 — Empaquetado + RAM + gate** (04-09, commit `8a8768c`): `tauri build` ok — exe
      release + MSI (`Glory Harness_0.1.0_x64_en-US.msi`) + NSIS setup en
      `C:\tmp\glory-target\glory-harness\release\bundle\`. Iconos válidos generados
      (`tauri icon`, array `icon` en config). RAM arranque release: WS 40 MB (objetivo
      ≤140 MB ✓). `install-release.bat` (copia el release a `%LOCALAPPDATA%\GloryHarness`).
      Gate `039A-1` **PASS** (0 errores, warnings preexistentes).
- [x] **069A-5 — Deuda cero Sentinel** (CERRADA 07-09 con transferencia):
      F1-F5 aterrizados 06-09 (core+CLI 0/0/0 por fichero entonces). F6 nunca
      llegó a 0/0/0: el bloqueo (desktop no compilaba) ya está levantado
      (`cargo check -p glory-harness-desktop` verde 07-09), pero sus 2
      warnings persisten y el código reciente (069A-2 F2-F4, 069A-7/9/10,
      Proyectos) metió deuda nueva. Gate full 07-09: 2E/10W/1I
      (`.quality-reports/check/069A-5/latest.md`). Toda la deuda restante y
      nueva se transfiere al próximo plan de deuda (pendiente de crear tras
      tabla). Plan archivado en `Agente/planes/completados/`. No se reclama
      0/0/0.
- [x] **079A-1 — Deuda cero Sentinel, segunda vuelta**: HECHO 07-09, gate
      full PASS 0/0/0 (ver `Agente/completados/tareas-2026-09-07.md`). Plan
      archivado en `Agente/planes/completados/`.
- [x] **069A-4 — Memoria de aprendizaje fases 1–5** (B3-F8b): HECHO 06-09
      (ver `Agente/completados/tareas-2026-09-06.md`).
- [x] **069A-1 — Navegador interno visible completo**: F1+F2 (WebView2 child, comandos IPC,
      ciclo COM) + F3 (panel UI) + F4 (anotaciones+ToolBrowser) + F5 (tool núcleo) + F6
      (pipeline multimodal: captura base64 + evento + runtime relay + frontend display).
      Commit `5f67e82`. Plan archivado en `Agente/planes/completados/`.
      Excepcion conocida: desktop compilation bloqueada por errores pre-existentes en
      navegador.rs (Tauri 2 IPC macros).
- [ ] **069A-6 — Navegador web: páginas que bloquean iframes (X-Frame-Options)**: pendiente
      de decisión del usuario (registrado 06-09 tras probar el Navegador en modo web). Causa:
      en el navegador el panel usa un `<iframe>` y muchos sitios (Google, YouTube, etc.)
      envían `X-Frame-Options: sameorigin`/CSP → se niegan a mostrarse dentro del panel
      (`Refused to display ... in a frame`). No hay forma técnica de saltárselo desde un
      iframe normal. Opciones a evaluar con el usuario: (a) botón "abrir en pestaña" junto
      al iframe para sitios bloqueados; (b) abrir siempre en pestaña nueva en modo web;
      (c) solo avisar en el log cuando un sitio bloquea la vista. Estado actual del fix de
      modo web (commiteado aparte): panel navegador dual — Tauri=WebView2 nativa intacta,
      web=`<iframe>` real con URL/navegación/recargar/cerrar funcionando (solo falla la
      captura, que es WebView2). El código en sí no tiene bug; es una limitación del iframe.
- [x] **069A-7 — Conversaciones: crear la fila SOLO al escribir (web + app)** (06-09):
      HECHO. Semántica create-on-write: no se crea fila al abrir/recargar/"Nueva conversación";
      la fila se crea al enviar el primer mensaje (auto-nombre H5). `Option<Uuid>` en estado de
      conversación (web `SesionWeb` + desktop `PanelDatos`), DELETE sin fila fantasma, borrador
      local en el front. Verificado: API + navegador end-to-end (5 escenarios); 80 tests cli +
      7 tests desktop; UI build OK. Gate bloqueado por `tool-release-unpublished` preexistente
      (repin `6baf87c2`, ajeno). Plan movido a `Agente/planes/completados/`; evidencia en
      `Agente/completados/tareas-2026-09-06.md`. BD quedó en 0 conversaciones (limpia).
- [x] **069A-8 — Selector de área de trabajo en el composer** (09-09):
      Añadido `<select>` nativo "Área de trabajo" dentro de `.entrada`, antes de `.caja`.
      Opciones: "Sin proyecto" + workspaces registrados. Cableado `DepsPanel` → `main.ts` →
      `adaptador.sesion.workspaces.activarPorRuta()`. Al cambiar proyecto: pone el panel
      en borrador y refresca sidebar. CSS en `entrada.css`. Build tsc+vite OK. Verificado
      en navegador: selector visible, cambio de opción funcional, panel reacciona.
- [x] **069A-9 — Seleccionar elemento de la página + modelo navega** (07-09):
      (a) Botón "Seleccionar elemento" en el panel navegador: hover resalta el
      elemento (outline + crosshair) y al hacer clic captura un descriptor robusto
      (`pagina` URL + `selector` CSS con `#id`/tag+clases+nth-child + `etiqueta` +
      `texto`) que se adjunta como badge al chat; el siguiente mensaje lo antepone
      (`[elemento de la página <url> — selector CSS: <sel> — etiqueta: <tag>]`) para
      que el modelo lo reciba. Solo app de escritorio (WebView2); modo web → aviso.
      E2E desktop 9/9 OK (hover resalta, clic captura, badge visible, envío con
      badge). (b) Fix causa raíz: el modelo no veía `navegador_reflejo` al cambiar
      de modelo porque `SesionComun::reconfigurar` reconstruía el runtime con
      `navegador: None`. `SesionComun` conserva el puerto y lo reinyecta en
      `reconfigurar`/`cambiar_workspace`. E2E modelo real: llamó `navegador_reflejo`
      (ok), navegó al fixture e hizo clic en `#btn-uno` (contador=1). Evidencia en
      `Agente/completados/tareas-2026-09-07.md`.
- [x] **069A-10 — Persistencia web de modelo, modo y razonamiento** (07-09):
      Fix: `guardar_config` persiste 4 claves en BD, `resolver_opciones` las
      restaura, `api.ts` mapea `proveedor→provider`, `opcionesArranque` envía
      campos vacíos. E2E verificado: PATCH devuelve configuración, sesión nueva
      la recupera, recarga mantiene valores. Gate bloqueado por
      `tool-release-unpublished` ajeno. Evidencia en
      `Agente/completados/tareas-2026-09-07.md`.
- [x] **069A-11 — Inicio del panel meta y acceso web local sin token** (07-09):
      El panel meta se monta después del selector de workspace y permanece oculto
      en el borrador inicial; aparece tras crear la conversación con el primer
      mensaje. El modo web local acepta sesión sin token y limita el bind a
      loopback cuando no hay `GLORY_HARNESS_WEB_TOKEN`. Evidencia en
      `Agente/completados/tareas-2026-09-07.md`.
- [x] **089A-17 — Git local estilo Synara: staged, changes y diff seleccionado** (09-09):
      El panel Git separa `Staged` y `Changes`, muestra estadísticas por archivo,
      mantiene el diff oculto hasta seleccionar una fila y renderiza un único
      archivo con ruta, hunks y números de línea. Tauri y HTTP entregan
      `diff_staged`/`diff_unstaged`, incluidos archivos `??` con límite combinado
      de 1 MiB; la selección usa grupo+ruta. Gate `089A-2` PASS 0E/0W/0I.
      Evidencia en `Agente/completados/tareas-2026-09-09.md`.
- [ ] **089A-1 — Controles de ventana propios estilo Paseo** (08-09, en curso):
      Quitar botones nativos (`decorations: false` en `tauri.conf.json`) e
      integrar minimizar/maximizar-restaurar/cerrar propios en la cabecera del
      panel principal (nuevo `componentes/ventana.ts` + `estilos/ventana.css`,
      iconos `minimizar`/`maximizar`/`restaurar` en `iconos.ts` + `tipos.ts`;
      orden Paseo; hover invertido monocromo reutilizando `.cab-boton`).
      Solo bajo Tauri (`esEntornoTauri`); en web no se montan. La cabecera
      principal actúa como barra de arrastre (`data-tauri-drag-region` +
      `startDragging()` programático; los clics en botones/inputs no arrastran).
      Referencia guardada en `area-trabajo/paseo`
      (`components/desktop/window-controls.tsx`, `titlebar-drag-region.tsx`).
- [ ] **089A-2 — Layout Paseo: entrada flotante, toggles, tabs y preview** (08-09, en curso):
      (a) `.mensajes` sin `max-width`/centrado (todo el ancho); `.entrada`
      flotante por encima (`absolute`, fondo sólido + borde superior) con
      reserva inferior dinámica vía `ResizeObserver` en `panelChat`.
      (b) Botón sidebar siempre visible y alterna (mostrar/ocultar); el
      preview de archivos vive dentro de Files; × en la barra de tabs cierra
      el panel.
      (c) Panel derecho con tabs (`panelDerecho.ts` + `tabs.css`): Chat
      lateral, Files, Git local y Navegador conviven; el preview pertenece
      al pane Files y no es una tab independiente.
      grip único; comando nuevo `navegador_mostrar` (oculta la webview hija
      sin destruirla al cambiar de tab).
      (d) Files integra árbol + preview de archivo (`panelFiles.ts` +
      `files.css` + comando `leer_archivo` validado al workspace); los
      cambios `file_write/file_patch` se reflejan en el archivo afectado.
      No existe una tab Cambios/Visor independiente.
      (e) Toggle del panel derecho (botón siempre visible en la cabecera
      principal, espejo del izquierdo; ocultar no destruye las tabs), ×
      propio por tab y multi-chat (una tab `chat:<id>` por conversación,
      máx. 8 laterales; reabrir una abierta solo activa su tab).
- [ ] **089A-3 — Barra superior global estilo Synara** (08-09, en curso, parte 1;
      commit `d013e03`, verificación visual pendiente del usuario; ajustes 08-09:
      orden fiel Synara —lista, atrás, adelante … marca … toggle derecho +
      botonera—; sin botón de visor (redundante); atrás/adelante presentes pero
      deshabilitados; botonera caption 100% Lucide):
      Barra de 46px a todo el ancho por encima de sidebar/paneles/panel
      derecho (nuevo `componentes/barraSuperior.ts` + `estilos/barraSuperior.css`,
      primera hija de `#app`): toggle sidebar + atrás/adelante mudados
      desde la cabecera del principal, zona central arrastrable y botonera
      min/max/cerrar con iconos Lucide (46px, planos, orden Paseo;
      cerrar con hover rojo `#c42b1c` como la referencia —excepción explícita
      a "sin rojo hover" pedida por el usuario). Arrastre + botonera solo bajo
      Tauri. La cabecera queda con título + ⋯ (+ × en laterales). Referencia:
      `area-trabajo/synara` (`DesktopWindowControls.tsx`,
      `SidebarHeaderNavigationControls.tsx`, `AppNavigationButtons.tsx`). Plan en
      `Agente/planes/plan-089A-3-barra-superior-2026-09-08.md`.
- [ ] **089A-4 — Launcher del panel derecho estilo Synara** (08-09, en curso, parte 2):
      Al abrir el panel derecho sin tabs muestra el inicio para elegir contenido
      (nuevo `estilos/launcher.css`, estado vacío en `panelDerecho.ts`): pantalla
      de opciones centrada (icono + etiqueta, full-width) al estilo
      `RightDockLauncher` de Synara con Files, Git local, Navegador y Chat
      lateral (con
      gating: el chat solo si hay conversación activa). El × global oculta el
      panel; cerrar la última tab deja el inicio (ya no se desmonta). Referencia:
      `RightDock.tsx` + `rightDockPaneMeta.tsx` en `area-trabajo/synara`.
- [ ] **089A-5 — Pendiente Synara (lógica, no visual)** (08-09, pendiente):
      historial de la app para atrás/adelante (misma lógica que Synara,
      hoy deshabilitados) y Terminal/Files/Source control como opciones del
      inicio cuando existan esos paneles.
- [x] **089A-13 — Gate cubre frontend TS + Tauri en VarSense** (09-09,
      HECHO): el gate solo analizaba `.rs` de `core`/`cli` (herencia de fase 0);
      añadidos `desktop/ui/src/**/*.ts` a `sentinel.config.json` y
      `desktop/src-tauri/src/**/*.rs` + `desktop/ui/src/**/*.ts` a
      `varsense.config.json`. Verificado: gate 089A-13 ve el TS (FAIL con
      14 errores + 116 warnings + 7 info, todo deuda real del frontend).
      Evidencia: `.quality-reports/check/089A-13/latest.md` (+ `sentinel.json`
      con ubicaciones).
- [ ] **089A-14 — Sanear hallazgos TS del gate (deuda revelada por 089A-13)**
      (09-09, pendiente): 14 errores (`innerHTML` ×10 en
      `entrada/mensajes/iconos/panelMeta/dom`: riesgo XSS; `catch` vacío ×3 en
      `panelNavegador.ts:423,491,521`; `main.ts` 1182 ef. triplica el límite
      300 de componentes) + 116 warnings (62 `barras-decorativas`, 22
      `window-reference` y 19 `dom-access` fuera de plataforma, 8
      `limite-lineas`, 3 interfaces grandes, 1 `console`, 1 dir) + 7 info
      (3 interfaces + 4 `todo-pendiente`). Detalle por fichero en
       `.quality-reports/check/089A-13/sentinel.json`. OJO: el árbol queda en
       rojo hasta sanearlo.
- [x] **089A-15 — Gate con cobertura por defecto (nada fuente fuera en silencio)**
      (09-09, HECHO): etapa `coverage`
      (`scripts/quality/sentinel-coverage.mjs`, primera del gate, fail-closed)
      que cruza `git ls-files` contra los includes de ambas configs; gate
      089A-15: coverage PASS 0/0/0 (165 analizables cubiertas, 35 visibles en
      sidecar). Sondas `--files-from` probaron la semántica real del motor
      (nombres sin glob = solo raíz, `src/**/*.ts` excluye `vite.config.ts`,
      css nunca se analiza) y se añadieron patrones explícitos
      (`core/cli/tauri Cargo.toml`, ambos `package.json`,
      `desktop/ui/vite.config.ts`, `**/*.mjs`, todos verificados total=1 en
      el motor). Evidencia: `.quality-reports/check/089A-15/`.
- [x] **089A-16 — Saneamiento total del gate (cero deuda tras 089A-15)**
      (09-09, HECHO): gate full **PASS 0/0/0** con `coverage` verde.
      F2-resto (`edbc8d9`, boundary DOM/window + barrels puros, gate
      F2RESTO 0E/0W/10I) + B-ISP (`f292869`, 10 interfaces por `extends`,
      gates BISP/BISP2 0E/0W/0I) + F3 (`caec783`, cero warnings Rust,
      clippy 0 + tests 363 OK) + cierre (`tsc` EXIT 0, `vite build` OK).
      Evidencia: `Agente/completados/tareas-2026-09-09.md` (entrada
      089A-16); reportes `.quality-reports/check/089A-16*/`. Pendiente:
      re-gate `unwrap-produccion-rs`.
- [x] **109A-1 — Hook pre-compactación** (10-09, HECHO):
      hook externo configurable (`ContextoConfig.gancho_pre_compact`) antes de
      resumir: recibe JSON por stdin, admite veto `exit 2` y ajuste JSON
      validado; timeout acotado, fallo/timeout/stdout inválido/salida no cero =
      warning + la compactación continúa. Runner HTTP con logs redacted, carga
      persistida en `chat`/`tui`/`run`/`schedule` y borrado/null consistente en
      UI. Evidencia en `Agente/completados/tareas-2026-09-10.md`.
      Gate canónico ejecutado: FAIL por `sccache-no-configurado` y deuda heredada
      de `data/referencias-cli/**`; análisis reducido del alcance propio: 0/0/0.
- [x] **109A-2 — Memoria estrictamente por proyecto + export/import** (10-09,
      HECHO): `AmbitoMemoria { Global | Proyecto(Uuid) }` en el contrato, con
      ámbito propagado por tools, `TurnoConfig`, proveedor y curador
      (`ejecutar_curador_todos` recorre todos los ámbitos). Migración real de
      la tabla `memoria` a `UNIQUE (user_id, workspace_id, clave)` con
      centinela `''` para global (los `NULL` no colisionan en `UNIQUE` y
      romperían el `ON CONFLICT` del upsert); los recuerdos previos quedan
      globales, sin reasignar. CLI `memoria` con `--global`,
      `--proyecto <uuid|ruta>` y `--todos`, más export/import markdown por
      recuerdo (`cli/src/infra/memoria_io.rs`): destino `project`
      (`.glory/memorias`) o `local`, sanitize obligatorio en el import.
      Evidencia: 393 tests lib verdes, clippy `-D warnings` limpio, build UI OK;
      gate canónico FAIL por deuda ajena (`sccache` y `data/referencias-cli/**`)
      con 0 hallazgos en `cli/src`/`core/src`; detalle en
      `Agente/completados/tareas-2026-09-10.md`. F3 (sección en Configuración)
      cerrada como 109A-3; plan completo en
      `Agente/planes/completados/plan-109A-memorias-por-proyecto-2026-09-10.md`.
- [x] **109A-4 — Comandos `/` estilo VS Code (`/compactar`, `/meta`)** (10-09,
       **HECHO** F1–F4): catálogo v1 de 8 comandos
       (`/ayuda /modelo /compactar /contexto /limpiar /revisar /iniciar /meta`),
       menú flotante `/` (`dominio/comandosSlash.ts`, `menuComandos.ts`,
       `entradaComandos.ts`, `panelChatComandos.ts` + IPC
       `comandos_listar`/`comando_expandir` reutilizando `core::skill`),
       `/compactar` con punto de compactación persistido que `preparar_turno`
       aplica como `[resumen] + posteriores` (sin borrar mensajes) y
       `/meta <texto>` como override de modo **por turno** (guard RAII
       `GuardaModoTurno` + `modo_efectivo()`, `PeticionTurno`,
       `solo_lectura` en el transporte Tauri) con retiro del modo global `meta`
       (migración a `predeterminado` + aviso único). Evidencia: 124+283 tests
       verdes, clippy `-D warnings` limpio, type-check/build EXIT 0 (97 módulos),
       E2E web de `/compactar`, `/meta` y del segmentado de modo reducido, y gate
       `check 109A-4` con `coverage`/`sentinel` PASS (10 warnings preexistentes) y
       único error ajeno `sccache-no-configurado`. Detalle en
       `Agente/completados/tareas-2026-09-10.md`; plan cerrado en
       `Agente/planes/completados/plan-109A-comandos-slash-2026-09-10.md`.
- [ ] **Gate: etapa Rust y sccache** (10-09, pendiente, independiente):
      el gate de glory-harness no compila Rust (las etapas `coverage`,
      `sccache` y `sentinel` solo analizan), así que `cargo` directo queda
      bloqueado por el guard sin vía de validación declarada; añadir una etapa
      que ejecute clippy/tests y decidir la configuración de `sccache`
      (`rustc-wrapper` + `SCCACHE_CACHE_SIZE`), hoy en rojo por
      `sccache-no-configurado`.

> **Corte de referencia del bloque 109A** (10-09 11:41Z, `1aff7e0`, con WIP en el
> árbol): es el corte de la **CONSOLA** (workspace-manager) = **371 hallazgos** (**101 errores** + 269
> warnings + 1 hint), medido con **VarSense 2.2.1** + **Sentinel del checkout compartido**
> (`area-trabajo/.quality-tools/sentinel`, 0.7.8 @ `902c45e`). Errores: `cssInlineScript` **52**
> (VarSense; **0 en este repo desde 109A-7**, 11-09) · `unwrap-produccion-rs` **30** (Sentinel) ·
> `axum-ruta-sintaxis-rs` **19** (Sentinel).
> Warnings: `claseHuerfana` **258** (VarSense) · `console-production` **8** (Sentinel) ·
> `limite-lineas` **3** (Sentinel). Las cifras se mueven con el WIP (el mismo día: 366/102 → 371/101),
> así que lo estable es la familia y el veredicto de cada una, no el número.
> **Corte del GATE canónico de este repo** (mismo árbol y mismos 224 archivos): `scripts/quality/
> stages.json` ejecuta Sentinel 0.7.8 @ **`1587c59`** — el commit que este repo fija en
> `quality-tools.json` (`provisionPath: ../.quality-tools-harness/sentinel`) — y **no ejecuta VarSense**
> (no hay etapa `varsense`), así que su único corte propio es **0 errores, 8 warnings, 1 hint**
> (11-09, tras 109A-6: los dos `limite-lineas` —`main.ts` y `panelDerecho.ts`— y la densidad de
> `desktop/src-tauri/src/` ya están resueltos; el corte anterior era de 10 warnings).
> Verificado el 10-09 ejecutando los dos binarios sobre el mismo árbol: `902c45e` → **49 errores**;
> `1587c59` → **0**. Los 49 del corte de consola **ya están corregidos upstream** (`08aaf25`,
> `axum-ruta-sintaxis-rs` version-aware; `1587c59`, `unwrap-produccion-rs` en fichero solo-test): son
> **falsos positivos del medidor, no deuda de este repo** (ver 109A-10). De los 101 errores de la
> consola, **52 son reales** (`cssInlineScript`) y **49 son falsos positivos**.

- [ ] **109A-9 — Avisos reales del gate: `console-production` ×8 y `todo-pendiente` ×1**
      (10-09, independiente): los 8 `console.*` están todos en el panel del navegador —
      `panelNavegadorSeleccion.ts` 4 (`console.error` en 77 y 116, `console.warn` en 127 y 131),
      `panelNavegador.ts` 3 (capturar/atrás/adelante, en 101/140/156) y `orquestador/navegadorVista.ts`
      1 (97) — y chocan con la regla de «sin fallos silenciosos»: pasarlos a aviso visible al usuario o
      a estado explícito. El único hint `todo-pendiente` (`core/src/nucleo/context.rs:972`) es un
      **falso positivo**: la regla casa `/\/\/\s*(TODO|…|PENDIENTE|XXX)\b/i` y el `///` de un doc
      comentario en español deja los dos últimos caracteres como marca, así que salta con la palabra
      «todo» en prosa («todo el historial…»); los 4 hint del área son el mismo caso (TASKS,
      RESTAURANTE, coolify-manager-rs). No hay nada que arreglar aquí: va a 109A-10. DoD: 0
      `console-production`; el hint se cierra corrigiendo la regla, no el código.
- [ ] **109A-10 — Falsos positivos del MEDIDOR (`unwrap-produccion-rs`, `axum-ruta-sintaxis-rs`, `claseHuerfana`)**
      (10-09; **no hay nada que arreglar en este repo**, la acción es de `workspace-manager`):
      **suma la familia `claseHuerfana` de VarSense** (tras 109A-8: la medición de `orphan-classes`
      sobre `desktop/ui/src/estilos` da **302 marcas**, y el cotejo nombre a nombre no encuentra
      ninguna otra sin uso fuera de las hojas; las 7 que sí eran CSS muerto real se retiraron en esa
      tarea). Dos casos confirmados al cotejar en 109A-8: (i) **composición por plantilla** —
      `gitDiff.ts` escribe `` `git-diff-linea git-diff-${tipo}` `` con
      `tipo ∈ {adicion, eliminacion, contexto}`, así que `.git-diff-adicion` y `.git-diff-eliminacion`
      sí se aplican y la regla no las resuelve; (ii) **concatenación por ternario** —
      `'ic' + (pequeno ? ' ic-xs' : '')`, ya conocida.
      Ojo al alcance de la medición: `orphan-classes` barre el workspace entero (**4516 archivos hoy**,
      `data/referencias-cli/**` incluido), así que su total global no es comparable con el corte de la
      consola de 10-09 (258 marcas sobre 224 archivos); el número útil es el recuento por carpeta
      propia.
      **49 de los 101 errores del corte de consola no son defectos de este código y ya están
      corregidos upstream.** (a) `unwrap-produccion-rs` ×30 — `cli/src/comandos/web_datos/pruebas.rs`
      (24) y `core/src/herramientas/navegador/pruebas.rs` (6); los dos son módulos solo-test con
      `#![cfg(test)]` en su cabecera (lo añadió a propósito `079A-1 F6`), y el analizador que los
      marcaba (`rustAnalyzer.js` → `calcularRangosTest`) solo reconocía la línea suelta `#[cfg(test)]` y
      descartaba rutas `/tests/`, así que el atributo interno no lo silenciaba; lo corrigió `1587c59`
      («ignorar fichero solo-test con `#![cfg(test)]`»). (b) `axum-ruta-sintaxis-rs` ×19 — todo en
      `cli/src/comandos/web.rs` (`router()`); el mensaje afirmaba que «esta versión de matchit (0.7.3)
      parsea `:param`», pero el proyecto resuelve **axum 0.8.9 → matchit 0.8.4** (`Cargo.lock`), donde
      `{id}` es la sintaxis correcta y `:id` sería un segmento literal: **aplicar el consejo habría
      roto las rutas**; lo corrigió `08aaf25` (regla version-aware según `Cargo.lock`). Esos dos
      arreglos son exactamente los que fija `quality-tools.json` (`provisionPath:
      ../.quality-tools-harness/sentinel`, 0.7.8 @ `1587c59`, dos días más nuevo que el checkout
      compartido `902c45e` que usa la consola), así que **este repo no tiene deuda pendiente por ellos
      y no hacen falta `sentinel-disable-file`**: la consola resolvía el binario por el checkout
      compartido sin mirar el `provisionPath` que fija cada proyecto (pendiente registrado en
      `workspace-manager/roadmap.md` como `039A-4`).
      Queda **un** defecto de regla real que sí hay que reportar al checkout compartido:
      `todo-pendiente` marca el `///` de un doc comentario en español (`core/src/nucleo/context.rs:972`,
      «todo el historial…») porque su patrón `/\s*(TODO|…|PENDIENTE|XXX)\b/i` deja pasar la palabra
      «todo» en prosa; el mismo falso positivo aparece en los 4 hints del área (TASKS, RESTAURANTE,
      coolify-manager-rs). DoD: la cifra que se documente es la del corte del gate canónico (0 errores)
      y el hint deja de contarse.
- [x] **089A-12 — Files estilo Synara: árbol + visor integrado** (09-09,
      HECHO): Files es un único pane dividido (árbol a la izquierda y preview
      a la derecha al seleccionar un archivo); se eliminaron `21 entradas`,
      los estados persistentes y Visor como tab/opción independiente; los
      errores de filesystem/Git se muestran mediante toast global. Evidencia:
      `Agente/completados/tareas-2026-09-08.md` (entrada 089A-12); build UI y
      gate 089A-12 PASS 0/0/0.
- [x] **089A-11 — Files/Git se recargan al cambiar de área de trabajo**
      (08-09, HECHO 08-09): al cambiar el workspace activo con la tab de
      Files/Git abierta, la lista/diffs seguían del área anterior (Files/Git
      resuelven la raíz en el backend según `comun.workspace`). Mecanismo de
      suscripción en `main.ts`: `refrescarProyectos()` detecta el cambio de
      ruta y notifica a los suscriptores; Files/Git recargan solo si su tab
      está abierta (`panelDerecho.tiene`). Evidencia:
      `Agente/completados/tareas-2026-09-08.md` (entrada 089A-11);
      verificación navegador real: Files 21→3 entradas al pasar de
      `glory-harness` a `Test` y Git se recargó al volver (3 cambios);
      gate 089A-11 PASS 0/0/0.
- [x] **089A-10 — Files y Git en el modo web (08-09, HECHO 08-09)**: el servidor
      `glory-harness web` no expone endpoints de filesystem/Git (solo Tauri
      IPC), así que en el navegador Files/Git mostraban "no está disponible en
      el modo web". Portar la lógica pura de
      `desktop/src-tauri/src/filesystem.rs` + `git.rs` a endpoints HTTP de
      `cli` (`web_datos/files.rs` + `web_datos/git.rs`, raíz resuelta desde
      `comun.workspace`): `GET .../files/info`, `.../files/listar`,
      `.../files/leer`, `.../files/buscar` y `.../git/estado`; y cablear los 5
      stubs de `api.ts` (`workspaceInfo/ListarEntrada/LeerArchivo/Buscar/
      GitEstado`) a esas rutas + habilitar el visor web (`leerArchivo`). Sin
      watcher (ponytail, igual que 089A-9). Verificación: navegador real sobre
      `--fixture`. Evidencia: `Agente/completados/tareas-2026-09-08.md`
      (entrada 089A-10); gate 089A-10 PASS 0/0/0 (14 archivos); 91 tests lib
      del crate `cli` en verde (incluye `parsea_status_porcelain_nul`).
- [x] **089A-7 — Dividir `core/src/herramientas/tools_archivo.rs`** (08-09,
      HECHO como parte de 089A-8): `ToolContentSearch` extraída a
      `core/src/herramientas/content_search.rs`; `tools_archivo.rs`
      1342→~830 líneas. Evidencia: `Agente/completados/tareas-2026-09-08.md`
      (entrada 089A-8).
- [x] **089A-8 — `content_search` integrado 100% (tgrep-core compilado, sin
      depender del sistema)** (08-09, HECHO): motor in-process con `tgrep-core`
      (git tag v1.0.5). Índice persistente por workspace + manifiesto de
      frescura, verificación con `regex` + `sandbox.leer` (secretos excluidos),
      smart-case, timeout 180 s, fallback local con aviso. Supera a la v1 por
      `GLORY_TGREP_BIN` (código eliminado). Evidencia:
      `Agente/completados/tareas-2026-09-08.md`; gate 089A-8 PASS 0/0/0.
- [x] **089A-9 — Explorador real del workspace, cambios/diffs y terminal**
      (08-09, HECHO 08-09): Fases 0–4 completadas (contratos, filesystem
      backend con tests 7/7, árbol lazy con apertura, cambios seguros de
      tools sin truncamiento, Git local con panel y tab). Watcher nativo y
      terminal PTY diferidos por decisión de MVP (ponytail); se reabren
      como tarea separada si el uso real lo justifica. Evidencia:
      `Agente/completados/tareas-2026-09-08.md`.

> **Hecho (04-09, correcciones del primer `tauri dev`):** los 7 hallazgos del primer arranque
> real quedaron corregidos (H1–H7, bloque 039A-1). Detalle en
> `Agente/documentacion/hallazgos-primer-tauri-dev-2026-09-04.md` y evidencia en
> `Agente/completados/tareas-2026-09-04.md`. El E2E en ventana real quedó HECHO el 06-09
> (Todo OK + gate 039A-3 PASS; evidencia en `Agente/completados/tareas-2026-09-06.md`).
>
> **Corrección de causa raíz H2 (04-09, tras el primer fix):** el usuario confirmó que la
> respuesta del asistente seguía sin aparecer EN VIVO (solo al recargar). Causa: el frontend
> `real.ts` leía el discriminante `evento` pero el backend serializa `AgenteEvento` con tag
> `tipo` (`core/src/evento.rs`) → el switch no pintaba ningún evento. Fix aplicado en
> `desktop/ui/src/tauri/real.ts` (`evento` → `tipo`; type-check limpio). **Validado en
> vivo el 06-09:** el streaming de tokens aparece en la ventana (E2E Todo OK).
>
> **Hecho (UI, 03-09):** los botones del nav «Agentes» / «Flujo» / «Complementos» ahora
> muestran «próximamente» en el chat (`onAccionNav` en sidebar + aviso en main.ts; §10.4).
> «Nueva conversación» queda reservado a la acción real con backend (F3), no a «próximamente».
>
> **Boceto (03-09, §10.5.1):** panel «modo meta» corriendo visto sobre la UI real
> (`panelMetaBoceto.ts/.css` montado dentro de `#entrada`, antes de `.caja`; 1 línea: meta
> editable + estado · tiempo · tokens + play/pausa). Es TEMPORAL y simulado; el panel real
> (meta editable en caliente, tokens/tiempo del turno, play/pausa real) quedó implementado en
> §10.5.2 (commit `5dcefe4`) y el boceto se retiró.

## Planes activos

- `Agente/planes/completados/plan-workspace-explorer-diffs-terminal-2026-09.md` (089A-9) —
  **cerrado 08-09** con Fases 0–4 completadas; watcher y PTY diferidos (ponytail).
  activo; primera fase filesystem local + árbol/apertura, sin Git/watcher/PTY
  hasta disponer de contratos y evidencia verificable.
- `Agente/planes/completados/plan-deuda-cero-079A-1-2026-09-07.md` (079A-1) —
  **cerrado 07-09** con gate full PASS 0/0/0.
- `Agente/planes/plan-web-real-069A-2.md` (069A-2) — **cerrado con observaciones**:
  implementación y meta web verificadas contra el binario reconstruido; quedan
  observaciones del gate y no se reabre F5b sin nueva justificación.
- **069A-6** no tiene plan activo: permanece bloqueada por decisión de producto sobre
  el comportamiento ante iframes rechazados por CSP/X-Frame-Options.

## Historial de planes cerrados

- `Agente/planes/completados/plan-109A-meta-ciclo-vida-2026-09-10.md` (109A-5) —
  **cerrado 11-09** con F1–F4 completas (meta con ciclo de vida por
  conversación, tareas visibles atadas a la meta, cierre con evidencia —badge
  del pie + historial durable— y regla de bloqueo ×3 con pausa automática).
  Gate con `coverage`/`sentinel` PASS en el baseline de 10 warnings.
- `Agente/planes/completados/plan-109A-comandos-slash-2026-09-10.md` (109A-4) —
  **cerrado 10-09** con F1–F4 completas (menú `/`, `/compactar` con punto de
  compactación persistido, `/meta` como override de turno y retiro del modo global).
  Gate con `coverage`/`sentinel` PASS en el baseline de 10 warnings preexistentes.
- `Agente/planes/completados/plan-109A-memorias-por-proyecto-2026-09-10.md` (109A-2/109A-3) —
  **cerrado 10-09**.
- `Agente/planes/plan-glory-harness-desktop-2026-09-03.md` (039A-1) — **cerrado 06-09**; las
  fases F1–F6 y el Bloque A (backend + cableado + panel meta real) están cerrados; los 7
  hallazgos del primer `tauri dev` quedaron corregidos (H1–H7) y el E2E en la ventana
  Tauri real quedó Todo OK con gate 039A-3 PASS.
- `Agente/planes/plan-glory-harness-ux-turno-2026-09-04.md` (039A-3) — **cerrado 06-09**; el bloque P1–P6, P6b, el backend de contexto y el E2E Tauri real quedaron verificados. El detalle histórico siguiente conserva la evidencia de cada fase.
  (04-09): bloque completo aprobado (P1-P6). P1 (pie de turno + persistir uso/modelo real)
  HECHO (mock + type-check + cargo check). P2 (editar/volver a punto: rewind transaccional por
  `rowid` + menú por mensaje) HECHO (unit + type-check + build + mock; commit `039A-3 (P2)`).
  P4 (⋯ cabecera + sidebar colapsable/redimensionable + botón expandir) HECHO (type-check +
  build + navegador mock; commit `039A-3 (P4)`).
  P5 (2 paneles, M1) HECHO (05-09, commit `903a54b`): backend ya estaba (045f7e1, `panel_id`
  por panel); front gestor de 1-2 `.chat` — fábrica `panelChat.ts` + orquestador delgado en
  `main.ts` + "Abrir en panel lateral" (sidebar ⋯, D4) + responsive <900px apila los 2 chats.
  Verificado con navegador mock (2 paneles, turno global M1, foco al cerrar lateral, guard
  responsive). Evidencia en `Agente/completados/tareas-2026-09-05.md`.
  P6-front (indicador circular de contexto + config "Ventana de contexto" 150k) HECHO (05-09,
  commit `769492d`): SVG `.ctx-indicador` en `entrada.ts` + CSS; hook `onContexto` en `real.ts`
  (fuente única = `ContextoDetalle`/`usage`) → `setContexto` en `main.ts`/`panelChat.ts`;
  opción `contexto_max_ventana` en `opciones.ts` persistida vía config; rama mock pinta 7%/150k.
  Verificado con navegador mock (círculo llenándose + config visible). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6-front menú hover de detalle del indicador HECHO (05-09, commit `a69c2a3`): al pasar el
  cursor por el círculo se abre el pequeño menú `.ctx-detalle` con el detalle del uso de la
  ventana (usados de M + %, reserva de salida, entrada del turno; "sin datos" si no hay turno).
  Se elimina el `title` nativo; `EstadoContexto` propaga el detalle completo desde el backend.
  Verificado con navegador mock (principal y lateral). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6b (mejoras UX de paneles) HECHO (05-09, commit `67729b5`): requisito del usuario — el panel
  lateral se redimensiona con un divisor arrastrable (min 260 / max 70%, ancho persistido); el
  lateral usa la entrada completa (modelo/razonamiento/modo compartidos M1, cambiar en uno
  actualiza el otro); el ⋯ de la cabecera va al extremo derecho; la lista de conversaciones ya no
  se oculta manualmente (sin botón de ocultar): se auto-oculta por ancho mínimo (720px) y el
  botón solo aparece cuando está oculta para MOSTRARLA. Verificado con navegador mock (arrastre,
  clamp 260, persistencia, 650px/1200px, M1 compartido). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque HECHO (05-09, commit `aa8dbd4`): al arrastrar el divisor de la sidebar hasta el
  borde la lista se oculta del todo al soltar (antes el clamp la dejaba en 180px) y el botón
  "mostrar lista" la reabre; el grupo izquierdo de la cabecera colapsa cuando su botón está
  oculto (sin hueco fantasma). Verificado con navegador mock. Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque 2 HECHO (05-09, commit `3a53771`): el divisor del panel lateral funcionaba
  INVERTIDO (el lateral está anclado al borde derecho de `#paneles`, pero el ancho se calculaba
  desde el borde izquierdo). Ahora `ancho = paneles.right - clientX`: arrastrar a la izquierda
  ENGRANDE el lateral y a la derecha lo ENCOGE (clamp 260 / 70%). Verificado con navegador mock
  (crece 342→465→725 máx, encoge →328→260 mín, persistencia `lateral_ancho`). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque 3 HECHO (05-09, commit `260c08e`): los grips (sidebar y lateral) eran franjas de
  5px en el flujo flex que robaban ancho permanentemente y se pintaban de negro al hover/
  arrastrar (el "borde negro de 5px" al redimensionar). Ahora son áreas de captura ABSOLUTAS
  (`position:absolute`, 9px centrados sobre la línea divisoria de 1px, `z-index:5`, sin fondo):
  no ocupan layout y no muestran franja al pasar el cursor. Verificado con navegador mock
  (principal ocupa 838px sin perder 5px, hover transparente, arrastre y colapso intactos,
  <900px se sigue ocultando). Evidencia en `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque 4 HECHO (05-09, commit `a0b965c`): al abrir el panel lateral su input no se veía
  (el `medir()` de `montarEntrada` corre en el constructor antes de estar en el DOM →
  `scrollHeight` 0 → `#lateral-input` quedaba con `height:0px`; solo se veían los controles).
  Ahora `abrirEnLateral` llama a `lateral.medir()` tras montar el panel. Verificado con
  navegador mock (input lateral 16px como el principal, autoresize al escribir). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque 5 HECHO (05-09, commit `c97bb53`): se quita el borde superior del pie de la
  sidebar (el `border-top` de `#sidebar .pie` sobre el botón "Configuración"). Verificado con
  navegador mock (`borderTop: 0px`). Evidencia en `Agente/completados/tareas-2026-09-05.md`.
  P6b retoque 6 HECHO (05-09, commit `490e933`): el usuario pidió "hay mucha separación aquí
  entre el primer botón y el texto, baja un poco" (cabecera del chat, entre el icono mostrar
  lista/× y el título). El `gap` de `.cabecera-chat` era `--sp-md` (16px); se baja a `--sp-sm`
  (10px). El ⋯ del extremo no se ve afectado (`margin-left:auto`). Verificado con navegador
  mock (separación botón→título 16→10px). Evidencia en
  `Agente/completados/tareas-2026-09-05.md`.
   P6-backend HECHO 06-09 (`OpcionesRun.max_ventana` + `VENTANA_MINIMA`, desktop 150k, core 128k
   intacto; tests 269 verdes + gate 039A-3 PASS 0 errores; evidencia en
   `Agente/completados/tareas-2026-09-06.md`). Fix colateral en alcance: 2 `expect` de `vault.rs` →
   `let-else` total (+ `es_vacio` huérfano eliminado).
   E2E `tauri dev` en ventana real HECHO 06-09 (Todo OK: streaming en vivo, tools →
   aprobación → turno-fin, P3 rewind+restaurar, P5 2 paneles, P6 150k tras reiniciar;
   evidencia en `Agente/completados/tareas-2026-09-06.md`).

## Notas

- Build siempre en `C:\tmp` (`CARGO_TARGET_DIR=C:\tmp\glory-target\glory-harness`), nunca en el árbol.
- F5 UI la está construyendo el usuario; este roadmap refleja la integración con backend real.
- Divergencia conocida (04-09, para el dueño del núcleo/CLI): el runtime solo persiste turno +
  respuesta; el mensaje del usuario lo debe persistir el consumidor — el desktop lo hace, pero
  `chat`/`run`/`tui` del CLI hoy no llaman a `guardar_mensaje` para el usuario (su historial
  entre turnos pierde el lado usuario). No se tocó por ser carril ajeno.
- **Los números de línea de la consola van 0-based** (10-09; defecto de `workspace-manager`, no de
  este proyecto): la línea que se muestra es la **anterior** a la real (verificado en 4 reglas y 4
  ficheros: `web.rs` 499→500, `pruebas.rs` 28→29, `anotaciones.ts` 52→53,
  `panelNavegadorSeleccion.ts` 76→77). Causa: `workspace-manager/src/server/gate/analizador.ts:259`
  guarda `range.start.line` del LSP (0-based) sin sumar 1 y
  `workspace-manager/src/v2/paneles/PanelConsola.tsx:123` lo imprime tal cual. Afecta a todo el área,
  así que al triar un hallazgo hay que mirar la línea siguiente.
- **Un conteo no es comparable sin el build (commit) y el alcance** (10-09, verificado en este repo):
  el **mismo** 0.7.8 dio **49 errores** con el checkout compartido `902c45e` y **0** con `1587c59` (el
  commit que fija `quality-tools.json`), sobre el **mismo árbol y los mismos 224 archivos**; en
  WANDORIUS (8) y GLORYPORT (5) los dos builds coinciden, así que la diferencia **no** es subreporte
  del nuevo, son las dos reglas ya corregidas ahí. Por eso toda cifra de este roadmap debe decir con
  qué binario y con qué alcance se midió, y por eso `109A-10` se cierra sin tocar código de este repo.
