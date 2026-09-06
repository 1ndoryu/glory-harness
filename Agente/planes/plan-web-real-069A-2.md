# Plan: Glory Harness — modo web local unificado con Tauri (ID **069A-2**)

- **Fecha:** 2026-09-07 (v3 — revisión tras auditoría `supervisor-thinking`; resincroniza F0/F1 ya ejecutadas, resuelve contradicciones §9/§4.3 y §7, añade auth SSE usable desde navegador a F2 y parte F5 en F5a/F5b)
- **Área:** `glory-harness` (`core` + `cli` + `desktop/src-tauri` + `desktop/ui`)
- **Estado:** en ejecución — F0 y F1 completadas (06-09); siguiente: F2
- **Dependencias:** Bloque A Tauri cerrado; `Agente/planes/plan-glory-harness-desktop-2026-09-03.md`; contrato `AgenteEvento`; persistencia SQLite y sesión real existentes.
- **Relación con otros planes:** no modifica el alcance del navegador interno `069A-1`; reutiliza el runtime y la persistencia ya validados por Tauri. 069A-1 aportó además el fix SSE/axum-0.8 usado por F1 (commit `5f67e82`).

## 0. Decisión resumida

**Arquitectura:** servicio HTTP + SSE como API única. Tanto el modo Tauri como el modo web usan el mismo backend Rust y el mismo cliente HTTP/SSE en TypeScript.

- No habrá un `real.ts` (Tauri IPC) separado de `web.ts` (HTTP). Habrá un solo adaptador HTTP/SSE que funcione igual en ambos escenarios.
- Tauri, en lugar de comandos IPC nativos, arrancará el servidor HTTP embebido en loopback y la UI se conectará a `localhost`.
- El modo web (`glory-harness web`) arranca el mismo servidor, sirve la UI y espera conexiones del navegador.
- El mock sigue existiendo para desarrollo visual sin backend.

Esto elimina la necesidad de mantener dos adaptadores y dos contratos. La UI no necesita saber si está dentro de Tauri o en un navegador: siempre usa `fetch` + SSE. El coste de migrar Tauri de IPC nativo a HTTP embebido se evalúa en F5a/F5b: F5a mantiene IPC con thin-adapters sobre el servicio común; F5b (servidor embebido opt-in) solo se ejecuta si la evaluación lo justifica.

## 1. Objetivo

Permitir que `desktop/ui` funcione de manera prácticamente idéntica en:

1. **App de escritorio Tauri:** empaquetada, un solo proceso, inicio rápido, misma UI.
2. **Navegador local:** usuario avanzado ejecuta `glory-harness web` y abre `http://127.0.0.1:xxxx`.

En ambos casos:

- el runtime es el mismo `AgentRuntime`;
- la persistencia es la misma SQLite;
- el acceso a archivos del workspace es el mismo (el proceso Rust local tiene permisos del usuario);
- las tools, el streaming, las aprobaciones, la cancelación y la configuración son las mismas;
- el historial, las conversaciones y el uso persisten de forma idéntica.

La web local es un modo de ejecución adicional para quien prefiera usar el navegador. Tauri sigue siendo el modo de empaquetado principal.

La primera versión debe permitir:

1. abrir una sesión local;
2. listar, crear, cargar, renombrar, archivar y eliminar conversaciones;
3. enviar mensajes y recibir `AgenteEvento` en streaming;
4. cancelar el turno;
5. aprobar o denegar herramientas pendientes;
6. leer y guardar la configuración soportada por el núcleo;
7. consultar proveedores/modelos permitidos;
8. cambiar el workspace local mediante una ruta absoluta validada;
9. conservar historial, uso y estado de turnos en SQLite;
10. mostrar errores de transporte, configuración, desconexión y cierre de turno sin estados silenciosos.

La app Tauri debe seguir funcionando durante todas las fases, pero consumiendo finalmente la misma API HTTP/SSE embebida.

