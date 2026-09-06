# Plan de ejecución: modo web real de Glory Harness (revisión 069A-2)

- **Fecha:** 2026-09-06
- **Área:** `glory-harness` (`cli/src/comandos/web.rs`, `cli/src/servicio/sesion.rs`, `desktop/ui`)
- **Estado:** pendiente de revisión del usuario — NO implementar hasta aprobación
- **Plan de arquitectura/contrato (fuente canónica):** `Agente/planes/plan-web-real-069A-2.md` (ID 069A-2)
- **Rama:** `main`. **Gate:** Sentinel/VarSense en `.quality-tools-harness`.

> **Propósito de este documento:** consolidar TODO el trabajo pendiente de 069A-2 en un plan de
> ejecución concreto, accionable y verificable, basado en el código real ya recopilado, para que
> el usuario lo revise antes de implementar. No sustituye al plan de arquitectura 069A-2: lo
> refina a nivel de bloques, archivos y decisiones.

---

## 1. Objetivo del bloque

Que el **modo web** (`glory-harness web` + navegador en `http://127.0.0.1:8799`) funcione de
verdad: abrir sesión real, chatear con la IA (streaming `AgenteEvento` por SSE), cancelar,
aprobar/denegar tools, listar/crear/cargar/renombrar/archivar/eliminar conversaciones, leer/
guardar configuración, cambiar workspace — todo con la MISMA persistencia SQLite y runtime que
el desktop Tauri. Hoy la UI en navegador solo muestra datos ficticios (compilada con
`VITE_MOCK=1` y el adaptador `real.ts` habla Tauri IPC, inexistente en navegador).

**No alcance (confirmado con el usuario en 069A-2 §2):** el navegador interno WebView2
(`069A-1`) NO funcionará en un navegador normal — solo existe dentro de la app Tauri. Esto se
comunicará al usuario; el chat real sí funcionará en navegador.

---

## 2. Contexto verificado (fuentes reales leídas)

### 2.1 `cli/src/comandos/web.rs` — estado actual (F1, casi completo)
- Rutas ya presentes: `GET /healthz`, `POST /api/v1/session`, `DELETE /api/v1/session/{id}`,
  `GET /api/v1/session/{id}/events` (SSE), fallback `ServeDir` para la UI.
- `SesionWeb { comun: SesionComun, tx: broadcast::Sender<String> }`; estado
  `AppState { token, sesiones: Mutex<Vec<(String, Arc<SesionWeb>)>> }`.
- SSE fix aplicado (axum 0.8): sanitiza `\n`→`\\n`, `\r`→`\\r`; KeepAlive `.text("heartbeat")` 15s.
- `crear_sesion` emite `ready` en el canal y devuelve `{ok, session_id, modelo, workspace,
  proveedores, conversacion}` (usa `Apertura`, que NO es `Serialize` → se vuelca con `json!`).
- Tests existentes: healthz, 401 sin/con token falso, 404 sesión inexistente.
- Warnings actuales: imports sin usar (`Body`, `Query`, `Method`, `Uri`, `Deserialize`,
  `AgenteEvento`), campo `comun` nunca leído.

### 2.2 `cli/src/servicio/sesion.rs` — servicio de sesión (F0, transporte agnóstico)
- `SesionComun { runtime: Arc<AgentRuntime>, persistencia: Arc<PersistenciaSqlite>, user_id:
  Uuid, modelo: String, modo: String, workspace: String }` — campos `pub`, sin getters.
- `OpcionesSesion { provider, modelo, dir, modo, razonamiento, nueva_conversacion, navegador }`
  con `Default`.
- `SesionComun::abrir(OpcionesSesion) -> Result<(Self, Apertura), Error>`; `info(conv_id, aviso)`;
  `reconfigurar(provider, modelo, modo, razonamiento)`; `async preparar_turno(conv_id, mensaje,
  meta) -> PreparacionTurno { turno_id, conversacion_id, historial, mensaje_efectivo, runtime }`;
  `async cancelar_turno(turno_id)`.
- `PreparacionTurno` ya deja runtime clonable (`Arc`) → el transporte decide cómo ejecutar y
  transportar (Tauri usa mpsc; HTTP usará broadcast/SSE).

