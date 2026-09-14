# Plan 139A-8 — Remediación de la auditoría integral (2026-09-13)

> **Estado: EN EJECUCIÓN (v5, 13-09).** "Adelante" recibido; árbol limpio
> (`c986dac` pusheado). **F0, F1, F2 y F3n completadas** (gates PASS abajo);
> siguiente F4. Preflight F3n detectó que F3–F6 (v1–v4) usaban etiquetas
> que NO corresponden a la auditoría e inventaban trabajo no pedido;
> el usuario ordenó **reconciliar y reescribir F3–F6** (ver § Decisiones.4).
> Fuente: `Agente/documentacion/auditoria-integral-2026-09-13.md` (v2 verificada).
> IDs S/R/K/G según la auditoría. Siguiente ID libre tras este: 139A-9.
> Fuente: `Agente/documentacion/auditoria-integral-2026-09-13.md` (v2 verificada).
> IDs S/R/K/G según la auditoría. Siguiente ID libre tras este: 139A-9.

## Decisiones (cerradas 13-09, v3)

1. **F4/S4 — rotura de API del puerto:** rotura dura, SIN shim de
   compat en runtime. El compilador la hace segura: todos los call sites que no
   compilen se corrigen en la misma sub-fase; `cargo build --workspace` verde es
   la prueba de cobertura total. Sin `#[deprecated]` a medias.
   (Corrección F0-F4: la v3 decía `cdp(URL)`, pero ese símbolo no existe en el
   código — `cdp(metodo, parametros)` en `ports.rs:591` — ni `attach_shell_por_defecto`
   (solo aparecía en este plan). El fix vigente es el de la auditoría §S4:
   subtraits `SoportaSkills`/`SoportaAmbitos` + detección de capacidad explícita;
   la rotura es igualmente dura: quitar los 2 defaults obliga a todos los
   implementadores a declarar capacidad y al curador a comprobarla.)
2. **F2 — método de medición p95:** test de integración
   `cli/tests/p95_turno.rs` (fixture vault con 5k mensajes, N=2 sesiones
   concurrentes, 50 iteraciones): mide bootstrap de turno + `listar_mensajes`,
   reporta p50/p95. Baseline = rama sin cambios; aceptación:
   `p95_después ≤ 0,5 × p95_antes`. Sin infra nueva: solo `std::time` + histograma
   manual en el test.
3. **F5/G1 — solape con 139A-7:** 139A-7 YA entregó batch (`workspace_git_resumen`),
   single-flight (`vueloUnico`) y pintar-solo-si-cambió (evidencia en
   `Agente/completados/tareas-2026-09-13.md:223-246`; gate pendiente, sin commit
   por decisión del usuario). 139A-8/F5 NO reimplementa: arranca re-ejecutando el
   fixture de 139A-7 como regresión y sigue solo con R5/G3/G2/K11. Si el gate de
   139A-7 sigue pendiente al arrancar F5, se cierra primero ese gate.
4. **Reconciliación F3–F6 (v5, 13-09, preflight F3n):** las fases F3–F6
   (v1–v4) quedan **invalidadas y reescritas** como F3n/F4/F5n/F6n/F7n:
   - Sus etiquetas K2/K7/K8/K10/G1/G2/G4 NO corresponden a la auditoría
     (audit K2 = herencia de entorno; K7 = MCP stdio; K10 = IPC Tauri;
     K8 = TOCTOU sandbox+`memoria.rs:417`; G1–G4/G8–G13 no son fases de
     este repo sino gaps de reglas Sentinel §4.3–§4.4).
   - Inventaban trabajo no pedido: rasgo `Redactor`/`SecretPattern`,
     etiquetas `secreto:`, fixture medidor, `agent.event` binario,
     normalización snake/camel, `core/src/estado/` (G11).
   - Dejaban fuera 5 ALTA (audit K2 entorno, K4 hooks exec, K5 jaula `cd`/cwd,
     K6 SSRF hooks, K7 MCP stdio) + R4 (índices) + R6/R8.
   - `sandbox.rs:96-132,335-378` YA canonicaliza (verificado por grep):
     queda solo el TOCTOU uso-vs-check, sin `OPENAT2` (Linux-only;
     binario Windows) — revalidación + doc del invariante.
   - S8 se implementa como dice la auditoría (`core::git::ServicioGit` +
     pasarelas), NO partiendo `git.rs` en 5 módulos.
   - F6n→F7n: la auditoría §5.6 dice "este repo solo sube el pin"
     (`glory-sentinel`): sin medidor ni reglas en este repo.
   - Cobertura v3 ("todos los CRÍTICO/ALTO tienen fase") queda invalidada;
     la cobertura real la da v5 (F3n = los 5 ALTA restantes).