## 2. No alcance

- multiusuario, autenticación externa, registro o exposición a Internet;
- servidor remoto accesible desde otra máquina (loopback siempre por defecto);
- copiar el runtime, las tools o la persistencia a TypeScript;
- notificaciones nativas del sistema en modo web;
- selector nativo de carpetas en modo web (el workspace se configura en el servidor o por input de texto);
- reanudación de stream perdido desde offset histórico en la primera versión;
- deploy, administración remota o push implícito;
- navegador interno visible/controlado por el agente (`069A-1`).

## 3. Evidencia y estado inicial verificados (actualizado 07-09)

- `cli/src/servicio/sesion.rs` (`SesionComun`, `PreparacionTurno`, `Apertura`) ya extraído de `main.rs` en F0 (evidencia: `Agente/completados/tareas-2026-09-06.md`, gate 069A-2 PASS citado).
- F1 implementada: `cli/src/comandos/web.rs` (axum 0.8 + tower-http 0.6 `fs`, health/sesión/SSE/ServeDir, 4 tests en `mod tests`); `cargo check -p glory-harness` verde 06-09 (solo warnings `dead_code`). Gaps F1 pendientes, movidos a F2: flag `--fixture` ignorado, tipos de evento SSE en `data` en vez de campo `event:`, sanitize `\n` como parche, exit code tras fallo de `axum::serve`.
- `desktop/src-tauri/src/main.rs` cubría sesión, persistencia de mensaje de usuario, streaming, uso, cancelación, aprobación, conversaciones, configuración y reconfiguración; a 07-09 está en refactor ajeno activo (069A-1/069A-2-web) y no compila — F5a bloqueado hasta que compile y el E2E Tauri baseline quede fijado con hash.
- `desktop/ui/src/tauri/real.ts` ofrece el contrato de adaptador Tauri que se sustituirá por un adaptador HTTP único.
- UI: factoría en `desktop/ui/src/main.ts:493` (`USA_REAL`), `usaReal` en `desktop/ui/src/componentes/panelChat.ts` (×10), detección en `real.ts:204` (`__TAURI__`).
- `cli/src/comandos/daemon.rs` usa TCP loopback + NDJSON; se conserva como daemon interno/legado, no como API web. F6 fijará la matriz daemon-TCP (agentes locales) vs web-HTTP (UI) y justificará o unificará `GLORY_HARNESS_DAEMON_TOKEN` / `GLORY_HARNESS_WEB_TOKEN`.
- El workspace declara Sentinel/VarSense y `sentinel doctor --json --workspace .` devolvió `readyForAnalyze: true`, `readyForGate: true`, sin incidencias (reverificado 07-09: `policy`, `issues: []`).
- El árbol ya contiene cambios ajenos/preexistentes en `core/`, `desktop/src-tauri/`, `roadmap.md` y archivos no rastreados. 069A-2 no debe descartarlos ni mezclarlos silenciosamente.

## 4. Arquitectura

```text
┌─────────────────────────────────────────────┐
│  glory-harness (binario Rust)               │
│                                              │
│  ┌──────────────┐  ┌──────────────────────┐ │
│  │  CLI mode    │  │  Modo servidor HTTP   │ │
│  │  (run, daemon)│  │  (glory-harness web) │ │
│  └──────────────┘  │                      │ │
│                    │  • GET /api/v1/...   │ │
│                    │  • POST /api/v1/...  │ │
│                    │  • SSE /api/v1/stream│ │
│                    │  • sirve la UI (dist)│ │
│                    └──────────┬───────────┘ │
│                               │             │
│                    ┌──────────▼───────────┐ │
│                    │  Servicio de sesión   │ │
│                    │  compartido           │ │
│                    │  • AgentRuntime       │ │
│                    │  • PersistenciaSqlite │ │
│                    │  • tools/aprobación   │ │
│                    │  • workspace local    │ │
│                    └──────────────────────┘ │
└─────────────────────────────────────────────┘

Tauri embebido:
  ┌──────────────────────────────────────────┐
  │  glory-harness-desktop.exe              │
  │  • arranca servidor HTTP en loopback    │
  │  • abre webview que carga localhost      │
  │  • mismo servicio, mismo adaptador TS   │
  └──────────────────────────────────────────┘

Modo web local:
  ┌──────────────────────────────────────────┐
  │  glory-harness web                       │
  │  • arranca servidor HTTP en loopback     │
  │  • sirve UI + API                        │
  │  • usuario navega a http://127.0.0.1:xxxx│
  │  • mismo servicio, mismo adaptador TS    │
  │  • acceso completo al workspace local    │
  └──────────────────────────────────────────┘
```