### 2.3 `desktop/src-tauri/src/main.rs` — orquestador de referencia (a replicar por HTTP)
- Patrón del turno (`enviar_turno`): guard de un turno activo → `conv_id` del panel →
  `meta` → `comun.preparar_turno(conv_id, mensaje, meta).await` → `mpsc::channel::<AgenteEvento>(64)`
  → tarea que acumula `Usage` (`UsoAcumulado`) y reenvía cada `ev` → `runtime.ejecutar_turno(
  user_id, turno_id, conv_id, historial, mensaje_efectivo, &tx_ev).await` → al `Ok` persiste
  `turno_actualizar_uso(...)`; al `Err` emite `AgenteEvento::Error`; siempre emite cierre.
- Helpers útiles: `conteos(&LlavesProveedor)`, `info_de_conversacion`, `panel_poner_conversacion`.
- CRUD conversaciones: `conversacion_crear/listar/renombrar/archivar/eliminar`,
  `listar_mensajes` (async, trait), `acciones_por_conversacion`, `turno_ultimo_uso_por_conversacion`.
- `proveedores_disponibles` = `catalogo_proveedores()` (de `glory_harness_core::llm`, devuelve
  `Vec<(&str, Vec<&str>)>`) + `LlavesProveedor::from_env()` para conteos.
- `config_leer/guardar` (tabla `config` global), `actualizar_meta` (normaliza, guarda en sesión).
- `elegir_workspace` usa `rfd::FileDialog` (nativo → NO en web; el plan usa ruta por input).

### 2.4 `desktop/ui/src/tauri/real.ts` — superficie del adaptador a replicar por HTTP
- Exporta tipos: `AgenteEvento` (unión discriminada por `tipo` snake_case), `OpcionesTurno`,
  `InfoConversacion`, `InfoSesion`, `MensajeGuardado`, `AccionRecuperada`, `CargaConversacion`,
  `ProveedorInfo`, `UsoTurno`, `ResultadoTurno`, `HooksAdaptador`, `AdaptadorReal`.
- `crearAdaptadorReal(hooks)` → `{ montar, detener, usoUltimoTurno, resultadoUltimoTurno,
  asegurarSesion, sesion: { nueva, listar, cargar, renombrar, archivar, eliminar, rewind,
  restaurarTramo, proveedores, configLeer, configGuardar, elegirWorkspace, actualizarMeta } }`.
- Lógica de UI del adaptador (reutilizable sin duplicar): `aplicar(ev)` switch de 16 eventos,
  `crearMensajeUsuario`, `crearAvisoSistema`, `crearTarjetaAprobacion`, `crearMensajeAsistenteVivo`,
  `crearHerramientaViva`, `el`, `bajarScroll`, `iconoDeTool`.
- **Divergencia conocida a corregir:** en TS `requiere_aprobacion` lleva `{tool, clasificacion}`
  pero el Rust emite `{tool, argumentos}`.

### 2.5 Tipos del núcleo
- `AgenteEvento` (`core/src/contrato/evento.rs`): tag `tipo`, snake_case. Variantes Token,
  ToolStart, ToolResult, RequiereAprobacion, PeticionAprobacion, Pregunta, PermisoDenegado,
  SubagenteInicio, PlanPropuesto, SubagenteFin, Usage, Contexto, ContextoDetalle, Telemetria,
  Error, Done, ToolNavegador.
- `RespuestaAprobacion` (`core/src/politica/aprobacion.rs`): `Aprobar`="aprobar",
  `Rechazar`="rechazar", `Siempre`="siempre".
- `catalogo_proveedores()` en `core/src/nucleo/llm/modelo.rs` (pública).

---

## 3. Decisiones de diseño (para revisión del usuario)