## Objetivo (v5)

Cerrar los hallazgos CRÍTICO/ALTO restantes de la auditoría sin romper lo que
ya funciona: entorno/exec de hijos y hooks (K2/K4/K5/K6/K7, F3n), unificación
filesystem/git/path + TOCTOU (S7/S8/K8, F5n), SQLite bajo async e índices
(R1/R2/R3 hechos en F2; R4 en F5n), streaming/DOM (R5/R6/R8 [+R7 front
condicional], F6n) y puertos/god-objects (S3/S4/S5/S1/S2, F4). El gate de
reglas (§4.4) compete a `glory-sentinel`: este repo solo sube el pin (F7n).
Los MEDIOS/BAJOS entran solo si su fase los roza; el resto queda como deuda
en la auditoría.

## No alcance

- Reescrituras de UI/tema, nuevas features, cambios del protocolo SSE/daemon
  (el frame interno Tauri `agent.event` de F5/R5/G3 sí puede evolucionar a
  binario porque emisor y receptor se cambian en la misma fase).
- Mem0 local (S6): solo detrás de trait + flag; sin motor nuevo.
- `tauri dev` visual completo: se exige solo donde la fase toca el frente (G1–G3, R5).

## F0 — Re-resolución obligatoria (toda fase)

Antes de codificar cada fase: `grep` de cada línea citada (drift por 139A-1…139A-7),
`git status --short --branch`, y confirmación de que la cita [S] sigue vigente.
Si una cita cayó (código cambiado), se anota en la auditoría §6 y se re-prioriza.

## F1 — Seguridad de ejecución (K1, K13, K3, K9, K12)

1. **K1:** jaula PTY/shell para comandos del modelo en `cli/src/infra/ejecutor.rs`:
   `comando_para_shell`→ argv + `Command` directo, allowlist `SEGURAS_SIN_SHELL`
   ampliada, `denegar_por_riesgo` cubre `COMANDOS_PELIGROSOS`; tests de bypass
   (`$(`, backticks, `${`, `;|&`, `sh -c` anidado). DoD: ningún string del modelo
   llega a `sh -c`; suite de bypass verde.
2. **K13:** `daemon.rs:285` — redact del token + bind a loopback o socket local.
3. **K3:** modo web loopback con auth maestra optativa (`GH_MASTER_TOKEN`;
   sin token solo loopback + aviso; con token también admite red).
4. **K9:** cookie `gh_sesion` `SameSite=Lax` + secreto aleatorio por arranque
   (flag para fijarlo en daemon), anti-CSRF en mutaciones web.
5. **K12:** `tower_governor` por sesión/IP en `POST /turns` + tope de turnos
   concurrentes y coste (el body-limit ya existe — `web/mod.rs:542`).
   Gate: `sentinel check` + pruebas de bypass/timeout.

## F2 — SQLite bajo async (R1, R2, R3)

1. **R1/R2:** pool `r2d2`/`deadpool-sqlite` o `spawn_blocking` + canal para lecturas
   de `persistencia_sqlite.rs`; `PRAGMA busy_timeout` + reintento `SQLITE_BUSY`.
   Medir con `cli/tests/p95_turno.rs` (ver § Decisiones.2): baseline antes del
   cambio, comparación después.
2. **R3:** `listar_mensajes` con `WHERE creado_en>=?` + `LIMIT` paginado
   (conservar `>=` por precisión de segundos — `sesion.rs:498-503`), auto-nombre
   en el path de creación (`SELECT … WHERE id=?`). DoD: turno largo sin full-scan;
   test con 5k mensajes.

## F3n — Seguridad ALTA restante (K2, K4, K5, K6, K7)

Auditoría §K2/K4–K7 (13-09, ALTA). F0 previa confirmó: sin `env_clear` en
`cli/`, `sandbox.rs` ya canonicaliza, sin `Redactor`/`secreto:` (no se crean).

