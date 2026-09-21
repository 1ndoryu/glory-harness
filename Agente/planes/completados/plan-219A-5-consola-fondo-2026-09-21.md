# 219A-5 — Consola de fondo: fin vivo, reap atómico, dueño y matar honestos (2026-09-21)

## Objetivo

Cerrar los 7 hallazgos del testing web real del 21-09 sobre la tab Consola
(filas `● corriendo` eternas, tope fantasma, transcripts 33→0→33, dueño
perdido al archivar, 1 ping → 3 consolas, matar sin feedback, 2 `cmd`
huérfanos). La tab pasa de "solo aprende el fin al abrir/recargar" a
"congela cada fila cuando el backend archiva".

## Causas raíz localizadas (código, no hipótesis)

- **H1 filas atascadas:** `ConsolaFin` solo se emite en síncrono.
  `core/src/herramientas/planificacion/comando.rs:165-174`
  (`evento_extra: None` en fondo, con comentario "F2 emitirá el fin al
  archivar" que **nunca se implementó**: el pump de
  `cli/src/infra/ejecutor.rs:420-509` no tiene canal de eventos fuera del
  turno). La tab solo aprende el fin vía `sincronizar()` (al abrir la tab,
  `panelConsola.ts:412`) o `cargarTranscript` (al seleccionar, `:297`).
  Sin canal vivo → fila `●` eterna hasta reload.
- **H2 tope fantasma:** consecuencia de H1. Las 4 filas `●` eran archivadas
  en el backend; por eso la 5ª (`e1747646`) sí se creó. Sin bug propio.
- **H3 transcript 0 + orden:** reap no atómico (`ejecutor.rs:495-499`:
  `vivas.remove` y `resultados.insert` con locks distintos; `salida()` en
  medio → `NoEncontrado` → la UI pinta 0 líneas) + `lista()` ordena las
  archivadas con `Instant::now()` (`:749-751`) sobre iteración de `HashMap`
  → orden no determinista entre llamadas.
- **H4 dueño:** `resultados: HashMap<String, ResultadoEjecucionComando>` no
  guarda origen; `lista` (`:759-760`) y `salida` (`:881`) fuerzan `Agente`.
- **H5 triple ejecución:** CAUSA LOCALIZADA 21-09 tarde vía SQLite
  (`%APPDATA%\glory-harness\glory-harness.db`, conv
  `1af2cc96-002d-49fc-89e7-47d0062daca5`): 3 turnos (`d91b5425`
  echo/whoami/ver, `9282e46c` ping -n 8 sync, `9264ab4c` ping -n 25); el
  turno `9264ab4c` contiene 2× `comando {"comando":"ping -n 25
  127.0.0.1","fondo":true}` a `22:14:53Z` y `22:14:55Z` (2 s) + 3×
  `comando_status` + `comando_matar`. NO es reintento del turno
  (`ya_reintentado` único, `tool_calls` secuencial): es RELANZAMIENTO DEL
  MODELO de la misma llamada fondo. F0: `fondo_duplicado()` en
  `ToolComando` reutiliza la viva idéntica (mismo comando + misma
  conversación) con aviso `NO lo relances`; solo fondo, sync intacto.
- **H6 matar:** la × solo actúa sobre la activa
  (`panelConsola.ts:531-536`, exige selección previa) y `matada: false` no
  da feedback (`conversaciones.rs:296-298`).
- **H7 huérfanos:** `kill_on_drop(true)` solo cubre drop ordenado; si el
  backend muere por kill, los hijos (`cmd` de propias, `cmd /C` de builtins
  por `jaula.rs:256`) sobreviven. Los PIDs 19872/22056 son de las 06:09,
  fuera del ownership del runner.

## Alcance / no alcance

Sí: F0–F5 + gate + completada. No: PTY, broadcast SSE (descartado por
alcance: exige bus por sesión en `web.rs`; se reabre si F1 no basta), Job
Objects Windows (solo si F5 demuestra que el apagado ordenado no basta),
VarSense (sin cambios de CSS salvo la × por fila, que reuse canon).

## Fases verificables

- **F0 — H5: fijar causa de la triple ejecución (previa, ajusta F1–F4).**
  Reproducir `ping -n 25` con log (request-id por POST de turno + llamada
  tool con timestamp) y determinar: reintento del turno, doble envío UI o
  reintento del modelo. Mitigación según hallazgo (p. ej. single-flight o
  dedup de ventana corta solo si la causa lo justifica; un `ping` repetido
  a propósito debe seguir creando 2 consolas).
- **F1 — Fin vivo en la tab abierta (H1, H2).** Refresco acotado: mientras
  la tab esté abierta Y `hayVivas()`, re-`sincronizar` ligero cada ~2 s
  (arranca/para con la tab; sin vivas no hay timer). Revisa explícitamente
  la regla "cero polling UI" de 209A-1: el polling vive solo con vivas
  visibles y muere sin ellas (tope temporal, no fondo permanente).
  Verificación: `ping -n 25` congela su fila solo (sin reload); con 4
  archivadas la 5ª se crea sin tope fantasma.