| # | Decisión | Recomendación | Alternativa |
|---|----------|---------------|-------------|
| D1 | **Modelo de sesión web** | Una sesión por `POST /session`; dentro de ella **una conversación actual** (equivalente al panel `"principal"`). Suficiente para MVP web (M1). | Soporte multi-panel web (más complejo, posponer). |
| D2 | **Vault/rewind en web** | NO implementar `rewind`/`restaurarTramo` en web v1: el adaptador web expone `sesion.rewind`/`restaurarTramo` devolviendo **error explícito "no disponible en modo web v1"**. El contrato 069A-2 §5 no lista endpoints de rewind/restore. El resto de CRUD sí funciona. | Implementar rewind sin vault (archivos_tramo vacío). |
| D3 | **Auth en el navegador** | La UI la sirve el MISMO proceso loopback → al servirla, el servidor **no exige Bearer para orígenes del propio proceso servido** en el flujo `POST /session` inicial **si** se valida `Origin`/`Referer` == mismo origen servido; o alternativa simple: inyectar el token en un `<meta>`/variable al servir el HTML para que el adaptador lo use como Bearer. **A decidir con el usuario** (§7 del 069A-2 prefiere cookie HttpOnly o bearer no persistido). Opción más simple y segura para MVP: el adaptador lee el token de un endpoint bootstrap mismo-origen (sin exponerlo en logs/URL). | Bearer manual en cada fetch (incómodo). |
| D4 | **Transporte SSE del adaptador TS** | `EventSource` no permite headers custom → o cookie (HttpOnly) o **query param token** (evitar en logs) o **`fetch` + lectura de `ReadableStream`** (permite Bearer y da control de reconexión). Recomendación MVP: **`fetch` + `ReadableStream`** con Bearer en header; si el token se sirve por bootstrap mismo-origen, también sirve `EventSource` sin header. **A decidir con el usuario.** | — |
| D5 | **Un único adaptador vs dos** | Mantener `crearAdaptadorReal` (Tauri IPC) **intacto** (el desktop debe seguir funcionando; F5 de 069A-2 — Tauri embebido por HTTP — es posterior). Añadir `crearAdaptadorWeb` (fetch/SSE) con la MISMA forma, reutilizando helpers de UI. En `main.ts`, elegir por entorno: Tauri → real; navegador con backend alcanzable → web; navegador sin backend → mock con aviso claro. La unificación total (un solo adaptador HTTP) es F5. | Refactorizar `real.ts` a HTTP ya (rompe Tauri hoy). |
| D6 | **Detección del backend web** | En `main.ts`, si `!esEntornoTauri()`: hacer un `probe` a `/healthz` (mismo origen si la UI la sirve el servidor). Si responde → `USA_WEB=true` con adaptador web. Si no → `USA_MOCK` (según `VITE_MOCK`) o aviso "sin backend". | Flag de build `VITE_WEB=1`. |
| D7 | **Workspace web** | `GET/POST /api/v1/workspace` con ruta absoluta validada (existe, es dir, canonicalize). Sin diálogo nativo. Bloquea si hay turno activo. Reabre la sesión con `nueva_conversacion=true` (igual que `elegir_workspace`). | — |

---

## 4. Contrato HTTP/SSE que implementa el backend (resumen de 069A-2 §5)

Prefijo `/api/v1`. Errores: `{ "ok": false, "code": string, "message": string }`.

### 4.1 HTTP
| Método y ruta | Propósito |
|---|---|
| `POST /api/v1/session` | Crear sesión autenticada → `session_id` + estado efectivo |
| `DELETE /api/v1/session/:id` | Cerrar sesión (idempotente) |
| `GET /api/v1/conversations` | Listar conversaciones |
| `POST /api/v1/conversations` | Crear conversación (y seleccionarla como actual) |
| `GET /api/v1/conversations/:id/messages` | Cargar historial (DTOs seguros) |
| `PATCH /api/v1/conversations/:id` | Renombrar / archivar |
| `DELETE /api/v1/conversations/:id` | Eliminar |
| `GET /api/v1/providers` | Catálogo allowlist (sin credenciales) |
| `GET /api/v1/config` | Leer configuración pública |
| `PATCH /api/v1/config` | Guardar configuración validada |
| `GET /api/v1/workspace` | Workspace efectivo |
| `POST /api/v1/workspace` | Cambiar workspace (ruta absoluta validada) |
| `GET /healthz` | Liveness |

### 4.2 SSE y acciones de turno
- `GET /api/v1/session/:id/events` → SSE. Eventos: `ready`, `turn.started`, `agent.event`
  (`data: { "event": <AgenteEvento> }`), `turn.finished`, `error`, `heartbeat`.
- `POST /api/v1/session/:id/turns` `{ "message" }` → `{ "turn_id" }`. Reglas: un turno activo
  por sesión (segundo → 409 conflicto); valida longitud.
- `POST /api/v1/session/:id/turns/:turn_id/cancel` → cancela (aborta handle + `cancelar_turno`
  en BD + emite `turn.finished ok:false "cancelado"`).
- `POST /api/v1/session/:id/approvals/:approval_id` `{ "approved": bool }` → mapea a
  `RespuestaAprobacion` (approved=false → Rechazar; para "siempre" se usará
  `{ "approved": true, "siempre": true }` o un campo `respuesta` — a definir con el contrato TS).
- `turn.finished` es OBLIGATORIO en cierre normal/error/cancelación (el adaptador no puede
  quedarse "ejecutando" eternamente).