### 4.1 Transporte: HTTP + SSE

Razones para SSE sobre WebSocket en esta fase:

- **Unidireccional servidor→cliente:** los eventos del agente fluyen del backend a la UI. SSE es el mecanismo natural para eso.
- **Acciones del cliente:** usar `POST` ordinarios para enviar mensaje, cancelar, aprobar. Sin necesidad de un canal bidireccional persistente.
- **Reconexión nativa del navegador:** SSE reconecta automáticamente; el adaptador TS se beneficia de eso sin implementar lógica extra.
- **Backend más simple:** no necesita mantener un canal WebSocket, ni manejar frames, ni gestionar su ciclo de vida.
- **Pruebas con herramientas estándar:** se puede probar con `curl`.

Si en el futuro se necesita un canal bidireccional (por ejemplo, para el navegador interno 069A-1), se puede añadir WebSocket como canal complementario. Pero el flujo principal de turno funciona con SSE.

### 4.2 Servicio compartido

Extraer del orquestador Tauri actual (`desktop/src-tauri/src/main.rs`) la lógica de aplicación que no depende de `tauri::Window` ni `AppHandle`:

- abrir/reconfigurar una sesión;
- resolver usuario, conversación, workspace y configuración;
- guardar el mensaje de usuario antes de ejecutar;
- construir el historial previo;
- ejecutar el runtime y reenviar `AgenteEvento` a un canal;
- cancelar y finalizar el turno con estado explícito;
- responder aprobaciones pendientes;
- operaciones CRUD de conversaciones y configuración.

Este servicio vive como módulo público en `cli/src/` o, si la separación es limpia, en un crate nuevo mínimo. El servidor HTTP y Tauri lo consumen.

### 4.3 Tauri consume la misma API (decisión §9: opt-in, ver F5a/F5b)

En lugar de exponer comandos IPC, Tauri (solo cuando F5b lo justifique y bajo flag explícito):

1. arranca el servidor HTTP en `127.0.0.1:{puerto}` al iniciar la app — puerto efímero seguro: bind `:0`, `local_addr()` real, URL pasada al webview solo tras readiness del `/healthz`;
2. la webview carga `http://127.0.0.1:{puerto}`;
3. la UI usa `fetch` + SSE, exactamente igual que el modo web.

La UI no ve diferencia. Tauri no necesita `invoke` ni `listen` salvo para operaciones estrictamente nativas (diálogo de archivos si se decide mantenerlo). El adaptador TypeScript es único.

**Consecuencia:** la UI se vuelve 100% agnóstica del contenedor. Tauri conserva su valor como empaquetado (ventana, instalador, iconos, actualizaciones). Hasta F5b, Tauri sigue con IPC cuyos comandos delegan en `SesionComun` (F5a), sin duplicar el flujo de sesión.

## 5. Contrato web inicial

El prefijo es `/api/v1`. Las respuestas de error tienen forma estable `{ "ok": false, "code": string, "message": string }`; no incluyen secretos ni trazas internas.

### 5.1 HTTP

