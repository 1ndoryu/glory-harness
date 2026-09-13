# Plan 139A-8 — Remediación de la auditoría integral (2026-09-13)

> **Estado: EN EJECUCIÓN (v4, 13-09).** "Adelante" recibido; árbol limpio
> (`c986dac` pusheado). **F0, F1 y F2 completadas** (gates PASS abajo);
> siguiente F3.
> Las 3 decisiones abiertas están cerradas abajo (§ Decisiones).
> Fuente: `Agente/documentacion/auditoria-integral-2026-09-13.md` (v2 verificada).
> IDs S/R/K/G según la auditoría. Siguiente ID libre tras este: 139A-9.

## Decisiones (cerradas 13-09, v3)

1. **F4/S4 — rotura de API del puerto:** rotura dura `cdp(URL)`, SIN shim de
   compat en runtime. El compilador la hace segura: todos los call sites que no
   compilen se corrigen en la misma sub-fase; `cargo build --workspace` verde es
   la prueba de cobertura total. Sin `#[deprecated]` a medias.
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

## Objetivo

Cerrar los hallazgos CRÍTICO/ALTO de la auditoría sin romper lo que ya funciona:
seguridad de ejecución (K1), SQLite bajo async (R1/R2), historial por turno (R3),
puerto de persistencia (S3/S4), `tool.rs`/`runtime` (S1/S2) e IPC del frente (G1–G3),
más el re-medidor del gate (G8–G13). Los MEDIOS/BAJOS entran solo si su fase los
roza; el resto queda registrado como deuda en la auditoría.

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

## F3 — Filesystem/git/vault (K2, K8, K7, K10, S8)

1. **K2:** canonicalización + `OPENAT2_RESOLVE_IN_ROOT`/equivalente y symlink-policy
   en `sandbox.rs` y `fs()`; `comando_herramientas` valida argv contra el root
   anclado. Tests de escape (`..`, symlink, TOCTOU rename).
2. **K8:** `memoria.rs:417` — revalidar existencia/contenido tras leer (hash o
   re-apertura) antes de indexar; doc del invariante.
3. **K7:** `redact.rs`/`redact` detrás del rasgo `Redactor` + `SecretPattern`
   documentado; tests de cada familia.
4. **S8:** `git.rs` partido en `estado`/`historial`/`staging`/`stash`/`remoto`.
5. **K10:** etiquetas `secreto:` namespaced por defecto + auditoría de `refs`
   existentes; `mostrar_archivo`/exports vetan `secreto:` salvo opt-in explícito.

## F4 — Puertos y god-objects (S3, S4, S5, S1, S2)

1. **S3:** `AgentPersistence` → `Conversaciones`/`Mensajes`/`Sesiones`/`Ajustes`/
   `Proyectos`/`VaultMemoria`/`TareasProgramadas`/`Mem0Store` (+ `AtomicStore`
   si se adopta). Re-export temporal con `#[deprecated]`.
2. **S4:** firma `cdp(URL)` con rotura dura (ver § Decisiones.1): sin shim,
   corregir todos los call sites en la sub-fase; quitar `attach_shell_por_defecto`.
3. **S5:** `NavegadorPort` → `Navegacion`+`Captura`+`Interaccion`+`InspeccionDom`
   con `Navegador = Interaccion` como alias de compat.
4. **S1/S2:** extraer de `tool.rs` el bloque de espera (`espera.rs`) y de
   `runtime/mod.rs` el ciclo de reintentos/tool-loop (`ciclo.rs`); `context.rs`
   parte `memoria/` y `llm/` a sus módulos.
   Gate por sub-fase: compila + tests del crate + `u64`/`bool` sin cambios de conducta.

## F5 — Streaming e IPC del frente (R5, G1, G2, G3, K11, G6)

1. **G1:** 139A-7 ya lo entregó (ver § Decisiones.3): re-ejecutar su fixture como
   regresión (`repos=0 estado=0` al cambiar de conversación) y nada más.
2. **R5/G3:** `agent.event` como frame binario/`Uint8Array` + throttle `rAF`;
   ventana de 200 eventos con `ResizeObserver`; `turno_id` en el frame.
3. **G2:** normalizar disco `snake_case`↔`camelCase` en el boundary (test de ida/vuelta).
4. **K11/G6:** `iconos.ts` a `data:`/máscara CSS; sanitizador compartido para
   `memoria-contenido` (productor) manteniendo el del sink.
   Verificación: CDP counts + captura, `tsc` + `vite build`.

## F6 — Gate: re-medidor y reglas (G8–G13, §4.4)

1. Fixture de regresión del medidor (casos §4.2) + `||` multilínea corregido
   (G8, G9) + modo `--strict` docs-vs-impl (G10) + tests `--check/--fix` (G13).
2. Añadir las 10 reglas (§4.4) con `ignorar #[cfg(test)]` explícito; las 5
   opcionales como warnings. Excepción `todo-pendiente` solo con ticket.
3. `core/src/estado/` con invariantes + `debug_assert` (G11).
   DoD: medidor verde en fixture + gate PASS sobre el árbol.

## Orden y dependencias

F0 → F1 → F2 → F3 → F4 → F5 → F6. F1 y F2 son independientes entre sí tras F0;
F5 depende de F4 solo en tipos tocados (re-export los desacopla); F6 cierra.

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
