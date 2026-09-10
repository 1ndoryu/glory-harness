# Plan 109A-4 — Comandos `/` estilo VS Code (`/compactar`, `/meta`)

> ID roadmap: **109A-4** · Fecha: 2026-09-10 · Estado: **CERRADO 10-09** (F1–F4
> HECHAS; evidencia en `Agente/completados/tareas-2026-09-10.md`). Gate canónico
> con `coverage`/`sentinel` PASS en el baseline de 10 warnings preexistentes y el
> único error ajeno `sccache-no-configurado`.
> Origen: petición usuario — menú `/` como VS Code; `/compactar`; `meta` pasa de
> modo de ejecución a comando; luego relevar comandos útiles en las referencias.

## 1. Estado actual (verificado)

- Core ya define comandos slash como markdown (`core/src/herramientas/skill.rs:1-98`,
  patrón Claude/opencode): `.md` con `tipo: comando` + `comando: <nombre>`,
  plantilla con `$ARGUMENTOS` y `@archivo`, expansión pura y determinista
  (sin I/O ni LLM). Hay descubrimiento en directorio (`:100`).
- UI con menú `/` desde F2 (`componentes/menuComandos.ts` + `entradaComandos.ts`;
  el textarea mantiene `autocomplete='off'` a propósito).
- `meta` hoy es **modo global**: `permiso_por_modo("meta")`
  (`politica/permiso.rs:188`), `turno_config.modo == "meta"`
  (`nucleo/runtime/turno/permisos.rs:123`), segmento en `OPCIONES_EJECUCION`
  (`dominio/opciones.ts:82-93`).
- Compactación solo automática (`nucleo/context.rs`); hook previo en 109A-1.

## 2. Fases

### F1 — Relevamiento en referencias (HECHO 10-09, catálogo v1 cerrado)

Relevadas las 5 referencias en `data/referencias-cli/` (gitignored):

| Ref | Modelo de comando | Catálogo relevante | Lección para GH |
|-----|-------------------|--------------------|-----------------|
| VS Code (`vscode/src/vs/workbench/contrib/chat/`) | `IChatAgentCommand` por agente (`common/participants/chatAgents.ts:123`), + `IChatSlashCommandService`; `/compact` (`browser/actions/chatActions.ts:1127-1147`) + hook `PreCompact` (`common/promptSyntax/hookTypes.ts:18,98-100`) | `/compact` + comandos por agente | Comandos con ámbito por agente, no globales; `PreCompact` ya cubierto por 109A-1 |
| opencode (`opencode/packages/opencode/src/command/index.ts:22-44,70-88`) | `Info` (name/description/agent/model/source `command\|mcp\|skill`/template/subtask/hints) + `hints()` (`$ARGUMENTS`, `$\d+`); builtins `init`/`review` (review con `subtask:true`) + config + prompts MCP + skills | `init` (“guided AGENTS.md setup”), `review … defaults to uncommitted”` (subtarea) | Mapea a `ComandoSlash` markdown existente (`skill.rs:39-43`); `hints`/`subtask` se añaden tras v1 |
| claurst (`claurst/src-rust/crates/commands/src/lib.rs:113-132`, `tui/.../app/commands.rs:102-109`) | trait `SlashCommand` (name/aliases/description/help/hidden/execute→`CommandResult`) + ~100 impls; `intercept_*` (pantallas UI) vs `execute` (lógica); `/help` agrupado por categorías (`:560-572`); `/compact` pide resumen con instrucción (`:628-648`) | help/clear+new/compact/model/context/cost/diff+review/init/memory/skills/btw + 90 más | Forma del trait a portar a Rust core; UI solo filtra/inserta, ejecución siempre en core (único camino, testeable headless) |
| grok-cli (`grok-cli/src/ui/slash-menu.ts:1-27,29-64`) | `SlashMenuItem` (id/label/description/aliases) + `filterSlashMenuItems` (score exacto > prefijo > contiene, comando antes que descripción) | exit/help/agents/mcp/models/new/commit/review/verify/skills/btw/update… | Algoritmo de filtrado del menú F2 |
| hermes (`hermes-agent/acp_adapter/commands.py:44-66,88-95`) | `_COMMANDS` (name→help/desc/hint) + `_cmd_<name>`; desconocido → `None` (pasa al LLM); `/compress` manual aun con auto off (`:228-267`); `/context` muestra presión (`:161-213`) | help/model/tools/context/reset/compress/steer/queue/version | GH NO hereda el fall-through al LLM: comando desconocido → error explícito + `/ayuda` (prohibido silencio) |

