# Plan 209A-1 — Consola lateral: visor de ejecuciones en vivo con tope anti-fuga (2026-09-20)

## Objetivo
Tab "Consola" en el panel derecho con panel interno de gestión de varias
ejecuciones: cada `comando` del agente abre una consola con su salida por
streaming en vivo. Sin PTY, sin shell interactivo, sin stdin. El foco es
que sea imposible dejar consolas de fondo comiendo RAM por descuido.

## Decisiones previas
- **089A-9** (08-09) difirió watcher nativo y terminal PTY por decisión de
  MVP; esta tarea es la reapertura prevista, acotada a la vía eficiente.
- **20-09, usuario**: "lo que sea más eficiente" → visor de ejecuciones en
  vivo, no PTY (`portable-pty` + stdin + resize quedan fuera).
- Solo el backend **CLI** ejecuta comandos hoy (`EjecutorCliente`,
  `cli/src/infra/ejecutor.rs`); el backend Tauri no tiene ejecutor
  (fail-closed). La consola vive sobre el ejecutor CLI.

## Alcance / no alcance
- SÍ: streaming de salida de `comando` (síncrono y fondo) a consolas en tab
  lateral; varias consolas; matar/cerrar/limpiar; topes de recursos;
  **comando literal completo visible** (como VSCode); **botón abrir terminal**
  por ejecución; **botón segundo plano** (desacoplar sin matar); el agente
  ejecuta en fondo y ve qué hay corriendo; **cerrar conversación o app mata
  todas sus ejecuciones**.
- NO: PTY, stdin interactivo, persistencia de transcripts entre sesiones,
  consola en el backend Tauri (llegará cuando tenga ejecutor), cambios en
  jaula/riesgo/aprobación/resumen del turno.