1. **K2 (entorno heredado):** `env_clear()` + allowlist mínima explícita en el
   punto único de spawn (`cli/src/infra/ejecutor.rs` + jaula F1) y en el
   cliente LLM (`core/src/nucleo/llm/red.rs:83`); allowlist documentada
   (PATH/SYSTEMROOT/TEMP-TMP/HOME-USERPROFILE + las estrictamente necesarias);
   las claves LLM solo viajan como variables nombradas. Tests: el hijo ve
   SOLO la allowlist (aserción sobre `env -0`/equivalente + clave señuelo
   ausente).
2. **K4 (hooks ejecutan binario arbitrario):** allowlist de binarios + tope
   de argv en `core/src/nucleo/hooks.rs` (dispatcher; el exec ya es directo
   sin shell —matiz v2—); fuera de allowlist → evento `bloqueado` + error
   tipado. Tests de bypass (ruta absoluta, `..`, extensión `.exe`/sin ext).
3. **K5 (jaula evadible con `cd`):** tras F1 (sin shell) verificar que el cwd
   del hijo queda anclado al workspace (`current_dir` explícito + denegar
   `cd` como comando); test de escape de cwd.
4. **K6 (SSRF vía hooks http):** allowlist de egreso en el cliente de hooks
   (`hooks.rs:389-412`): denegar loopback/link-local/metadata
   (`127.0.0.0/8`, `169.254.169.254`, `::1`) salvo allowlist explícita;
   timeout ya existe (se conserva). Tests de URL prohibida/permitida.
5. **K7 (MCP stdio sin allowlist):** allowlist de comandos + argv + env mínima
   en `core/src/herramientas/mcp.rs` (spawn); tests de comando fuera de lista.
   DoD: 5 suites verdes + `sentinel check` PASS; sin cambios de conducta salvo
   denegaciones documentadas (fail-closed).

## F4 — Puertos y god-objects (S3, S4, S5, S1, S2)

F0-F4 (verificado 13-09): las 5 citas [S] siguen vigentes (`tool.rs` 1354 lín.
con `AgentToolContext:33`/`AgentToolResult:73`/`AgentTool:147`/`AgentToolRegistry:169`;
`runtime/mod.rs` 1232 lín. con `::nuevo:261-322`; `ports.rs:281-369` S3 verbatim +
defaults S4 `:349-368`; `NavegadorPort:581-600` 9 métodos). Lo que cayó es la
paráfrasis de este plan (v1–v4), no la auditoría: valen los nombres de fix de la
auditoría. Inventario in-repo a tocar: 6 impls `AgentPersistence`
(`cli/infra/persistencia.rs` Memoria, `cli/persistencia_sqlite/puerto.rs` Sqlite,
`TiendaPrueba` en `memoria/soporte.rs`, `PersistenciaMock` en `contrato_tests.rs`,
`PersistenciaGrabadora` en `cron.rs:403`, `PersistenciaScheduler` en
`scheduler.rs:384`); consumidores S4 en `curador.rs:184` (`memoria_ambitos`) y
`:320` (`skills_registrar`) + test `:430-431` (`sin_registro`); adaptador S5 en
`desktop/.../navegador/puerto.rs` + tool en `core/herramientas/navegador/`
(`operaciones.rs`, `reflejo.rs`, `pruebas.rs`); consumidores `NavegadorPort` en
`cli/comandos/run.rs:24,65,84` y `tool.rs:15,68`. Orden: S3 → S4 → S5 → S1 → S2.

1. **S3:** segregar `PersistenciaTurnos` / `PersistenciaMemoria` /
   `PersistenciaSkills` / `ColaTareas` (+ `PersistenciaAuditoria`/`PersistenciaConversacion`
   si el corte lo pide); `AgentPersistence` = trait compuesto (supertraits), no
   monolito. Sin `#[deprecated]`: el alias compuesto mantiene compilando a los 6
   impls y a los consumidores externos (`Proyectos`/`VaultMemoria`/`Mem0Store` viven
   fuera de este repo; romperán en su sync, no aquí).