Catálogo v1 CERRADO (8 comandos, nombres en español como la UI actual):

- `/ayuda [comando]` — lista agrupada + detalle (todos la tienen).
- `/modelo [id]` — ver/cambiar modelo (claurst+hermes+grok).
- `/compactar [instrucción]` — compactación bajo demanda (F3; claurst+hermes+vscode).
- `/contexto` — presión de contexto + umbral (hermes `/context`, claurst `/context`+`/cost`).
- `/limpiar` — vaciar conversación (claurst `/clear`, hermes `/reset`); `new` no aplica (GH create-on-write 069A-7).
- `/revisar [args]` — revisar cambios no commiteados por defecto (opencode `/review`, claurst `/review`+`/diff`).
- `/iniciar` — setup guiado `AGENTS.md` del proyecto (opencode `/init`, claurst `/init`).
- `/meta <texto>` — un turno solo-lectura como override (F4; específico GH, sin precedente en refs).

Diferidos a hijas (fuera de v1): gestión de skills, `btw` lateral, `steer`/`queue`
(turnos concurrentes que GH no tiene), commit/pr, MCP, agents, pantallas
config/theme, checkpoints/rewind (P2 ya existe por UI), coste detallado,
voz, teleport/remoto.

### F2 — Menú `/` en la entrada (HECHO 10-09)

Implementado:

- **Datos** `dominio/comandosSlash.ts` (puro, sin DOM): 8 builtins del catálogo v1
  con categoría/ayuda/uso, `filtrarComandos` (exacto > prefijo > nombre > resumen,
  desempate `localeCompare`), `partirComando`, `consultaMenu`, `catalogoTexto`.
- **Menú** `componentes/menuComandos.ts` + `estilos/menuComandos.css`: flotante,
  sin robar foco (`mousedown` con `preventDefault`), filas `role="option"`,
  posicionamiento con `anchoVentana/altoVentana`, cierre por Escape/fuera/resize/blur.
- **Disparador** `componentes/entradaComandos.ts`: abre con `/` al inicio del
  textarea, filtra al teclear, cierra con espacio (paso a argumentos).
- **Ejecutor** `componentes/panelChatComandos.ts`: `/ayuda`, `/modelo`, `/contexto`,
  `/limpiar`, `/revisar`, `/iniciar` y comandos markdown del área; `/compactar`
  (activado después en F3) y `/meta` responden explícitamente «aún no está activo
  (llega en la fase siguiente del plan 109A-4)»; desconocido → `comando
  desconocido «/x»; prueba /ayuda`.
  Sin fall-through al LLM.
- **Comandos del área**: `comandos_listar`/`comando_expandir` (Tauri, ahora en
  `desktop/src-tauri/src/comandos/mod.rs`) reutilizan `glory_harness_core::skill`;
  en modo web se declaran ausentes con `util/capacidad.ts` (error tipado, sin
  ruido en consola).

Evidencia (10-09):

- `npm run type-check` limpio; `npm run build` EXIT 0 (96 módulos,
  `index-*.js` 155.86 kB, `index-*.css` 47.66 kB).
- `cargo clippy -p glory-harness-desktop --all-targets -- -D warnings` limpio.
- E2E real en navegador (`localhost:8760`): menú con los 8 builtins; filtro `/co`
  devuelve prefijos antes que coincidencias por nombre; ↑↓ mueven la selección;
  Escape cierra; Enter ejecuta; `/ayuda` imprime el catálogo agrupado;
  `/modelo` lista proveedores/modelos y `/modelo zzz` falla con mensaje explícito;
  `/contexto` sin datos avisa en vez de mentir; `/limpiar` vacía el panel y deja
  «Nueva conversación»; `/noexiste` → «comando desconocido».
- Bug real encontrado y corregido en el E2E: el menú no llamaba a `preventDefault`,
  así que Enter llegaba al textarea e insertaba salto de línea en vez de ejecutar
  (`menuComandos.alTecla`).