- **F2 — Reap atómico + orden estable + UI que no blankea (H3).**
  Backend: insertar en `resultados` ANTES de retirar de `vivas` (ventana
  `NoEncontrado` eliminada) + `seq` monótona de archivado para ordenar
  (adiós `Instant::now()` por llamada). UI: en error/transitorio de
  transcript, conservar líneas conocidas + reintentar (nunca pintar 0 sobre
  33). Tests lib del reap + orden. Verificación: 3 lecturas seguidas del
  transcript → mismo contenido y orden.
- **F3 — Dueño en archivadas (H4).** Envolver el mapa en struct privado
  `Archivada { resultado, origen, seq }` (sin tocar el contrato público
  `ResultadoEjecucionComando`); `lista`/`salida` propagan el dueño real.
  Verificación: propia archivada sigue `mía` en fila + visor tras reload.
- **F4 — Matar honesto (H6).** × por fila viva (no solo activa, canon
  28×28) + toast cuando `matada: false` ("ya había terminado").
  Verificación: matar sin seleccionar + matar una ya muerta avisa.
- **F5 — Huérfanos (H7).** Gancho de apagado: `matar_todas` en cierre
  graceful del `web` (espejo del reap global Tauri `main.rs:571`;
  verificar que existe en web) + documentar que el kill del proceso deja
  huérfanos. Los PIDs 19872/22056 requieren permiso del operador
  (`taskkill` manual, fuera del runner): pedirlo al implementar.

## Dependencias

F0 primera (puede recortar F1). F1–F4 independientes entre sí (mismo árbol:
integrar en serie, un commit por fase). F5 última (necesita backend vivo
para probar el apagado). Backend de pruebas en `:8799` parado antes del gate.

## Gate y DoD

`sentinel check 219A-5 --stages scripts/quality/stages.json` PASS como
último gate del commit a integrar + `type-check` + `build` UI + batería
web 1:1 del testing (un ping = una consola; fin sin reload; transcripts
estables; dueño tras archivar; matar con feedback). Cierre: entrada en
`Agente/completados/tareas-2026-09-21.md`, plan a `completados/`, roadmap
actualizado. Sin evidencia funcional no se cierra.

## Estado

Implementado 21-09 tarde (desviaciones del plan anotadas). Próximo: commit
por fase + mover a `completados/` + entrada de cierre.

- **F0 hecho:** `fondo_duplicado()` + test
  `fondo_duplicado_reutiliza_viva_misma_conversacion`; fast PASS 17 ok.
- **F1 hecho:** `programarRefresco/detenerRefresco` (2 s, solo con vivas y
  `onSincronizar`, sin solape) en `sincronizar/manejarEvento/crear propia`.
- **F2 hecho (desviación):** en vez de `seq` se unificó el estado en
  `RegistroConsolas { vivas, archivadas, orden: VecDeque }` bajo UN lock;
  reap atómico en `archivar()` (solo el pump archiva; `matar` no toca el
  registro); `lista()` ordena archivadas por `orden` (fin real). UI no
  blankea: no sustituye con volcado vacío si hay líneas.
- **F3 hecho (desviación):** `Archivada { resultado, origen }` sin `seq`
  (el orden lo da `orden`); `lista/salida` propagan el dueño.
- **F4 hecho (desviación):** × por fila como `span` con rol botón (un
  `button` no anida en el `button` de la fila; canon 28×28 no aplicable) +
  `matarEntrada` común + `onMatar: Promise<boolean> | void` + `onInfo`
  (toast matada / ya-terminada) + errores vía `onError` (sin doble toast).
- **F5 hecho (desviación):** el gancho web YA existía (`apagado_ordenado`
  209A-1 F4-resto llama `matar_todas` por sesión ante Ctrl+C); solo se
  verificó `matar_todas` contra el nuevo registro (snapshot de ids, sin
  deadlock) + comentario obsoleto actualizado. Kill del proceso sigue
  dejando huérfanos (documentado); PIDs 19872/22056 pendientes de permiso
  del operador.
- **Evidencia:** full gate forzado
  `sentinel check 219A-5 --stages scripts/quality/stages.json --full` PASS
  (rust 68.4 s, 0 errores; 30 tests lib ok incl.
  `reap_ordena_por_fin_y_retiene_dueno`) + `type-check` UI limpio +
  batería API con exe fresco: propia `ping -n 30` → archivada
  `codigo_salida: 0` con `origen: usuario`; `ping -n 300` matada
  (`matada: true`) → archivada `codigo_salida: 1`, orden por fin real,
  transcript con dueño; segundo matar → `matada: false` honesto.
  Sin clic visual (sin automatización de navegador): la × por fila y el
  refresco quedan para comprobación visual del operador.