---

## 5. Bloques de ejecución (orden y verificación por bloque)

> Cada bloque: editar → compilar/type-check → verificación mínima → (al final del bloque)
> commit coherente con ID 069A-2. **No mezclar** con la deuda 069A-5 ni con cambios ajenos.

### Bloque 1 — Backend: turno real + SSE + cancelación + aprobación (F2)
Archivo: `cli/src/comandos/web.rs`.

1. Ampliar `SesionWeb` para reflejar el orquestador de un panel:
   - `conversacion_actual: Mutex<Uuid>` (se fija en `crear_sesion` con `apertura.conversacion.id`).
   - `turno: Mutex<TurnoWeb { handle: Option<JoinHandle<()>>, activo: bool, turno_id: Option<Uuid> }>`.
   - `meta: Mutex<Option<String>>`.
2. `POST /api/v1/session/:id/turns`:
   - Guard de un turno activo → 409 `sesion_activa` / `turno_en_curso`.
   - `comun.preparar_turno(conv_actual, mensaje, meta).await`.
   - Emitir por el broadcast `{event:"turn.started", data:{turn_id}}`.
   - Spawn tarea que: crea `mpsc::channel::<AgenteEvento>(64)`, relé acumulando `Usage` y
     reenviando cada evento con envoltura `{event:"agent.event", data:{event:<AgenteEvento>}}`
     (usar `serde_json::to_value`), rompe en `Done`; luego `runtime.ejecutar_turno(...).await`;
     al Ok persiste `turno_actualizar_uso`; al Err emite `agent.event` de tipo `Error`;
     SIEMPRE emite `{event:"turn.finished", data:{turn_id, ok, error:null|string}}`; libera el
     guard de turno.
   - Responder `202 { turn_id }` de inmediato.
3. `POST /api/v1/session/:id/turns/:turn_id/cancel`: aborta handle, `comun.cancelar_turno(id)`
   en tarea, emite `turn.finished ok:false "cancelado por el usuario"`.
4. `POST /api/v1/session/:id/approvals/:approval_id`: parsea body `{approved, siempre?}` →
   `runtime.responder_aprobacion(&id, r)`. Idempotente (el runtime ya lo es).
5. Mantener el fix de newlines para TODOS los payloads SSE (los eventos `AgenteEvento` pueden
   contener `\n` en `resumen`/`mensaje`/`diff`).
6. Limpiar imports sin usar. Tests: 409 por segundo turno, cancelación emite cierre, ownership
   (aprobar con id de otra sesión → error).

**Verificación:** `cargo check` (target `C:\tmp`), tests `web.rs`, prueba con `curl` de un
turno fixture/sin clave (debe emitir `turn.finished` aunque falle el proveedor, nunca colgar).

### Bloque 2 — Backend: conversaciones, proveedores, config, workspace (F3)
Mismo archivo `web.rs`.

1. `GET/POST /api/v1/conversations` (crear selecciona como actual; devuelve `InfoConversacion`).
2. `GET /api/v1/conversations/:id/messages` → `CargaConversacion` (id, titulo, mensajes,
   acciones, ultimo_uso). **Ownership:** validar que la conv pertenece al `user_id` de la sesión.
3. `PATCH /api/v1/conversations/:id` body `{titulo?}` y/o `{archivada?}` → renombrar/archivar.
4. `DELETE /api/v1/conversations/:id` → eliminar; si era la actual, crear una nueva vacía y
   seleccionarla (paridad con `eliminar_conversacion` del desktop).
5. `GET /api/v1/providers` → `catalogo_proveedores()` + `LlavesProveedor::from_env()` →
   `Vec<ProveedorInfo { id, modelos, claves }>` (mismo shape que el desktop). Sin credenciales.
6. `GET /api/v1/config` → objeto `{ proveedor, modelo, modo, nivelRazonamiento,
   contexto_max_ventana }` (claves soportadas por config) **sin secretos**.
   `PATCH /api/v1/config` → guarda claves validadas (longitud, valores permitidos) y aplica
   `reconfigurar` si cambia provider/modelo/modo/razonamiento (bloqueado si turno activo).
7. `GET /api/v1/workspace` → `{ workspace }` (ruta completa NO a clientes secundarios; se puede
   devolver a este cliente mismo-origen, o normalizada si se prefiere).
   `POST /api/v1/workspace` `{ path }` → valida ruta absoluta, `canonicalize`, existe y es dir;
   bloquea si turno activo; persiste `config "workspace"`; reabre sesión (`nueva_conversacion=true`);
   devuelve el estado de sesión nuevo.