| Método y ruta | Propósito | Resultado mínimo |
|---|---|---|
| `POST /api/v1/session` | Crear sesión autenticada | `session_id`, configuración efectiva, capacidades |
| `DELETE /api/v1/session/:id` | Cerrar sesión | cierre idempotente |
| `GET /api/v1/conversations` | Listar conversaciones | ids, títulos, estado archivado, fechas |
| `POST /api/v1/conversations` | Crear conversación | conversación creada y seleccionada |
| `GET /api/v1/conversations/:id/messages` | Cargar historial | mensajes ordenados y DTOs seguros |
| `PATCH /api/v1/conversations/:id` | Renombrar/archivar | conversación actualizada |
| `DELETE /api/v1/conversations/:id` | Eliminar | confirmación explícita |
| `GET /api/v1/providers` | Catálogo allowlist | proveedores/modelos sin credenciales |
| `GET /api/v1/config` | Leer configuración pública | solo opciones soportadas |
| `PATCH /api/v1/config` | Guardar configuración | configuración validada y efectiva |
| `GET /healthz` | Readiness/liveness local | estado sin información sensible |

La API debe comprobar que cada conversación pertenece a la sesión/usuario autenticado. Los ids recibidos no autorizan acceso por sí mismos.

### 5.2 SSE y acciones HTTP

Ruta de eventos: `GET /api/v1/session/:id/events`.

**Auth SSE usable desde navegador (requisito F2):** `EventSource` nativo no puede enviar cabecera `Authorization`, así que el Bearer-en-header de F1 no sirve para la historia principal del plan. F2 debe añadir uno de los dos antes de cualquier prueba en navegador real: (a) cookie `HttpOnly` de sesión + comprobación `Origin` en mutaciones, o (b) ticket de un solo uso y corta duración entregado por `POST turns`/sesión y pasado en query del SSE. Sin esto, F2 no es comprobable.

El servidor devuelve `Content-Type: text/event-stream`. Cada evento usa el campo SSE `event:` (no solo dentro del JSON de `data`; F1 los mete todos como `message` — alinear en F2) con `data` JSON:

- `event: ready` → `{ "session_id": "..." }`
- `event: turn.started` → `{ "turn_id": "..." }`
- `event: agent.event` → `{ "event": <AgenteEvento> }`
- `event: turn.finished` → `{ "turn_id": "...", "ok": true, "error": null }`
- `event: error` → `{ "code": "...", "message": "..." }`
- `event: heartbeat` → `{ "at": "..." }`

Acciones cliente mediante HTTP:

- `POST /api/v1/session/:id/turns` → inicia un turno con `{ "message": "..." }`;
- `POST /api/v1/session/:id/turns/:turn_id/cancel` → cancela un turno;
- `POST /api/v1/session/:id/approvals/:approval_id` → responde `{ "approved": true|false }`.

Reglas del canal:

- `turn.finished` es obligatorio en cierre normal, error o cancelación si el canal sigue disponible.
- Solo hay un turno activo por sesión; un segundo inicio devuelve un conflicto explícito.
- Una aprobación tardía o duplicada es idempotente y no ejecuta dos veces la tool.
- El backend filtra los eventos a la sesión autorizada y conserva el contrato `AgenteEvento` del core.
- Si el navegador reconecta, recibe el estado actual y los eventos nuevos; la primera versión no promete replay histórico completo. F2 implementa el snapshot mínimo (endpoint de estado o `ready` con estado al suscribir; hoy `ready` se emite pre-registro y se pierde si el SSE llega tarde) o lo declara diferido explícito.
- La UI no presenta una respuesta completa falsa si recibe `error` o `turn.finished` con fallo.

## 6. Workspace local y acceso al sistema de archivos

El proceso Rust es local en ambos escenarios y conserva el acceso completo al filesystem que tenga el usuario que lo ejecuta. El navegador nunca accede directamente al disco: realiza peticiones al backend local.