- Gate canónico (`check 109A-4`): `coverage` PASS, `sentinel` PASS — 0 errores y
  los 10 warnings preexistentes (8 `console-production` + 2 `limite-lineas`);
  `sccache` FAIL por `sccache-no-configurado`, deuda **ajena** ya registrada en el
  roadmap («Gate: etapa Rust y sccache»).
- Nota de arquitectura: el módulo Tauri pasó a `comandos/mod.rs` porque el nuevo
  archivo plano dejaba `src-tauri/src/` en 11 archivos (>10) y activaba
  `directorio-abarrotado`; la reorganización por dominio de ese directorio queda
  como deuda en 109A-6.

### F3 — `/compactar` (HECHO 10-09)

Compactación bajo demanda con el MISMO gestor de contexto y los mismos ganchos
que la pasada automática, más un **punto de compactación persistido** para que el
turno siguiente arranque del resumen en vez de reenviar el historial entero:

- **Core** `nucleo/context.rs`: `CompactarResultado.resumen_texto` (el consumidor
  necesita el texto para persistirlo) y `compactar_forzado(...)`, que ignora
  umbral y ventana de seguridad porque lo pide el usuario pero conserva el único
  invariante útil — `hay_material(...)`, material nuevo y tramo resumible fuera
  de la cola verbatim — para no «compactar» en balde. Automática y manual
  comparten `aplicar_compactacion`, así que no pueden divergir en ahorro ni en
  contadores.
- **Runtime** `nucleo/runtime/turno/mod.rs`: `compactar_manual(&[AiMessage],
  instruccion)` devuelve `CompactarManual { compactado, motivo, tokens_antes,
  tokens_despues, ahorro_pct, ocupacion_pct, tramos, resumen }`; dispara
  `PreCompact` (veto = no se toca nada) antes y `PostCompact` después, y reutiliza
  `limite_resumen_chars` para acotar el resumen del gancho.
- **Persistencia**: columnas `conversaciones.compactado_en` + `resumen_compactado`
  (migración idempotente `ALTER TABLE`) con `conversacion_compactar` /
  `conversacion_compactacion` (ownership por usuario; resumen en blanco = ausencia).
- **Servicio** `SesionComun::compactar_conversacion`: compacta el historial real
  de la conversación y persiste el punto; `preparar_turno` envía
  `[resumen] + mensajes posteriores a la marca` cuando hay punto. Los mensajes NO
  se borran: historial visible, rewind y auditoría siguen completos.
- **Escritorio**: comando Tauri `compactar_conversacion` (mismo guard de turno en
  curso que `cargar_conversacion`), expuesto en `Transporte`; en modo web se
  declara ausente (`adaptadores/apiCompactar.ts`), sin simular ahorro.
- **UI** `panelChatComandos.ts`: `/compactar [instrucción]` informa del ahorro real
  («contexto compactado: A → B tokens (~N% menos)») o del motivo del no-op.

Evidencia (10-09):

- `cargo test -p glory-harness-core -p glory-harness`: 122 + 281 verdes, incluidos
  los `forzado_*` del core y `compactar_conversacion_persiste_el_punto_y_el_turno_arranca_del_resumen`,
  `compactar_conversacion_sin_material_no_persiste`, `compactacion_round_trip_y_ownership`
  y `migracion_anade_columnas_de_compactacion` (BD con esquema anterior → migra).
- Bug real encontrado por los tests: `PersistenciaSqlite` guarda `creado_en` con
  precisión de SEGUNDOS, así que un punto con nanosegundos descartaba los mensajes
  del mismo segundo. El punto se guarda con `SecondsFormat::Secs` y el filtro usa
  `>=`: duplicar algo ya resumido es inofensivo, perder un turno no lo es.
- `cargo clippy -p glory-harness-desktop --all-targets -- -D warnings` limpio;
  `npm run type-check` limpio; `npm run build` EXIT 0 (97 módulos, `index-*.js`
  156.73 kB).
- E2E en navegador (modo web): `/compactar` falla con motivo explícito
  («no se pudo compactar: la compactación por demanda requiere la aplicación de
  escritorio») en vez de mostrar un ahorro que no ocurrió.
- Límite real de la verificación: el camino Tauri no se pudo ejercer en ventana
  (la infraestructura de tests del escritorio no monta un runtime Tauri); cubierto
  con los tests de servicio —mismo par de llamadas que hace el comando—, clippy y
  type-check.