2. **S4:** subtraits `SoportaSkills` / `SoportaAmbitos` + detección de capacidad
   explícita en el curador (rotura dura según § Decisiones.1 corregida): se eliminan
   los 2 defaults; `Memoria`/`Sqlite` declaran ambas capacidades; los dobles de test
   declaran lo que simulan (`TiendaPrueba::sin_registro` pasa a NO implementar
   `SoportaSkills`, el test de curador verifica el aviso por capacidad, no por `Err`).
   Sin tocar `cdp` (firma vigente y fuera de S4) y sin `attach_shell_por_defecto`
   (inexistente).
3. **S5:** `NavegadorBase` + `Capturable` + `Scriptable` + `Automatizable`
   (nombres de la auditoría; sustituyen a los de la v1–v4); el adaptador desktop
   implementa las 4 caras y la tool consume las caras que necesita (`None` en
   CLI/web sigue siendo fail-closed). El `snapshot` desktop ignora hoy el selector
   (`puerto.rs:99-102`): observado, fuera del alcance de S5, sin cambio de conducta.
4. **S1/S2:** extraer de `tool.rs` el bloque de espera (`espera.rs`) y partir según
   auditoría en `herramientas/contexto.rs`, `herramientas/resultado.rs`,
   `herramientas/registro.rs` (`tool.rs` solo el trait, ~120 lín.); de
   `runtime/mod.rs` el ciclo de reintentos/tool-loop (`ciclo.rs`) + `runtime/construccion.rs`
   (builder + lista declarativa), telemetría a `runtime/telemetria.rs`, modos a
   `runtime/modos.rs`; `context.rs` parte `memoria/` y `llm/` a sus módulos.
    Gate por sub-fase: compila + tests del crate + `u64`/`bool` sin cambios de conducta.

    **Ejecución F4 (13-09, un commit):** S3 → S4 → S5 → S1 → S2 en este orden.
    S1 = `herramientas/contexto.rs` + `resultado.rs` + `registro.rs` (`tool.rs`
    solo el trait); sin `espera.rs` (no existe tal bloque en `tool.rs`).
    S2 = `runtime/construccion.rs` (TurnoConfig + PuertosHarness + `nuevo` +
    guardas/hooks/sandbox) + `modos.rs` (GuardaModoTurno) + `telemetria.rs`
    (bloqueo/telemetría/CompactarManual) + `ciclo.rs` (planes, tareas, curador
    nativo, aprobación, prompt, hooks); `mod.rs` fachada (struct + re-exports +
    19 tests verbatim). Deglob completo: los 5 hermanos (`subagente.rs`,
    `tools.rs`, `turno/mod.rs|auditoria.rs|permisos.rs`) + los 4 nuevos con
    imports explícitos desde paths canónicos; `mod.rs` conserva solo lo que usa
    su cuerpo (Arc/Uuid/HashMap/AgentContextManager/GuardasTurno/
    DispatcherHooks/TelemetriaTurno/AgentToolRegistry). Regiones propias a cero
    en `fmt --check`; resto del árbol con suciedad preexistente intacta.
    Evidencia: `cargo check --workspace --all-targets` 0 errores/0 warnings;
    `cargo test --workspace` 158+2+1+330+15 ok; `sentinel check 139A-8` PASS.

## F5n — Unificación path + SQLite restante (S7, S8, K8, R4)

Auditoría §S7/S8 (DRY), §K8/G4 (TOCTOU), §R4 (índices). Paso 3 (+R4 del paso 2).

1. **S7:** única `FileSystemPolicy` + `SandboxArchivos` como única validación;
   Tauri (`filesystem.rs`) y `web_datos/files.rs` = adaptadores finos.
   `tools_archivo.rs:18-24` (`MAX_LECTURA` 1 MB) vs `MAX_FILE_BYTES` 256 KB:
   unificar el tope en la política con justificación documentada.
2. **S8** (fix según auditoría): `core::git::ServicioGit` + pasarelas finas en
   `desktop/.../proyecto/git.rs` y `cli/.../web_datos/git.rs` (NO partir en 5
   módulos). Nota: 139A-2 añadió `arbol_repos`/barrido — se conserva vía el
   servicio.
3. **K8-TOCTOU:** `memoria.rs:417` (fuente = fila de BD) — revalidar
   existencia/contenido tras leer (hash o re-apertura) antes de indexar + doc
   del invariante; `sandbox.rs` ya canonicaliza: solo doc + test del check
   (`..`, symlink, rename). Sin `OPENAT2` (Linux-only).