## Diseño eficiente (topes anti-fuga, innegociables)
1. **`MAX_CONSOLAS_VIVAS = 4`** (constante en el ejecutor, testeable): nueva
   ejecución con tope alcanzado → error explícito ("hay 4 consolas
   corriendo; mata o cierra una"). Bloquear, nunca matar silencioso ni
   permitir ilimitadas.
2. **Ring buffer 128 KB por consola viva**; transcript de finalizada
   truncado a 32 KB (reutilizar `truncar` de `ejecutor.rs`). La RAM por
   consola queda acotada por construcción, no por disciplina.
3. **Cerrar consola viva = matar + reap + conservar transcript acotado.**
   Sin zombis: el handle `Child` se libera al salir/matar (patrón actual
   `HandleTarea`), y el cierre UI invoca `matar` (`comando_matar` ya existe).
4. **`stdin` a null** en ejecuciones de consola: un comando que pida input
   falla rápido en vez de colgar hasta timeout/matar.
5. **Push por eventos, cero polling desde la UI**: el bombeo stdout/stderr
   emite chunks por el canal de eventos del turno (SSE en web); `comando_status`
   queda solo como fallback del modelo. Sin timers ni `setInterval`.
6. **Reap global + visibilidad**: al cerrar sesión/app se matan todas las
   vivas; el panel interno muestra por consola estado (● corriendo / ✓ fin
   + código / ✕ matada), tamaño de buffer y contador de vivas.
7. **Comando verbatim como VSCode**: el resultado guarda el comando literal
   completo (campo nuevo `comando`, sin el recorte a 60 del resumen); la
   consola lo muestra tal cual se ejecutó en su cabecera, y la salida exacta
   debajo. La línea de resumen del turno sigue corta; el detalle y la consola
   llevan el literal.
8. **Segundo plano = desacoplar sin matar**: nueva operación
   `desacoplar(id_ejecucion)` — una ejecución síncrona en curso pasa a fondo
   (conserva proceso, buffer y suscriptores; libera el turno) y devuelve su
   id. Botón "segundo plano" en la fila del resumen y en la consola. El
   agente ya tiene `fondo`/`comando_status`/`comando_matar`; se suma
   `comando_lista` (en ejecución + recientes con su `id_ejecucion`) para que
   vea qué hay corriendo sin adivinar ids.
9. **Ámbito por conversación**: cada ejecución lleva `conversacion_id`;
   cerrar la conversación mata sus ejecuciones vivas (reap por ámbito,
   además del global al cerrar app). Cerrar app = matar todo, verificado.

## Fases verificables
- **F1 — Streaming backend** (`cli/src/infra/ejecutor.rs`): cada `ejecutar`
  genera `id_ejecucion` (también síncronas), guarda comando literal +
  `conversacion_id`, tarea tokio que bombea stdout+stderr por líneas a
  suscriptores broadcast + buffer final; exponer chunks por el canal de
  eventos web existente (verificar cuál: SSE del modo web). Timeout 120 s
  intacto para síncronas; fondo sin timeout bajo tope nº1.
- **F2 — Topes + fondo operable** (misma área): `MAX_CONSOLAS_VIVAS`,
  ring 128 KB, transcript 32 KB, `stdin(null)`, `desacoplar`, `comando_lista`,
  reap por conversación + global. Tests: tope bloquea la 5ª, matar libera
  plaza, buffer nunca supera el tope, desacoplar conserva el proceso y libera
  el turno, cerrar conversación/app mata todo.
- **F3 — UI tab Consola** (`desktop/ui`): `abrirTab('consola', …)` como
  Files; panel interno = lista de consolas (estado + código + KB) + visor
  de la activa con últimas ~500 líneas renderizadas ("cargar más" bajo
  demanda); botones matar / cerrar / limpiar / copiar; badge con nº vivas;
  **botón "abrir terminal" en cada fila de comando del resumen** (abre su
  consola) y **botón "segundo plano"** (desacopla la ejecución en curso).
  Estilos en `estilos/consola.css`, monocromo estricto, sin radius.
- **F4 — Cableado agente→consola**: toda tool `comando` crea/actualiza su
  consola (auto-apertura de la tab configurable, defecto: abrir); el agente
  usa `fondo` para lo largo, `comando_lista` para ver qué corre y
  `comando_matar` para limpiar; el resumen `Comando [nivel] …` no cambia.
- **F5 — Verificación y gate**: `type-check` + `build` + `cargo test`
  (nuevos tests F2 en verde) + E2E real en `:8799` (comando literal visible
  íntegro, streaming visible, abrir-terminal desde la fila, segundo plano a
  mitad sin perder salida, 5ª ejecución bloqueada, cierre de conversación y
  de app → cero procesos verificados por OS) + gate Sentinel PASS.

## Límites honestos
- Sin stdin ni PTY: programas que detectan no-TTY cambian formato (sin
  color/barras); se acepta.
- Nietos detached en Windows pueden sobrevivir al `matar` (kill solo hijo
  directo): documentar en la consola, no prometer kill-tree.

## Definition of Done
E2E F5 en verde + gate PASS + roadmap actualizado + evidencia en
`Agente/completados/tareas-2026-09-20.md`. Sin transcript persistido entre
sesiones; sin proceso huérfano tras cerrar conversación o app (verificado
por OS); comando literal íntegro visible en cada consola.

## Estado
F1 cerrado 20-09 noche: eventos `ConsolaInicio/Chunk/Fin` en el contrato,
`ToolResult.consola_id` + `ResultadoEjecucionComando{comando,id_ejecucion}`,
`AgentToolContext{conversacion_id,tx_eventos}` threadado hasta `ejecutar_tool`
(subagente pasa `nil`), `ToolComando` genera el id y reenvía chunks por el
canal del turno (sync emite Fin como `evento_extra`; fondo deja el reenvío
vivo), `EjecutorCliente.ejecutar_en_vivo` con `bombear` por líneas (64 KB
stream / 2 KB por línea). `cargo check --workspace` + `cargo test
--workspace --lib` (512 passed) + `clippy --workspace --all-targets -D
warnings` en verde.
F2 cerrado 20-09 noche (backend): `ConsolaViva{comando,conversacion_id,inicio,
suelta,anillo}` + `vivas: HashMap<id,Arc<ConsolaViva>>` en `EjecutorCliente`;
`MAX_CONSOLAS_VIVAS=4` (la 5ª → `Error::Limite`, hijo matado al nacer, sin
zombis); ring 128 KB con descarte de antiguas + contador; `stdin(null)` en
ambas ramas; `desacoplar(id)` marca `suelta` (el pump deja de enviar al turno
pero sigue acumulando + anillo; `NoEncontrado` si no es viva); `lista()` =
vivas (con comando real + bytes anillo) + recientes archivadas (poda
best-effort a 64); el pump reapea al salir (retira viva + archiva); `estado`
de viva informa el comando real; `ToolComando` pasa `ctx.conversacion_id` y
convierte `Limite` en `ok:false` accionable (apunta a `comando_lista`);
nueva tool `comando_lista` (VIVA/hecha/sin consolas) registrada junto a las
otras. Desviaciones honestas: transcript archivado = truncado 8 KB existente
(no 32 KB: el `salida` de `ResultadoEjecucionComando` mantiene su contrato);
`lista()` de archivadas reporta `conversacion_id=nil` (el reap por ámbito de
F4 operará sobre vivas, que sí lo portan); orden de firma
`(id,comando,conversacion_id,fondo,chunks)`. Tests: 4 nuevos en cli
(viva visible + reapea, desacoplar no mata, tope rechaza 5ª, id desconocido)
+ 3 en core (lista vivas/hechas, lista vacía, tope→ok:false) + registro con
`comando_lista`. `cargo test --workspace --lib` 519 passed (173 cli + 346
core) + `clippy -D warnings` en verde. Próximo paso: F3 (UI tab Consola).
F3.1 cerrado 20-09 noche (exploración UI: `abrirTab`, SSE→`aplicarEvento`,
`onMostrarArchivo`, supresión F1, `EjecutorCliente` en desktop vía
`SesionComun`).
F3.2 cerrado 20-09 noche (tab Consola + auto-apertura + stream en vivo):
`realTipos.ts` gana `consola_inicio/chunk/fin` + `tool_result.consola_id` +
hook `onConsolaEvento`; `aplicarEventos.ts` lo deriva al hook (el chat no
pinta salida); nuevo `componentes/panelConsola.ts` (store por `id_ejecucion`,
lista + visor push-only, comando verbatim por `textContent`, stderr con
patrón, tope de vista 1000 líneas con contador de descartadas, auto-scroll
solo al fondo, limpiar-solo-terminadas, copiar transcript, nota sin-stdin-ni-PTY)
+ `estilos/consola.css` (monocromo, tokens); `componentes/panelDerecho.ts`
gana opción 'consola' (inicio + menú +); `orquestador/panelDerecho.ts` monta
la pieza, `abrirConsola/abrirConsolaEn/abrirConsolaPorAgente/onConsolaEvento/
notificarTurnoInicioConsola` con la misma máquina `crearSupresionNavegador`
que F1 (manual abre→olvida, × a mitad de turno→suprime, turno nuevo→reinicia),
restore + persistencia; `ganchos.ts` `reflejarConsola` + toasts
abierta/suprimida; `main.ts` cablea hook, `turnoEnCurso` y `alIniciarTurno`.
`npm run type-check` + `npm run build` en verde. Próximo: F3.3 (enlaces
fila/resumen por `consola_id` + botones lista/matar/desacoplar) y F4 (reap).
F3.3 cerrado 20-09 noche (enlace fila→Consola): `HerramientaViva` gana
`agregarAccion(etiqueta,icono,onClick)` (receta `aviso-accion`
generalizada en `mensajes.css` como `.herramienta-accion`; no abre el
<details>); `aplicarEventos.ts` añade "ver en Consola" (icono `terminal`)
a la fila cuando `tool==='comando'` y hay `tool_result.consola_id`;
hook `onVerConsola(id)` en `realTipos.ts` → `verConsolaEn` en
`GanchosDeps`/`ganchos.ts` → `main.ts` lo cablea a
`todoPanelDerecho.abrirConsolaEn(id)` (gesto de usuario, sin toast).
Desviaciones honestas: el enlace es SOLO en vivo (el historial estático
no conserva `consola_id`; la tab sí es durable); el resumen de fin de
turno no enlaza (cubre solo escrituras vía `cambiosResumen`, sin filas
de comando: añadirlas cambiaría su contrato, fuera de alcance); NO hay
botón "abrir terminal" externo ni "segundo plano" (ver F4-resto).
`npm run type-check` + `npm run build` en verde.
F4-backend cerrado 20-09 noche: `EjecutorCliente::matar_por_conversacion
(conv)->usize` (solo las vivas de esa conversación) +
`matar_todas()->usize` (global, idempotente: en vacío 0 sin error) vía
helper `matar_handle`; `kill_on_drop(true)` en ambos spawns (cancelar el
turno a mitad de `wait()` ya no deja huérfano); HALLAZGO Y FIX: `matar`
tenía carrera real (si el pump ya hizo `take` del `Child`, el handle
queda en `None` y el kill era no-op silencioso: mis 2 tests nuevos lo
demostraron 1-vs-2): `ConsolaViva` gana `matar: Notify` y el pump hace
`select!{ wait | notified→kill+wait }` (el poseedor mata). Tests nuevos:
reap-por-conversación-solo-las-suyas + matar-todas-vacía-e-idempotente.
`cargo test --workspace --lib` 521 passed (175 cli + 346 core) +
`clippy --workspace --all-targets -D warnings` en verde.
F4-RESTO CERRADO 21-09: retención del executor (`HarnessCli.ejecutor_comando`
y `SesionComun.ejecutor` en cli; `Sesion.ejecutor` en daemon; `comun.ejecutor`
en desktop vía `construir_harness_con`): eliminar conversación (desktop +
web) y eliminar-conversaciones-proyecto (web) llaman a
`matar_por_conversacion`; cerrar sesión (web + daemon) y cierre de app
(Tauri `on_window_event` + web `apagado_ordenado` con Ctrl+C + daemon
`ctrl_c`) llaman a `matar_todas`; archivar NO reapea (reversible, sin ids).
Canal UI→backend NUEVO: `consola_matar(id)->bool` (Tauri, idempotente) +
`POST /api/v1/session/:id/consolas/:eid/matar` (web) → `Transporte.
consolaMatar` (Tauri + web) → `sesion.matarConsola` → × `x-circulo` en la
cabecera de la tab (solo habilitada con activa viva; la vista NO retira:
el `consola_fin` la congela). VERIFICADO 21-09: `cargo check --workspace
--all-targets` + `cargo test --workspace --lib` 523 passed (177 cli con 2
nuevos `matar_consola_*`, 346 core) + `clippy --workspace --all-targets
-D warnings` + `npm run type-check` + `npm run build` en verde. Tests
nuevos: id-desconocido → `{ok:true, matada:false}`; viva real (ping 60 s)
→ `{ok:true, matada:true}` + proceso muerto. PENDIENTE solo F5 E2E real
en `:8799` (turno vivo con LLM + verificación de procesos por OS).

## Correcciones al diseño (lo que el código desmintió)
- Diseño-3 "el cierre UI invoca `matar`": FALSO hasta F4-resto (no había
  canal UI→backend); CERRADO 21-09 con `consola_matar` (Tauri) + endpoint
  web + × por entrada viva. Las primitivas ya estaban testeadas (F4-backend).
- Diseño-8 "desacoplar una síncrona libera el turno" NO existe:
  `desacoplar` solo marca `suelta` en vivas (= solo `fondo=true`); las
  síncronas no son vivas (no están en `lista()`, `comando_matar` no las
  alcanza: solo timeout 120 s o cancelación con `kill_on_drop`). No hay
  tool `comando_desacoplar` ni botón "segundo plano".
- Diseño-9 "cerrar conversación/app mata todo": era FALSO hasta F4-resto
  (primitivas sin cablear); CERRADO 21-09 (ver Estado). Excepción honesta:
  archivar NO reapea (reversible hilo a hilo, sin ids).
- Preexistente (318A, fuera de alcance): `comando_status` de id
  desconocido devuelve `ok:true` con "(tarea de fondo desconocida)",
  no error, pese a lo que dice su descripción.