- `GET /api/v1/workspace` devuelve el workspace efectivo, con rutas sensibles ocultas o normalizadas cuando se muestran en UI.
- `POST /api/v1/workspace` recibe una ruta absoluta y cambia el workspace de la sesión/proceso después de validarla.
- El backend verifica que la ruta existe, es un directorio accesible y cumple las raíces permitidas por la configuración local. No se acepta una ruta relativa ni una ruta construida mediante concatenación insegura.
- Al cambiar de workspace, se bloquea si hay un turno activo y se reconfigura la sesión antes de aceptar nuevos mensajes.
- El mismo contrato se usa desde Tauri y desde el navegador local; no se añade un picker ni una API específica del contenedor.
- La ruta no se registra completa en logs ni se devuelve innecesariamente a clientes secundarios. Nota: `POST /api/v1/session` (F1) devuelve el workspace absoluto en su respuesta — aceptado para loopback/single-user, no loguearlo ni ampliarlo a otros endpoints sin revisar §7.

## 7. Autenticación, autorización y seguridad

### MVP local/single-user

- El servidor arranca con una clave configurada fuera del repositorio (`GLORY_HARNESS_WEB_TOKEN` o mecanismo equivalente del proyecto).
- El cliente intercambia la credencial una sola vez por una sesión opaca de corta duración; preferir cookie `HttpOnly`, `Secure` cuando corresponda y `SameSite=Lax/Strict`.
- Si se usa cookie, proteger mutaciones con comprobación de `Origin` y CSRF; si se usa bearer temporal, no persistirlo en logs ni URL.
- Bind `127.0.0.1` por defecto. Escuchar en una interfaz remota requiere una fase separada con autorización, TLS y autenticación adecuada.
- Tokens: nunca en logs persistentes, URLs, `DATABASE_URL`, claves LLM ni paths sensibles. Excepción única: impresión a stderr del token temporal generado en first-run loopback (`web.rs`, igual que `daemon.rs`), necesaria para que el usuario local lo obtenga; prohibido reimprimirlo o persistirlo.
- Comparación de token en tiempo no constante aceptada solo en loopback; pasar a comparación constante al introducir cookie/remoto.

### Autorización de dominio

- Validar longitud y contenido de mensajes, títulos, ids, modo, razonamiento y opciones antes de llegar al runtime.
- Permitir solo providers/modelos del catálogo real del núcleo.
- Resolver el workspace en servidor y validar contención contra las raíces permitidas.
- Aplicar ownership a sesión, conversación, turno y aprobación.
- Limitar conexiones, sesiones, turnos concurrentes, tamaño de payload y frecuencia de mutaciones.
- No usar `innerHTML`, `eval`, SQL interpolado ni shell concatenado.

## 8. Fases ejecutables

### F0 — Contrato y extracción segura ✅ COMPLETADA 06-09

- [x] Congelar la matriz de capacidades común y localizar todos los `usaReal`/mensajes “requiere Tauri” en la UI (`main.ts:493`, `componentes/panelChat.ts` ×10, `real.ts:204`).
- [x] Extraer el servicio de sesión (`SesionComun`, `PreparacionTurno`, `Apertura`) sin cambiar el resultado actual de Tauri (06-09, ver `Agente/completados/tareas-2026-09-06.md`).
- [x] Definir DTOs HTTP/SSE y errores versionados (`{ "ok": false, "code", "message" }`, §5).
- [x] Añadir tests unitarios de ownership, límite de un turno, aprobación idempotente, workspace y cancelación.
- [x] Verificar `cargo check`, tests afectados y `tsc --noEmit`.

**Salida:** existe una interfaz de servicio reutilizable y la regresión Tauri sigue verde.

### F1 — Servidor HTTP mínimo ✅ COMPLETADA 06-09 (con gaps trasladados a F2)