4. **R4 [HECHO 2026-09-14]:** 6 índices en `MIGRACIONES`
    (`persistencia_sqlite.rs:247-257`): `idx_conversaciones_user_act`,
    `idx_turnos_conv`, `idx_acciones_turno`, `idx_tareas_user`,
    `idx_tarea_logs_tarea`, `idx_workspaces_user_fij`; regresión
    `persistencia_sqlite.rs:pruebas::listados_calientes_usan_indice` (9
    consultas: las 4 de §R4 + logs/sidebar + controles con índice previo).
    Antes: `SCAN a` (JOIN acciones), `SCAN turnos`, `SCAN tareas`,
    `SCAN tarea_logs`. Después: 0 SCAN. `cargo check --workspace
    --all-targets` 0 err; `cargo test --workspace` 515 passed (162+2+1+336+14).
    Residual: `tareas_pendientes` (`puerto.rs:388`,
    `WHERE estado ORDER BY creado_en`, scheduler) sin índice — fuera de §R4,
    deuda; listado por área usa `idx_conversaciones_ws` (prefijo) para el
    filtro y ordena en memoria (filas ya acotadas).
    DoD: tests de escape + índices verificados + gate PASS.

## F6n — Streaming/DOM restante (R5, R6, R8, R7-condicional)

Auditoría §R5/R6/R8 (+R7). Arranca re-ejecutando el fixture de 139A-7 como
regresión (ver § Decisiones.3).

1. **R5:** coalescing ~50–100 ms o N tokens en `turnos.rs:340-353`, evitar
   `to_value` en path caliente, separar canal control/datos (`DifusionSse`).
2. **R6:** `turn.finished` con título/uso incluido + actualización optimista;
   `listar` incremental/paginado (mitiga G1-fan-out §4.3).
3. **R8:** mover en vez de clonar en `sesion.rs:539,556-557`,
   `turnos.rs:342-345`, `ui/turno.rs:76,85-89`, `runtime/turno/mod.rs:54-63`
   (`Cow<str>`/buffers/hash-ventana).
4. **R7 (condicional):** `pintarHistorial` incremental SOLO si no colisiona
   con el trabajo sin commitear de 139A-9 en `desktop/ui` (verificar
   `git status` primero); si colisiona → deuda + aviso.
   Las etiquetas G1/G2/G3/K11/G6 de la v1–v4 no tienen sección en la
   auditoría: fuera de alcance (deuda en auditoría). Verificación: CDP counts
   + captura donde toque frente; `tsc` + `vite build`.

## F7n — Pin del gate + cierre (auditoría §5.6)

"Este repo solo sube el pin" (`glory-sentinel`): verificar si las 10+5 reglas
(§4.4) ya existen upstream y subir el pin; si no existen → tarea externa en
el proyecto dueño + cierre 139A-8 con la evidencia. Sin medidor ni reglas en
este repo. DoD: pin subido o tarea externa creada + último gate PASS +
roadmap limpio + completada + plan a `completados/`.

## Orden y dependencias

F0 → F1 → F2 → F3n → F4 → F5n → F6n → F7n. F4 (puertos) y F5n (unificación)
son independientes entre sí tras F3n; F6n depende de F4 solo en tipos tocados
(re-export los desacopla); F7n cierra.

## Definition of Done (global)

- Cada fase: tests nuevos verdes + suite del crate + gate `sentinel check` PASS.
- Sin cambios de conducta salvo los documentados (S4 firma, G2 casing, K3 auth).
- Roadmap actualizado, completada en `Agente/completados/tareas-YYYY-MM-DD.md`,
  commit por fase con ID.

## Ejecución F1 (13-09, completada + gate PASS)

- **K1** (`cli/src/infra/jaula.rs` nuevo + `ejecutor.rs`): `Command` directo,
  allowlist/denylist, `Error::Sandbox`; `truncar` en bytes, drenaje
  concurrente de pipes, kill-antes-de-reunir.
- **K13** (`daemon.rs`): token redactado por defecto (`****+4+len`),
  `--mostrar-token` explícito, bind `127.0.0.1` fijo, comparación constante.
- **K3** (`web/seguridad.rs::direccion_escucha`): sin token → loopback +
  aviso; con token → `0.0.0.0` (ya existía inline, ahora extraído + tests).