8. Endpoint de **reconfiguración** vía `PATCH /api/v1/session/:id` (para cambiar modelo/modo/
   razonamiento SIN perder conversación) — o cubrirlo con `PATCH /api/v1/config`. **A decidir**:
   el desktop distingue `reconfigurar_sesion` (conserva conv) de `abrir_sesion` (conv nueva).
   Recomendación: `PATCH /api/v1/session/:id` `{provider?,modelo?,modo?,razonamiento?}` conserva
   la conversación actual; el adaptador lo llama en `asegurarSesion` cuando cambia la clave.

**Verificación:** tests de ownership (conversación de otra sesión → 404), validación de workspace
(ruta relativa → 400; no existe → 400; es archivo → 400), `cargo check` + tests.

### Bloque 3 — Frontend: adaptador HTTP/SSE (F4)
Archivo nuevo: `desktop/ui/src/adaptadores/web.ts` (o `api.ts`).

1. Crear módulo con `crearAdaptadorWeb(hooks)` devolviendo la MISMA forma que
   `crearAdaptadorReal` (montar/detener/usoUltimoTurno/resultadoUltimoTurno/asegurarSesion/sesion.*).
2. Reutilizar (importar, no duplicar) helpers de UI de `real.ts` o mover los compartidos
   (`iconoDeTool`, tipos, `aplicar` switch, componentes) a un módulo común
   (p. ej. `adaptadores/eventos.ts` / `adaptadores/tipos.ts`) para evitar duplicación y
   dependencia circular. `real.ts` se mantiene intacto para Tauri.
3. Cliente HTTP: helper `api(método, ruta, body)` con Bearer (según D3) y parseo de errores
   `{ok:false, code, message}` → lanzar con mensaje claro.
4. SSE: según D4. Parsear los `event:`/`data:` y mapear a las llamadas a `aplicar(ev)` /
   `cerrar(ok,error)`.
5. `sesion.rewind` / `sesion.restaurarTramo` → `Promise.reject(new Error('no disponible en modo
   web v1'))` (D2).
6. `sesion.elegirWorkspace` → en web: pedir ruta por prompt/input de texto y `POST /workspace`
   (D7). Sin diálogo nativo.
7. **Corregir divergencia TS:** `requiere_aprobacion` debe recibir argumentos reales del Rust.
8. Mantener `mock.ts`/`simulacion` para desarrollo sin backend.

**Verificación:** `tsc --noEmit`, `npm run build`; prueba en navegador contra el backend.

### Bloque 4 — Frontend: cablear `main.ts` (F4)
Archivo: `desktop/ui/src/main.ts`.

1. Detección: `USA_REAL = esEntornoTauri()` (Tauri); si no, `probe` a `/healthz`
   (o `VITE_WEB=1`) → `USA_WEB`; `USA_MOCK = !USA_REAL && !USA_WEB && VITE_MOCK==='1'`.
2. Elegir adaptador: Tauri → `crearAdaptadorReal`; web → `crearAdaptadorWeb`; mock → simulación.
3. `usaReal` efectivo para `panelChat`/`panelMeta`/`sidebar` = `USA_REAL || USA_WEB`
   (las ramas "requiere la app Tauri" funcionarán en web al pasar por el adaptador web).
4. Ocultar o avisar el botón de navegador interno en web (Tauri-only; fuera de alcance).
5. Actualizar el aviso del else-branch de `panelChat.ts` (línea ~618) para el caso real "sin
   backend": en navegador sin servidor → mensaje claro de cómo arrancar `glory-harness web`.
6. `guardarSidebar`/`leerSidebar`/config/meta/renombrar/archivar/eliminar/panelMeta: usar el
   adaptador web cuando `USA_WEB` (los métodos existen con la misma forma).
7. Estados de conexión/reconexión/error/stream interrumpido visibles (sin estados silenciosos).

**Verificación:** `tsc --noEmit`, `npm run build`, prueba en navegador con backend real:
abrir sesión, enviar mensaje real con streaming, cancelar, CRUD conversaciones, cambiar modelo,
cambiar workspace.

### Bloque 5 — Compilación, servidor y prueba vertical
1. Recompilar binario: `$env:CARGO_TARGET_DIR="C:\tmp\glory-target\glory-harness"; cargo build
   -p glory-harness`.