- [x] Implementar `glory-harness web` con bind loopback y puerto configurable (`cli/src/comandos/web.rs`, axum 0.8).
- [x] Implementar health, autenticación, creación/cierre de sesión y stream SSE `ready`/`heartbeat`.
- [x] Servir la UI compilada desde el mismo proceso para evitar CORS y configuración innecesaria (`tower-http fs` + `ServeDir`).
- [x] Añadir fixture local sin proveedor externo para comprobar errores y ciclo de vida (parcial: flag `--fixture` aceptado pero ignorado — ver F2).

**Salida:** el navegador local puede abrir una sesión y cerrar correctamente.

Gaps F1 asumidos por F2: tipos de evento SSE en campo `event:` (hoy todo llega como `message`), sanitize `\n` como parche (emitir JSON compacto en origen), exit code distinto de SUCCESS tras fallo de `axum::serve`, `--fixture` real o eliminación del flag.

### F2 — Turno real con SSE ✅ COMPLETADA 06-09 (`e731c74`)

- [x] Auth SSE usable desde navegador (cookie `HttpOnly` + `Origin` o ticket de un solo uso en query, §5.2) + prueba con `EventSource` nativo sin headers.
- [x] Tipos de evento en campo SSE `event:` + snapshot de estado al suscribir (o diferido explícito).
- [x] Sustituir el sanitize `\n` por emisión JSON compacta; cerrar gaps F1 (exit code, `--fixture`).
- [x] Añadir `POST turns`, eventos `AgenteEvento`, `turn.finished`, cancelación y cierre/error explícitos.
- [x] Conectar el servicio a la misma persistencia SQLite y runtime que Tauri.
- [x] Cubrir aprobación/denegación con una tool fixture segura mediante POST.
- [x] Validar que una desconexión no deja un turno eternamente “ejecutando”.

**Salida:** una prueba vertical ejecuta un turno real/fixture con streaming, cancelación y aprobación.

### F3 — Conversaciones, configuración y workspace ✅ COMPLETADA 06-09 (`40e47d0`)

- [x] Implementar endpoints de conversaciones, historial, providers/modelos y configuración.
- [x] Implementar `GET/POST /api/v1/workspace` con validación de ruta absoluta y bloqueo durante turnos.
- [x] Probar dos procesos (web + Tauri/CLI) sobre la misma SQLite: `busy_timeout`, modo WAL y error explícito ante bloqueo — sin “database is locked” silencioso.
- [x] Aplicar validación, ownership y límites en cada endpoint.
- [x] Mantener solo opciones realmente soportadas por `OpcionesRun`/`TurnoConfig`.

**Salida:** la API puede reconstruir una conversación, cambiar workspace local y aplicar configuración persistida.

### F4 — Adaptador HTTP/SSE único ✅ COMPLETADA 06-09 (`8b597e7`)

- [x] Convertir `desktop/ui/src/tauri/real.ts` en un adaptador HTTP/SSE común, o moverlo a `desktop/ui/src/adaptadores/api.ts` sin duplicar implementación.
- [x] Cambiar `main.ts` a una factoría simple: API HTTP disponible → adaptador real; backend ausente → mock con aviso claro.
- [x] Eliminar las decisiones de transporte de `componentes/panelChat.ts` y componentes relacionados; consultar capacidades del contrato.
- [x] Mantener `mock.ts` para desarrollo visual sin backend.
- [x] Mostrar estados de conexión, reconexión, error y stream interrumpido.

**Salida:** la misma UI ejecuta el flujo real por HTTP/SSE dentro de Tauri y en navegador.

### F5a — Integración Tauri sin migración (thin-adapters, desbloquea cuando `main.rs` compile)

- [ ] Delegar cada comando Tauri en `SesionComun` sin cambiar payloads IPC; adaptador de compatibilidad si el servicio lo requiere.
- [x] Fijar E2E Tauri baseline con hash de commit verde antes de tocar más Tauri.
- [x] Evaluar con datos el coste de F5b (líneas IPC a eliminar, riesgos E2E, mecanismo de puerto efímero + readiness).