- **K9** (cookie firmada `{sid}.{firma:016x}`, SipHash por prefijo secreto
  de 128 bits, sin deps nuevas): secreto aleatorio/arranque,
  `--secreto-sesion <texto>` o `GLORY_HARNESS_SESION_SECRETO` para fijarlo;
  formato sin firmar → 401 fail-closed; `SameSite=Lax` + check de origen
  preexistentes se conservan; Bearer sin cambios.
- **K12** (desvío documentado: **sin `tower_governor`** —dep nueva con red
  no disponible—; ventana deslizante std de 30 inicios/minuto global
  → 429 `limite_turnos`, tope global 4 turnos concurrentes
  → 429 `demasiados_turnos`; el tope 1/sesión sigue en 409 `turno_activo`).
- Tests: 5 unit (`seguridad.rs`) + 3 integración (`turnos.rs`: cookie sin
  firmar 401, ventana 429, tope global 429); `web_datos/pruebas.rs` y
  `turnos.rs` migran a cookie firmada.
- Gate: `sentinel check 139A-8` **PASS** (21 archivos, 0 errores; 3 infos ISP
  preexistentes en `desktop/ui`). `cargo test comandos::web`: 44/44.

## Ejecución F2 (13-09, completada + gate PASS)

- **R1/R2** (`persistencia_sqlite.rs::con_conn`): `spawn_blocking` + `Arc` +
  `Mutex` sobre la conexión única (`busy_timeout` 5 s); `puerto.rs`
  (`AgentPersistence`, 17 métodos) y `tareas.rs` (`ProgramadorTareas`) clonan
  sus argumentos a valores propios y ejecutan el SQL en `con_conn`. Las
  inherentes puntuales de 1 fila por PK quedan síncronas (sin `await` que
  mover: no había bloqueo que corregir).
- **R3** (`conversaciones/mensajes.rs` nuevo, partido de `mod.rs` por
  limite-lineas 500): `leer_mensajes` (una consulta con `creado_en >= ?` +
  `LIMIT` opcionales, `ORDER BY creado_en ASC, rowid ASC`),
  `listar_mensajes_desde` (async, convierte la marca a segundos RFC 3339),
  `leer_conversacion` (SELECT puntual por PK para el auto-nombre);
  `sesion.rs::preparar_turno_con_modo` resuelve el punto de compactación
  ANTES de leer y usa `listar_mensajes_desde(Some(cuando))` (turno largo sin
  full-scan). `>=` conserva la semántica de segundos de `sesion.rs`.
- **Medición** (`cli/tests/p95_turno.rs` nuevo): fixture 5 k mensajes, N=2
  tareas × 25 iters = 50 `preparar_turno` concurrentes; baseline debug
  `p50=61,02 ms p95=87,81 ms` (`P95_BASELINE_MS=Some(88,0)`); tras el fix
  `p50=1,90 ms p95=3,83 ms` (≤ 0,5× = 44 ms, ~23× mejor).
- **DoD**: `mensajes_desde_filtra_en_sql_y_obtener_es_puntual` (200 mensajes,
  100 posteriores con `>=` mismo segundo, ownership ajena/inexistente).
- **Gate**: `sentinel check 139A-8` **PASS** (18 archivos, rust 83 s, 0
  errores, 0 warnings; 477 tests ok). Fricción de disco: el guard exige 8 GB
  en C: (`GLORY_MIN_FREE_GB` no atraviesa `sentinel check`, usa 8 fijo);
  liberado borrando `debug/incremental` (4,66 GB, se regenera). `C:\tmp`
  sigue ~11,7 GB (casi todo el target activo) por encima del tope 7 GB de
  `AGENTS.md`: pendiente higiene de target (mejora candidata, no creada).

## Ejecución F3n (13-09, completada + gate PASS)

- **K2** (`core/src/entorno.rs` nuevo): `env_clear()` + allowlist
  (`VARS_ENTORNO_MINIMO` + prefijos `CARGO_`, `RUSTUP_`, case-insensitive en
  Windows) vía `aplicar_entorno_minimo`, aplicada en el spawn de
  `ejecutor.rs::construir_comando`, `hooks.rs::ejecutar_comando_local`,
  `mcp.rs::McpProveedorStdio::nuevo` y `git.rs::ejecutar_git`. Tests:
  `filtra_claves…`, `conserva_los_valores…` e `hijo_ve_solo_la_allowlist`
  (señuelo `GLORY_HARNESS_SENUELLO_K2` barrido, `PATH` sobrevive).
