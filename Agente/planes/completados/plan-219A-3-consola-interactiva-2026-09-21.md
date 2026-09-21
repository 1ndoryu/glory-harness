# Plan 219A-3 — Tab Consola como visor interactivo (21-09-2026)

## Objetivo
La tab deja de ser un visor con texto explicativo: sin consolas muestra una
consola lista; con ejecuciones, una sub-barra interna con las consolas del
agente para verlas e interactuar (stdin). Sin PTY (fuera de alcance).

## Alcance / no alcance
- SÍ: sub-barra horizontal siempre visible; frame "lista"; input stdin bajo
  el visor (solo viva); backfill al abrir la tab (lista + salida); endpoints
  web + comandos Tauri + transporte UI; tests.
- NO: lanzar comandos desde la UI (nueva superficie RCE: decisión aparte);
  PTY/colores/interactivos full-screen; archivar síncronas (transitorias,
  solo en vivo + error honesto al escribir).

## Fases verificables
1. **Contrato core** (`core/src/contrato/ports.rs`): `escribir(id, bytes) ->
   Result<usize>` y `salida(id) -> Result<TranscriptConsola>` con default
   NoSoportado (mocks de `comando.rs` no se tocan). Nuevo
   `TranscriptConsola { id_ejecucion, comando, viva, codigo_salida,
   lineas: Vec<ChunkConsola> }` junto a `InfoConsola`.
2. **Ejecutor** (`cli/src/infra/ejecutor.rs`): `ConsolaViva` +=
   `stdin: Mutex<Option<ChildStdin>>`; fondo spawn con `stdin(piped)` y
   `take()` al registrar la viva (síncrona queda `null`: sin interact).
   `escribir`: tope 64 KB (`MAX_ESCRITURA_STDIN`), `write_all+flush`,
   BrokenPipe → `NoEncontrado`. `salida`: viva = volcado del anillo;
   archivada = líneas de `resultados` (flujo stdout, merged); tope 2000
   líneas. Tests: escribir→Ok(n) + tras matar→Err (sin eco: no hay `cat`
   en Windows; comando `Start-Sleep`); salida de archivada tras fin.
3. **Web** (`web_datos/conversaciones.rs` + rutas en `web/mod.rs` + tests
   espejo de los de matar): `GET consolas` (lista), `GET consolas/{eid}/
   salida` (transcript), `POST consolas/{eid}/escribir {texto}` (`{ok,
   escritos}`; id vacío/texto vacío/>64KB → `peticion_invalida`;
   desconocida → error, NO idempotente: la UI debe avisar).
4. **Tauri** (`desktop/src-tauri/.../conversaciones.rs` + registro en
   `main.rs:510`): `consola_lista`, `consola_salida`, `consola_escribir`
   (mismo patrón que `consola_matar`: trim id, sesión con ejecutor).
5. **Transporte UI** (`realTipos.ts` + `api.ts` + `transporteTauri.ts` +
   `adaptadorReal.ts`): `escribirConsola(id, texto): Promise<number>`,
   `listarConsolas()`, `leerSalidaConsola(id)` (tipos visibles mínimos).