**Evaluación F5a (read-only, 07-09, baseline `c28e653`):** `main.rs` 1189
líneas, compila (2 warnings), ~19 comandos IPC (`abrir/enviar/cancelar/
reconfigurar/aprobaciones` + 6 conversación + `rewind`/`restaurar_tramo`/
`proveedores`/`config_*`/`elegir_workspace`/`actualizar_meta`). De ellos,
4 sin paridad web por diseño (`elegir_workspace` diálogo nativo, `rewind`,
`restaurar_archivos_tramo`, `actualizar_meta`: el transporte HTTP los
rechaza explícito). **F5b NO justificada hoy:** el modo web ya funciona
standalone (E2E curl 07-09) y Tauri in-process está intacto; el servidor
embebido añadiría puerto efímero + readiness sin ganancia funcional, y
`main.rs` sigue en refactor ajeno activo (colisión). Revisitar con baseline
fresco cuando el refactor ajeno se asiente; entonces thin-adapters sobre los
~15 comandos con paridad.

**Salida:** Tauri consume el servicio común sin duplicar lógica y la migración embebida queda evaluada, no decidida.

### F5b — Integración Tauri embebida (SOLO si F5a la justifica, bajo flag explícito §9)

- [ ] Arrancar el servidor HTTP dentro de `desktop/src-tauri` en loopback (bind `:0` → `local_addr()` → URL al webview tras readiness) y obtener un puerto libre de forma segura.
- [ ] Hacer que el webview cargue la URL local del servidor, con cierre ordenado al salir.
- [ ] Mantener diálogos o capacidades estrictamente nativas solo si son imprescindibles; no duplicar el flujo de sesión.
- [ ] Verificar que el modo `web` y el modo Tauri usan el mismo workspace, SQLite y configuración.

**Salida:** Tauri y navegador son dos formas de abrir la misma aplicación, no dos implementaciones.

### F6 — Límites, validación y cierre ✅ BACKEND COMPLETADO 07-09 (`c28e653`)

- [x] Añadir rate limits y límites de cuerpo/conexiones/SSE.
- [x] Validar origin, cookie/token, expiración y cierre de sesión.
- [x] Fijar matriz daemon-TCP vs web-HTTP (consumidores, envs de token) y justificar o unificar tokens.
- [x] Auditar logs para confirmar que no contienen secretos, prompts innecesarios ni rutas sensibles.
- [x] Documentar arranque, token, bind, workspace, SQLite, backup y apagado.
- [ ] Ejecutar pruebas Rust, `tsc --noEmit`, build UI y E2E en navegador y Tauri.
- [ ] Ejecutar el gate canónico con reporte asociado al commit y registrar evidencia en `Agente/completados/`.

**Salida:** ambos modos pasan la misma matriz funcional y el modo web local falla de forma explícita y segura.

## 9. Compatibilidad Tauri y estrategia de rollback (resuelto v3: el flag explícito gobierna)

- No cambiar nombres ni payloads de comandos Tauri durante F0–F3 salvo que el servicio común lo requiera; si se necesita un ajuste, mantener un adaptador de compatibilidad.
- Cada fase debe poder revertirse sin borrar SQLite ni migrar datos destructivamente.
- El servidor web se habilita mediante comando/flag explícito; no arranca al abrir la app Tauri. Esto incluye F5b: el modo embebido es opt-in hasta que la evaluación F5a demuestre lo contrario; la frase anterior de §4.3 que sugería arranque automático queda sustituida por esta regla.
- La selección web en la UI se activa solo cuando `window.__TAURI__` no existe y el endpoint está configurado; sin endpoint, la UI conserva mock con aviso claro.
- Si una regresión aparece en Tauri, se detiene la extracción y se corrige en el servicio común antes de continuar; no se parchea solo el transporte web.

## 10. Pruebas y criterios de aceptación

### Aceptación funcional