- **K4** (`hooks.rs`): `HOOKS_BINARIOS_PERMITIDOS` (lectura/inspección +
  `git`), topes argv (`MAX_ARGS_HOOK=32`, `MAX_BYTES_ARGS_HOOK=64 KB`),
  `validar_comando_hook[_con_extras]` + `GLORY_HOOKS_ALLOW`,
  `Hook::{comando,comando_interno,interno}` (el interno salta la allowlist
  solo para (comando, args) fijados en código); el runner valida externos
  fail-closed. `notificar.rs` migrado a `comando_interno`.
- **K5** (`jaula.rs`): `cd` con args denegado en Windows (`cd` pelado =
  sonda de cwd); test `cd_con_destino_denegado_pelado_como_sonda`.
- **K6** (`hooks.rs`): `host_bloqueado_por_nombre`,
  `ip_egreso_denegada` (incluye IPv4-mapeadas), `ips_resueltas` por
  `spawn_blocking`, `validar_egreso_http` (denegar por defecto;
  `RunnerComandoHttp.egreso_extra` exime hosts exactos); el runner bloquea
  K6 antes del POST. Tests sin red (literales/nombres) + fix E0521
  (`host_auditable` por valor para el closure `spawn_blocking`).
- **K7** (`mcp.rs`): `MCP_BINARIOS_PERMITIDOS` (runtimes, sin shells),
  `validar_servidor_mcp[_con_extras]` + `GLORY_MCP_ALLOW`, misma
  normalización/tope que K4, higiene K2 en el spawn. Tests k7.
- **Gate**: `sentinel check 139A-8` **PASS** (20 archivos; coverage,
  sccache, sentinel y rust verdes; rust 186 s). Primera pasada FAIL por
  `clippy::useless_format` propio (`format!` sin interpolación en
  `validar_egreso_http:605`): corregido a `.to_string()`. Queda el WARNING
  preexistente `limite-lineas` en `hooks.rs` (ya tenía 1247 líneas antes de
  F3n; partirlo es trabajo separado, no de esta fase).
- **Fricción**: el árbol NO está `rustfmt`-limpio en general (deriva de
  toolchain, decenas de ficheros no tocados); se dejaron fmt-limpias solo
  las regiones propias (`entorno.rs`, `mcp.rs` a cero; `hooks.rs` sin hunks
  nuevos) sin tocar líneas ajenas. Leases del guard: uno por comando
  (`fmt --check`, `test`, …); `sentinel check` corre sin lease.

## Revisión

- v1 (13-09): borrador en revisión.
- v2 (13-09, re-revisión): añadidos K10 (estaba ALTA sin fase) y K12 explícito en
  F1; cabeceras F1/F3 actualizadas.
- v3 (13-09, cierre): 3 decisiones tomadas (§ Decisiones: rotura dura S4, test
  `p95_turno.rs` con umbral 0,5×, F5/G1 = solo regresión de 139A-7). Plan LISTO;
   arranque F0 pendiente de tu "adelante". Cobertura de hallazgos: todos los
   CRÍTICO/ALTO tienen fase (K1,K13,K3,K9,K12,K2,K8,K10,R1,R2,R3,S1–S5,G1);
   MEDIOS/BAJOS solo si los roza su fase; resto = deuda registrada en auditoría.
- v4 (13-09): F2 ejecutada y cerrada con gate PASS (ver § Ejecución F2);
  siguiente F3.
- v5 (13-09, reconciliación): preflight F3 detecta mismatch citas-plan vs
  auditoría + código (sin `env_clear`, sandbox ya canonicaliza, sin
  `Redactor`/`secreto:`, `OPENAT2` Linux-only, F6 compete a glory-sentinel,
  5 ALTA + R4/R6/R8 fuera del plan). Usuario ordena reescribir: F3–F6
  invalidadas → F3n (K2/K4/K5/K6/K7) → F4 (puertos, sin cambios) → F5n
  (S7/S8/K8/R4) → F6n (R5/R6/R8/R7-cond) → F7n (pin + cierre).