2. Recompilar UI SIN mock (o con la flag web): `cd desktop/ui; npm run build`.
3. Relanzar `glory-harness web` (matar el proceso actual en `:8799` primero).
4. Recargar `http://127.0.0.1:8799` y probar:
   - conversación real con IA (requiere claves LLM en entorno; si no hay, verificar que el turno
     termina con `turn.finished` explícito y error visible, nunca colgado);
   - CRUD de conversaciones persistido (sobrevive reinicio del servidor);
   - cancelación y aprobación/denegación;
   - cambiar workspace.
5. **No romper Tauri:** lanzar (o al menos `cargo check` del crate desktop + `tsc`) para
   confirmar que el desktop sigue compilando y su E2E previo sigue válido.

### Bloque 6 — Cierre, gate y documentación
1. Verificación funcional real registrada (con o sin claves; límites explícitos).
2. `cargo test --workspace` (target C:\tmp), `tsc --noEmit`, build UI.
3. Gate canónico de glory-harness (Sentinel/VarSense en `.quality-tools-harness`) con reporte
   asociado al commit. Los warnings preexistentes (deuda 069A-5) quedan SEPARADOS.
4. Commit por bloque con mensaje en español e ID 069A-2.
5. Actualizar: `roadmap.md` (estado de 069A-2), este plan (marcar bloques), y
   `Agente/completados/tareas-2026-09-06.md` (o la fecha del cierre) con evidencia.
6. Comunicar al usuario: navegador interno NO en navegador normal (fuera de alcance); el chat
   real SÍ.

---

## 6. Riesgos y mitigación

| Riesgo | Mitigación |
|---|---|
| Un turno queda "ejecutando" si el SSE se desconecta | El spawn del turno es independiente del cliente SSE; SIEMPRE emite `turn.finished` y libera el guard al terminar (aunque nadie escuche). |
| `Apertura`/`InfoConversacion` no son `Serialize` en algunos casos | Convertir a DTOs con `json!`/structs `Serialize` propios (como ya hace `crear_sesion`). |
| Divergencia `requiere_aprobacion` TS vs Rust | Corregir el tipo TS en el bloque 3; probar con un evento real. |
| Romper el desktop Tauri | No tocar `real.ts` ni `main.rs` del desktop en este bloque; solo añadir el adaptador web y ramas condicionales en `main.ts`. Verificar compilación desktop. |
| SSE con `\n` (axum 0.8 assert) | Sanitización ya aplicada; mantenerla en todos los payloads. |
| Exposición accidental del token | Bind loopback, token fuera del repo, no imprimir; decisión D3 con el usuario. |
| Ruta de workspace insegura | Solo absoluta + canonicalize + existe + es dir + bloqueo con turno activo. |
| C:\tmp cerca del límite | Comprobar antes de compilar; usar target por rama. |

---

## 7. Definition of Done de este bloque

1. El navegador local abre una sesión REAL con el runtime y la SQLite compartidos.
2. Enviar un mensaje produce streaming de eventos `AgenteEvento` y un `turn.finished` coherente
   (nunca un estado "ejecutando" eterno).
3. Cancelar y aprobar/denegar funcionan por HTTP; aprobación idempotente.
4. CRUD de conversaciones, proveedores, config y workspace funcionan con ownership y validación.
5. El modo mock sigue funcionando sin backend; el desktop Tauri no se ha roto (sigue compilando
   y su E2E previo sigue válido).
6. `cargo test --workspace`, `tsc --noEmit`, build UI y gate canónico verdes (warnings
   preexistentes separados).
7. Roadmap, plan y completadas reflejan exactamente lo validado; navegador interno fuera de
   alcance comunicado al usuario.

---

## 8. Pendiente de decisión del usuario (antes de implementar)

1. **D3 — Mecanismo de autenticación del navegador** (bootstrap mismo-origen vs token inyectado
   vs Bearer manual vs cookie HttpOnly).
2. **D4 — Transporte SSE del adaptador** (`fetch`+`ReadableStream` con Bearer vs `EventSource`
   con cookie/query).
3. **D5 — Confirmar** mantener `real.ts` Tauri intacto y añadir adaptador web separado (la
   unificación total es F5, posterior).
4. **D7 — Workspace por input de texto** en web (sin diálogo nativo): confirmar.
5. Alcance de verificación en vivo: ¿hay claves LLM disponibles en el entorno para probar una
   respuesta real, o basta con validar el ciclo de vida (turn.finished + errores visibles)?