- [ ] Con token válido, el navegador abre sesión y con token inválido recibe `401` sin crear runtime.
- [ ] El SSE conecta con `EventSource` nativo sin cabeceras personalizadas (cookie o ticket, §5.2).
- [ ] Los eventos SSE llegan con su tipo en el campo `event:`, no todos como `message`.
- [ ] Dos procesos sobre la misma SQLite no producen “database is locked” silencioso.
- [ ] Puede listar/crear/cargar/renombrar/archivar/eliminar solo sus conversaciones.
- [ ] Un mensaje produce tokens/eventos en vivo y un `turn.finished` coherente.
- [ ] Cancelar detiene el turno, emite estado visible y persiste el turno como cancelado/interrumpido.
- [ ] Aprobar ejecuta una tool fixture una sola vez; denegar produce permiso denegado y no ejecuta la tool.
- [ ] El historial sobrevive al reinicio del servidor y mantiene mensaje de usuario y respuesta.
- [ ] Cambiar modelo/modo/razonamiento solo aplica valores permitidos y conserva la conversación.
- [ ] Una desconexión SSE no deja recursos ni turnos ejecutándose indefinidamente.

### No regresión

- [ ] El E2E Tauri previamente validado sigue pasando.
- [ ] El modo mock sigue funcionando sin backend.
- [ ] `cargo test --workspace`, `cargo clippy --all-targets -- -D warnings` cuando corresponda, `tsc --noEmit` y build UI quedan verdes.
- [ ] Gate Sentinel/VarSense PASS; cualquier warning preexistente queda separado de los findings nuevos.

## 11. Riesgos y mitigaciones

| Riesgo | Mitigación |
|---|---|
| Extraer lógica de `main.rs` rompe Tauri | F0 con tests de regresión y servicio usado primero por Tauri; no duplicar implementación. |
| SSE se corta durante una tool/aprobación | estado explícito, timeout, cancelación idempotente y finalización persistida. |
| Exposición accidental de un servidor local | bind loopback, opt-in, token fuera del repo, comprobación de origin y sin deploy implícito. |
| El daemon TCP parece reutilizable pero carece de contrato | mantenerlo separado; reutilizar núcleo/servicio, no su protocolo ni su estado en memoria. |
| API HTTP añade demasiadas dependencias | evaluar dependencia mínima y medir; no crear un crate nuevo hasta demostrar necesidad. |
| UI sigue acoplada a Tauri | factoría + capacidades y prueba de ejecución web sin `window.__TAURI__`. |
| Dos clientes ejecutan el mismo turno | lock por sesión, límite uno, ownership y respuesta de conflicto explícita. |
| Ruta de workspace inválida o peligrosa | aceptar solo rutas absolutas, validar existencia/contención y bloquear cambios durante un turno. |

## 12. Definition of Done

069A-2 se cierra únicamente cuando:

1. existe el plan implementado por fases y el contrato HTTP/SSE está cubierto por pruebas;
2. la UI web ejecuta el flujo esencial real sin mock, con `EventSource` nativo;
3. Tauri mantiene su flujo E2E (baseline con hash) y no se ha duplicado el runtime;
4. autenticación, ownership, límites y errores están probados;
5. el historial y las aprobaciones son durables/idempotentes;
6. concurrencia SQLite entre dos procesos verificada sin bloqueos silenciosos;
7. el gate final es PASS con evidencia reproducible;
8. roadmap, completadas y documentación reflejan exactamente lo validado.

## 13. Próximo bloque ejecutable

F6-backend (sin tocar Tauri, en vuelo ajeno): límites de cuerpo/mensaje/sesiones + TTL de sesión en `web.rs`; auditoría de logs sin secretos; matriz daemon-TCP vs web-HTTP; documento operativo (arranque, token, bind, workspace, SQLite, backup, apagado). Después F5a-evaluación read-only (baseline hash + coste F5b con datos) y, cuando el refactor ajeno de `main.rs` se asiente, thin-adapters. Cierre con gate canónico + completada + push.