6. **Panel** (`panelConsola.ts` + `consola.css` + `panelDerecho.ts`
   orquestador): `.consola-lista` → sub-barra horizontal de chips (marca +
   comando corto, click activa); vacío → frame listo (`$` + "lista para la
   primera ejecución" + nota non-TTY reformulada, corta); input
   `consola-entrada` (Enter envía línea+"\n", solo viva, si no
   `deshabilitado` + title honesto); `sincronizar()` al abrir la tab:
   lista→merge sin duplicar + salida→backfill + estados; error de
   escritura → `onError` (toast).
7. **Verificación**: lease cargo + `cargo test --workspace --lib` + clippy;
   `tsc` + build; VarSense 0 nuevos; DOM en vivo (sub-barra, frame listo,
   input deshabilitado sin viva; stdin real cubierto por tests backend —
   un turno en vivo no es disparable desde el harness).

## Definition of Done
- Sub-barra + frame listo + input visibles según estado; escribir en viva
  llega al proceso (test); backfill recupera anillo al abrir tarde;
  errores (desconocida, >64KB, transitoria) con mensaje, sin silent;
  grep clases viejas = 0; commit `219A-3` con roadmap/completada.

## Estado
- Implementada F1-F7 el 21-09 (código + gate + funcional). CERRADO con
  commit 21-09 por decisión del usuario: commit de todo el árbol (219A-3 +
  trabajo olvidado). Plan CERRADO.
- F1 contrato: `TranscriptConsola` + `escribir`/`salida` (default
  `NoEncontrado`) en `core/src/contrato/ports.rs`; export en `core/src/lib.rs`.
- F2 ejecutor: `stdin(piped)` + `take()` en `ejecutar_fondo_con_id`; anillo
  `VecDeque<ChunkConsola>` (con flujo); `escribir` (tope 64 KB) + `salida`
  (tope 2000 líneas); test `escribir_acepta_en_viva_y_falla_tras_matar`.
- F3 web: `listar_consolas` / `salida_consola` / `escribir_consola` +
  `EscribirConsola` en `web_datos/conversaciones.rs`; 3 rutas en `web/mod.rs`;
  re-export en `web_datos/mod.rs`.
- F4 Tauri: `Info/Linea/TranscriptConsolaTauri` + `consolas_listar` /
  `consola_salida` / `consola_escribir` en `chat/conversaciones.rs`;
  registro en `main.rs`.
- F5 transporte: tipos + métodos en `realTipos.ts`, `transporteTauri.ts`,
  `api.ts` (HTTP), `adaptadorReal.ts`.
- F6 panel: reescritura `panelConsola.ts` (sub-barra, `sincronizar`,
  `cargarTranscript` + `pendientes`, `acotar`, stdin Enter, frame listo);
  estilos stdin en `consola.css`; puentes + `sincronizar()` al abrir en
  `panelDerecho.ts`.
- F7 evidencia: `sentinel check 219A-3 --stages scripts/quality/stages.json`
  → PASS (541 tests ok, 0 fallidos); `npm run type-check` + `npm run build`
  verdes; funcional en vivo (`web :8799`: `GET /consolas` → `{ok:true,
  consolas:[]}`, salida/escribir desconocida → `no_encontrado`). Sin
  positivo vivo (un turno no es disparable por API; cubierto por test Rust).
  Proceso `glory-harness` colgado (PID 16092, 20-09) bloqueaba el link del
  gate (`Acceso denegado` al relinkar `debug\glory-harness.exe`): detenido
  antes del PASS. Warnings `funcion-larga` en `ejecutar_fondo_con_id`
  (129 líneas) y `comando.rs:ejecutar` (115) son preexistentes/ajenos.
- Desvíos del plan: comandos Tauri `consolas_listar` (plural, no
  `consola_lista`); comando de la entrada rescatada lo corrige
  `sincronizar` (el backend sabe el verbatim); `revelar()` también pide
  backfill; sin clase `.consola-lista` nueva (se reutiliza la lista).

## Dependencia de commit (21-09, bloquea el cierre)
- 219A-3 NO compila sola sobre HEAD: mis handlers usan
  `comun.ejecutor` (`SesionComun.ejecucion`), campo que existe solo en el
  árbol (hunk [209A-1 F4-resto] sin commitear en `cli/src/servicio/sesion.rs`
  + cableado `harness.ejecutor_comando`). `lista/matar` SÍ están en HEAD.
- El árbol además trae trabajo ajeno sin commitear (resto F4: reap al
  eliminar/cerrar, ruta+comando `matar`; resume de sesión por cookie;
  `cambiosListar` web; `UsoTurnoRecuperado`; refactors daemon/run/llm/
  runtime/mensajes/sidebar). Mi commit debe excluirlo (stage por hunk).
- INTENTO 21-09 (verificado, sin commit): script `stage_219a3.py` (11
  ficheros filtrados + 7 íntegros) + verificación con `stash --keep-index`.
  Dos hallazgos: (1) HEAD está rojo — `main.ts` no satisface
  `GanchosDeps/DepsCrearPanelCtx/NavegadorVistaDeps`; el cableado vive en
  WIP ajeno (`main.ts` 179A-2/129A-8 → frente Cambios 129A-7/129A-8 con
  `PanelDerechoTodo.cambios` + navegador `abrirPorAgente/
  notificarTurnoInicio`); incluirlo mezclaría un frente entero. (2) Rust
  igual: `harness.ejecutor_comando` no existe en HEAD (lo define el WIP de
  `run.rs`/`daemon.rs`). Un commit solo-219A-3 sería rojo en ambos lados.
  Decisión final del usuario 21-09: commitear TODO (trabajo olvidado).
  `type-check` verde en árbol completo + gate PASS previo (541 tests) sobre
  el mismo código. Basura `prueba.txt`/`prueba-2.txt` borrada;
  `.glory-harness/backups/` excluido (datos runtime).
  Lección: el PASS estático del patch no basta — `git apply` coloca
  inserciones puras según `+start` (hubo que recalcular `new_s = old_s +
  delta_prev + 1`); y nunca `git reset` antes de `git stash pop`.