Interacción documentada: un `rewind` posterior sigue funcionando; el punto no se
limpia porque el resumen cubre justamente el material anterior a la marca.

### F4 — `/meta <texto>` y retiro del modo (HECHO 10-09)

- **Núcleo**: override de modo POR TURNO. `AgentRuntime.modo_turno:
  Mutex<Option<String>>` + `GuardaModoTurno` (RAII: `Drop` limpia el override
  aunque el future se cancele o el turno falle) y `modo_efectivo()`. Todas las
  lecturas de política del turno pasan por ahí —esquemas de tools,
  `permiso_para_llamada`, subagente y plan store—, así que un `/meta` afecta a
  EXACTAMENTE un turno. `permiso_por_modo("meta")` se conserva como política
  interna del turno forzado. `ejecutar_turno_con_modo` recibe `PeticionTurno`
  (identidad + entrada + `modo_forzado`) para no añadir un séptimo argumento
  posicional (clippy) y dejar legible la llamada del transporte.
- **Servicio**: `preparar_turno_con_modo(..., modo_turno: Option<&str>)`; el
  prefijo `[META: …]` se decide con el modo EFECTIVO del turno, así que un
  `/meta` sin modo global sigue anteponiendo la meta vigente.
- **Transporte**: comando Tauri `enviar_turno(..., solo_lectura: Option<bool>)`
  —un booleano, no una cadena de modo: el transporte no elige política, solo
  pide «sin efectos»—; `preparar_paquete` usa el texto del comando como meta del
  turno. `Transporte.soportaSoloLectura()` (Tauri `true` / web `false`) hace que
  el modo web RECHACE `/meta` con motivo explícito en vez de simularlo.
- **UI**: `panelChatComandos` implementa `/meta <texto>`; el flag `soloLectura`
  viaja en `OpcionesTurno` hasta `enviar_turno` (el reenvío tras aprobar conserva
  `ultimaOpcion`, así que la política no se pierde). El texto se refleja en la
  fila de meta y se persiste como meta de la conversación (109A-5 F1), que es la
  que el backend antepone en los siguientes turnos solo-lectura.
- **Retiro del modo global**: `ModoEjecucion = 'predeterminado' | 'autonomo'`
  (`entradaTipos.ts`, `OPCIONES_EJECUCION` con nota que apunta a `/meta`);
  `VistaMetaDeps.getModo` desaparece y la fila de meta ya no depende del modo;
  `aplicarSesionGuardada` MIGRA `modo: meta` → `predeterminado`, reescribe la
  config y avisa UNA vez por ejecución (`vistaModal.migrarModoRetirado`).

Evidencia (10-09):

- `cargo test -p glory-harness-core -p glory-harness`: 124 + 283 verdes,
  incluidos `f4_modo_forzado_no_toca_el_modo_de_la_sesion` y
  `f4_modo_forzado_deniega_efectos_y_deja_leer` (deny con efecto, allow de
  lectura).
- `cargo clippy -p glory-harness-desktop --all-targets -- -D warnings` limpio;
  `npm run type-check` limpio; `npm run build` EXIT 0 (97 módulos,
  `index-*.js` 157.69 kB).
- E2E en navegador (modo web): el segmentado «Modo» ya solo ofrece
  «Predeterminado»/«Autónomo» con la nota nueva, y `/meta revisar el roadmap`
  falla con «el turno solo-lectura requiere la aplicación de escritorio /
  comando no ejecutado» en vez de correr un turno normal.
- Límites reales: el camino Tauri no se puede ejercer en ventana (la
  infraestructura de tests del escritorio no monta un runtime Tauri) —cubierto
  por los tests del núcleo, que consumen la misma política, y clippy—; la
  migración de config `meta → predeterminado` no se pudo reproducir contra el
  backend web (la instancia local devuelve 429 «demasiadas sesiones abiertas»),
  así que queda verificada por type-check y revisión de la rama.

## 3. No alcance

- Nuevos comandos más allá del catálogo v1 (van a tareas hijas). Skills intactas.

## 4. Definition of Done

`tsc` EXIT 0, `vite build` OK, clippy 0, tests verdes, gate PASS, E2E
(`/compactar` resume, `/meta` no muta, menú navegable por teclado), roadmap y
completada con evidencia.
