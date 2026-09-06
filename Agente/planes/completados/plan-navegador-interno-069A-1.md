# Plan: Navegador interno para Glory Harness — agent-controlled browser (ID **069A-1**)

- **Fecha:** 2026-09-06 (refinado 2026-09-06, v3 — revisión de arquitectura)
- **Área:** glory-harness (desktop Tauri 2 + core Rust)
- **Inspiración:** ChatGPT Codex (navegador visible con agente controlando + anotaciones del usuario)
- **Referencias técnicas:**
  - **Tauri 2 API — `Window::add_child()`** (`feature = "unstable"`): crea un segundo WebView (hijo) dentro de la misma ventana. `Webview::with_webview()` expone la plataforma nativa subyacente.
  - **CoreWebView2 (Windows)**: interfaz COM nativa del motor WebView2. Métodos clave: `CapturePreview` (screenshot nativo PNG/JPEG → stream), `CallDevToolsProtocolMethod` (CDP: DOM.getDocument, Page.captureScreenshot, Input.dispatchMouseEvent, Runtime.evaluate, etc.), `AddHostObjectToScript` (bridge objeto Rust → JS), `ExecuteScript`, `PostWebMessageAsJson`, `Navigate`, `GoBack/Forward`, eventos `NavigationStarting`, `NewWindowRequested`, `PermissionRequested`, `WebMessageReceived`.
  - **Tauri 2 Capabilities**: sistema de permisos por webview/window (`src-tauri/capabilities/`). `remote` gobierna qué páginas remotas pueden acceder a APIs IPC expuestas; no habilita la navegación por sí mismo. La navegación y el acceso a IPC deben tratarse como controles separados.
  - [Obscura](https://github.com/h4ckf0r0day/obscura) — headless browser en Rust, ~30 MB RAM base, V8 engine, CDP + MCP nativo, Apache 2.0
  - [terminal-browser](https://github.com/zenbu-labs/terminal-browser) — Electron-based, renderizado en terminal (descartado para RAM)
  - Codex CLI (Apache 2.0) — no tiene navegador visible en CLI; el navegador es feature de la app de escritorio (no open source)
- **Estado:** ✦ F1 y F2 completados y validados funcionalmente (06-09-2026). El plan continúa ACTIVO: F3 (panel UI), F4 (anotaciones), F5 (tool núcleo) y F6 (multimodal) pendientes de implementar.
- **Evidencia F1 (2026-09-06):** Tauri 2.11.5 expone `Window::add_child()` y `Webview::with_webview()` con `features = ["unstable"]`; wry 0.55.1 y `webview2-com` 0.38.2 están presentes en `Cargo.lock`. `feature = "unstable"` activada en `Cargo.toml` del desktop.
- **Evidencia F2 — compilación (2026-09-06):** `navegador.rs` implementa `CapturePreview` PNG→Base64, `ExecuteScript`, `CallDevToolsProtocolMethod`, click, rellenado y snapshot, con límites de entrada y timeout de 15 s. `cargo build` (shim Sentinel, target `C:\tmp`) terminó correctamente en 3m 26s (dev profile), sin errores. Warnings ajenos preservados (imports no usados en `cli/src/persistencia_sqlite/conversaciones.rs`, `leer_max_ventana` no usado en `main.rs`).
- **Evidencia F2 — artefacto final:** `C:\tmp\glory-target\glory-harness\debug\glory-harness-desktop.exe`, 23.320.064 bytes, `LastWriteTimeUtc: 2026-09-06 08:24:12Z`, SHA-256 `7D35AA9B36112D05AD31953E08AFE93C90DFE08A393A8C41EA76836D77A23676`.
- **Evidencia F2 — validación funcional real (06-09-2026):** el binario se ejecutó con `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` apuntando a Vite dev server en `http://localhost:1420/`. Cliente CDP ejecutó todo el ciclo IPC (ver `C:\tmp\test-ipc-navegador.mjs`):
  - `navegador_abrir` + `navegador_js` con DOM de fixture (createElement) → ✓
  - `navegador_rellenar(#area, "test-123")` → ✓ textarea.value = "test-123"
  - `navegador_click(#btn)` → ✓ dataset.clicked = "yes"
  - `navegador_cdp(DOM.getDocument)` → ✓ respuesta CDP con root válido
  - `navegador_snapshot` → ✓ texto "Click\\nTest"
  - `navegador_capturar` → ✓ PNG válido, base64 length 9432, magic bytes `iVBOR`
  - `navegador_navegar` → ✓
  - `navegador_redimensionar` → ✓
  - Rechazo URLs no permitidas: data:, javascript:, file: → ✓ todas RECHAZADAS
  - **Ciclo cerrar→reabrir validado con instancia limpia** (ver `C:\tmp\test-ipc-navegador-reopen.mjs`): abrir→js→cerrar→abrir→DOM fixture→rellenar→leer→click→cdp→snapshot→capturar→cerrar → ✓ todas las operaciones COM exitosas tras la reapertura. Confirmado que `ICoreWebView2` extraído una vez en `navegador_abrir` sobrevive al cierre y recreación de la child.
- **Evidencia F2 — memoria (árbol glory-harness, medido con `Get-CimInstance` + `Get-Process`):**
  - Antes de apertura: 7 procesos (1 desktop + 6 WebView2 glory-harness), WS 379.70 MiB, Private 183.33 MiB
  - Después de apertura (child webview cargando fixture local): 8 procesos (nuevo renderer), WS 464.44 MiB, Private 226.98 MiB
  - Después de cierre: 8 procesos (el renderer hijo persiste como proceso base), WS 464.85 MiB, Private 227.03 MiB
  - La webview hija añadió ~84 MiB de Working Set y ~44 MiB de Private Bytes. El proceso renderer hijo (`--type=renderer --renderer-client-id=6`) permanece activo como parte del runtime WebView2 base incluso sin webviews.
- **Evidencia de gate (2026-09-06):** `sentinel check 069A-1 --stages scripts/quality/stages.json --workspace .` dio **PASS**, alcance incremental de 24 archivos, 0 errores, 9 warnings y 4 info. Los warnings son existentes/no bloqueantes.

---

## 0. Resumen ejecutivo

**Decisión de motor: WebView2 nativo (Tauri 2) como camino principal, no Obscura.**

La investigación de la API nativa de Tauri 2 revela que **no necesitamos Obscura como motor**. Glory Harness ya ejecuta sobre WebView2 (wry/Tauri 2 en Windows). Con `Window::add_child()` (feature `unstable`) podemos crear un **segundo WebView** dentro de la misma ventana que actúe como navegador interno. A través de `Webview::with_webview()` accedemos al `CoreWebView2` nativo que expone:

- **`CapturePreview`** — screenshot nativo del contenido del WebView (PNG/JPEG, NO BMP), sin DevTools, sin CDP, sin subprocesos.
- **`CallDevToolsProtocolMethod`** — CDP completo: navegación, click, relleno de formularios, evaluación JS, captura de DOM.
- **`AddHostObjectToScript`** — bridge bidireccional: objetos Rust expuestos al JS de la página. Su semántica de llamadas y sus restricciones de origen deben validarse en el prototipo; no se asume que sea un canal síncrono.
- **`PostWebMessageAsJson`/`ExecuteScript`** — comunicación directa backend → página.
- **Eventos**: `NavigationStarting`, `NewWindowRequested`, `PermissionRequested`, `WebMessageReceived`.

**El navegador usa el mismo runtime WebView2 que ya tiene la app — reutiliza el proceso base del runtime Chromium, sin motor extra.** Obscura pasaría a ser una alternativa opcional (headless, menor RAM) para cuando el usuario solo quiera automatización sin interfaz visible.

**Límite de RAM total estimado (desktop + navegador): ≤ 178 MB** (objetivo ~180 MB). NO verificado: es estimación basada en la webview actual sin medir una webview hija con página real. WebView2 es multiproceso (proceso browser + renderer + GPU); una webview hija puede añadir procesos hijos de Chromium además del heap compartido. La medición real con `Get-Process` al cerrar cada fase determinará el consumo exacto (ver §5).

---

## 1. Comparación de motores: Obscura vs WebView2 nativo

Esta sección se añadió tras la corrección del usuario (069A-1 v2). Antes de decidir el motor, se investigaron las APIs reales disponibles.

### 1.1 Obscura

| Aspecto | Detalle |
|---|---|
| **Motor** | V8 (anteriormente Servo, ahora V8 embed) |
| **RAM base** | ~30 MB (headless, sin GPU, sin UI) |
| **CDP** | Nativo (soporta navegación, click, fill, screenshot, snapshot de accesibilidad) |
| **MCP** | Nativo (`obscura mcp` como servidor MCP) |
| **Interacción** | Solo por CDP/MCP — no hay ventana visible, no hay navegador que el usuario vea |
| **Capturas** | CDP `Page.captureScreenshot` → base64 |
| **Multiplataforma** | Sí (Rust, V8 portable) |
| **Licencia** | Apache 2.0 |
| **Integración** | Subproceso externo, comunicación por stdio/HTTP MCP o CDP WebSocket |

**Limitaciones críticas:**
- **No es interactivo visible.** El usuario no ve la página en vivo. Solo ve capturas fijas que pide el agente. Una secuencia de capturas no equivale a un navegador interactivo.
- **Consumo real puede ser mayor.** 30 MB es base headless; una página con JS pesado (React, Google Maps, etc.) puede aumentar el heap V8 significativamente. Obscura expone `--v8-flags "--max-old-space-size=256"`.
- **No hay scroll, hover, ni interacción en tiempo real.** El usuario no puede tocar la página directamente; todo pasa por el agente.
- **Requiere lanzar/detener un proceso extra.**
- **Mecanismo de capturas bajo demanda** — el agente pide screenshot cuando necesita ver, el usuario no ve cambios hasta el próximo screenshot.

### 1.2 WebView2 nativo (vía Tauri 2 `Window::add_child`)

| Aspecto | Detalle |
|---|---|
| **Motor** | Edge WebView2 (Chromium, mismo runtime que la app) |
| **RAM base** | ~30 MB adicional estimado por webview hija (comparte runtime Chromium base; WebView2 es multiproceso: browser + renderer + GPU. Una webview hija puede añadir un proceso renderer hijo, no solo memoria heap. Sin medir aún.) |
| **Screenshot nativo** | `ICoreWebView2::CapturePreview` — API COM nativa, PNG/JPEG directo a stream (NO BMP), sin CDP, sin DevTools |
| **CDP** | `ICoreWebView2::CallDevToolsProtocolMethod` — CDP completo asíncrono: DOM, Input, Runtime, Page, etc. |
| **Interacción** | **El usuario ve el navegador EN VIVO.** Es un WebView2 dentro de la app, con scroll, hover, interacción real. El usuario puede tocar la página directamente. |
| **Comunicación backend↔página** | `AddHostObjectToScript` (bridge nativo), `PostWebMessageAsJson`, `ExecuteScript` |
| **Control del agente** | CDP (`CallDevToolsProtocolMethod`) + `ExecuteScript` — el agente llama a la webview hija igual que a un headless, pero el usuario ve los cambios |
| **Multiplataforma** | Windows nativo; macOS/Linux vía webkit2gtk/WKWebView (Tauri abstrae) |
| **Integración** | No es un subproceso externo gestionado por la app; la webview está dentro de la ventana Tauri, con host Rust y procesos WebView2 separados |

**Ventajas críticas:**
- **El usuario VE el navegador en vivo.** No es una secuencia de capturas; es una ventana navegable que el usuario puede tocar.
- **Captura nativa sin CDP.** `CapturePreview` es API del sistema, más rápida y ligera que CDP.
- **Sin proceso externo gestionado por la aplicación.** No hay que lanzar/detener Obscura. WebView2 mantiene su arquitectura multiproceso (browser/renderer/GPU) aunque la webview hija forme parte de la ventana Tauri.
- **Bridge nativo.** `AddHostObjectToScript` permite llamar funciones Rust desde JS de la página (inyección de herramientas de anotación, captura de coordenadas de click del usuario).
- **Scroll e interacción real.** El usuario puede hacer scroll, hover, clic directamente en la página del navegador, independientemente del agente.

### 1.3 Decisión

| Criterio | Obscura | WebView2 nativo | Ganador |
|---|---|---|---|
| RAM base | ~30 MB extra (claim público no verificado) | Estimación inicial ~30 MB; WebView2 es multiproceso y debe medirse como árbol de procesos | Pendiente de medición |
| Interactividad visible | No (solo capturas) | **Sí (navegador real)** | WebView2 |
| Screenshot nativo | No (CDP) | Sí (CapturePreview) | WebView2 |
| CDP | Nativo | Sí (CallDevToolsProtocolMethod) | Empate |
| Bridge backend→JS | CDP Runtime.evaluate | AddHostObjectToScript + PostWebMessage | WebView2 |
| Sin proceso externo gestionado por la app | No | Sí (aunque WebView2 mantiene procesos propios) | WebView2 |
| Multiplataforma | Sí (Rust puro) | Parcial (Win: WebView2, Mac/Linux: webkit2gtk) | Obscura |
| Madurez (en GH) | No instalado | **Ya usado** (wry/Tauri 2) | WebView2 |

**Conclusión: WebView2 nativo vía `Window::add_child()` es el motor principal.**

Obscura queda como alternativa opcional (headless, menor RAM, multiplataforma) para modo "solo automatización sin interfaz visible" — pero no se implementa en este plan a menos que surja una necesidad explícita.

**Implicación arquitectónica:** este plan cambia de "MCP + Obscura subproceso" a "segundo WebView2 + CDP nativo vía CoreWebView2". La UI usa comandos IPC Tauri para navegación manual y controles del panel; la tool del agente no debe atravesar `desktop/ui`: se conecta desde el runtime a un puerto Rust proporcionado por el backend Tauri (detalle en F5).

### Qué ya existe que reutilizamos

| Componente | Path | Qué aporta |
|---|---|---|
| **Cliente MCP stdio** | `core/src/herramientas/mcp.rs` | `McpProveedorStdio` JSON-RPC línea a línea. **Ya no es el camino principal**, pero puede usarse para Obscura como modo headless opcional. |
| **Registro dinámico de tools** | `core/src/herramientas/tool.rs` | `registrar_mcp(tools, mcp_tools, registro)`. El desktop puede registrar tools `navegador_*` como tools nativas, no MCP. |
| **Sistema de 2 paneles (M1)** | `desktop/ui/src/main.ts`, `panelChat.ts` | Layout extensible: `#paneles` flex container. Se duplica el patrón para crear `panelNavegador.ts`. |
| **Evento Tauri** | `desktop/src-tauri/src/main.rs` | `agente-evento` con tag `tipo` y variantes. Se añaden variantes `CapturaPantalla`, `ToolBrowser`. |
| **Persistencia SQLite** | `cli/src/persistencia_sqlite.rs` | Historial durable; almacenamiento de capturas como mensajes de sistema si aplica. |
| **Tool `web_fetch`** | `core/src/herramientas/tools_web.rs` | Complementa, no sustituye al navegador. |
| **IPC backend existente** | `desktop/src-tauri/src/main.rs` | 17 comandos IPC para la UI. Se añaden comandos para el panel; la tool del agente usará un puerto Rust compartido. |
| **NUEVO: `Window::add_child()`** | Tauri 2 `feature = "unstable"` | Crea un segundo WebView dentro de la misma ventana. **Corazón del navegador visible.** |
| **NUEVO: `Webview::with_webview()`** | Tauri 2 `feature = "unstable"` | Da acceso a `PlatformWebview` → en Windows, al `ICoreWebView2Controller` y `ICoreWebView2` nativos. |
| **NUEVO: `ICoreWebView2::CapturePreview`** | Windows COM (vía `webview2-com` crate) | Screenshot nativo del WebView. Sin CDP, sin DevTools, sin subproceso. |
| **NUEVO: `CallDevToolsProtocolMethod`** | Windows COM (CDP) | Control completo: navegación, click, fill, DOM, evaluación JS. |
| **NUEVO: `AddHostObjectToScript`** | Windows COM | Bridge de objetos Rust expuestos al JS de la página. |

### Qué NO existe y hay que crear

| Ausencia | Impacto |
|---|---|
| **Feature `unstable` en Tauri** | Activada en `desktop/src-tauri/Cargo.toml`; `Window::add_child()` y `Webview::with_webview()` compilan con la versión fijada. La prueba runtime sigue pendiente. |
| **Tool `navegador_reflejo` en el núcleo** | Herramienta nativa (no MCP) que expone navegación, click, fill, screenshot y snapshot mediante `PortNavegador`; no llama IPC ni frontend TS. |
| **Comandos IPC del navegador** | `navegador_navegar`, `navegador_click`, `navegador_rellenar`, `navegador_capturar`, `navegador_snapshot`, `navegador_js`, `navegador_abrir`, `navegador_cerrar` — nuevos comandos Tauri que manipulan la webview hija. |
| **Evento `CapturaPantalla`** | `AgenteEvento` no transporta base64. Nueva variante. |
| **Evento `ToolBrowser`** | Evento para mostrar acciones del agente en vivo en la UI. |
| **Panel de navegador en UI** | Contenedor para la webview hija + overlay de anotaciones + log de acciones. |
| **Overlay de anotaciones** | Canvas HTML sobre la webview, toggle para modo anotación. |
| **Capability Tauri `navegador:default`** | `default.json` necesita capability para la webview hija con permisos restringidos. |
| **Pipeline multimodal** | El agente recibe la captura como imagen (si el proveedor LLM soporta `image_url`), o como snapshot de texto (DOM accesible). |
| **Permiso/categoría `navegación`** | Las tools del navegador necesitan categoría propia para aprobación del usuario. |

---

## 2. Arquitectura propuesta (v2 — WebView2 nativo)

```text
┌──────────────────────────────────────────────────────────────────┐
│  desktop (Tauri 2 + feature "unstable")                          │
│                                                                  │
│  ┌─────────────────────────┐   ┌──────────────────────────────┐  │
│  │ PanelChat               │   │ PanelNavegador               │  │
│  │  - historial            │   │  ┌────────────────────────┐  │  │
│  │  - entrada              │   │  │ Webview2 hijo (visible)│  │  │
│  │  - aprobación           │   │  │  - scroll/interacción  │  │  │
│  └──────────┬──────────────┘   │  │  - CapturePreview →    │  │  │
│             │                  │  │    agente (base64)     │  │  │
│             │                  │  └────────────────────────┘  │  │
│             │                  │  ┌────────────────────────┐  │  │
│             │                  │  │ overlay anotaciones    │  │  │
│             │                  │  │  (canvas, toggle)      │  │  │
│             │                  │  └────────────────────────┘  │  │
│             │                  │  ┌────────────────────────┐  │  │
│             │                  │  │ Log acciones del agente│  │  │
│             │                  │  └────────────────────────┘  │  │
│  ┌──────────▼──────────────────▼─────────────────────────────┐  │
│  │ main.ts (orquestador) + real.ts (adaptador)               │  │
│  │  - estado M1 compartido (modelo/modo)                     │  │
│  │  - turno global único                                     │  │
│  └──────────────────────────┬────────────────────────────────┘  │
│                             │ invoke("navegador_*")             │
│                             │ (solo controles del panel/UI)     │
│  ┌──────────────────────────▼────────────────────────────────┐  │
│  │ main.rs (backend Tauri)                                   │  │
│  │  - Sesion { runtime, persistencia, vault }                │  │
│  │  - Estado { sesion, turno }                               │  │
│  │  - 17+ comandos IPC existentes                            │  │
│  │  - NUEVOS: navegador_navegar, navegador_click,            │  │
│  │    navegador_rellenar, navegador_capturar,                │  │
│  │    navegador_snapshot, navegador_js, navegador_abrir       │  │
│  └──┬────────────────────────────────────────────────────┬───-┘  │
│     │                                                    │       │
│  ┌──▼──────────────────────────┐   ┌────────────────────▼─────┐  │
│  │ webview hija (Window::      │   │ CoreWebView2 nativo     │  │
│  │   add_child)                │   │  - CapturePreview       │  │
│  │  - navegación real          │   │  - CallDevToolsProtocol  │  │
│  │  - visible en el panel      │   │  - AddHostObject         │  │
│  │  - el usuario interactúa     │   │  - ExecuteScript        │  │
│  │  - el agente controla por   │   └──────────────────────────┘  │
│  │    CDP/JS                    │                                 │
│  └─────────────────────────────┘                                   │
└──────────────────────────────────────────────────────────────────┘
```

### Flujo típico (v2 — WebView2 nativo)

1. Usuario abre el navegador interno o pide al agente investigar algo
2. La UI llama `invoke("navegador_abrir", { url: "https://..." })` para navegación manual → backend crea la webview hija vía `Window::add_child()` → navega; el agente usa `PortNavegador` desde el runtime y no pasa por el frontend.
3. La webview hija se renderiza **en vivo** en `PanelNavegador`. El usuario ve la página, puede hacer scroll, hover, clic directo.
4. El agente llama `navegador_navegar({ url: "..." })` → backend emite evento `ToolBrowser { accion: "navigate", url: "..." }` a la UI → el log muestra "🔍 Navegó a X"
5. Agente llama `navegador_click({ selector: "#precio" })` → CDP `Input.dispatchMouseEvent` o JS → evento `ToolBrowser { accion: "click" }` → la UI resalta dónde clickeó
6. Agente necesita ver la página → llama `navegador_capturar` → backend llama `CapturePreview` → base64 → evento `CapturaPantalla` → la UI recibe la captura (el usuario ya la ve en vivo)
7. Usuario activa overlay de anotaciones → dibuja flecha/círculo sobre la webview → guarda → se envía al agente como JSON
8. Agente recibe captura + anotaciones + snapshot de accesibilidad (DOM textual) y continúa

### Dualidad agente↔usuario (v2)

- **El usuario ve el navegador en vivo.** No necesita esperar capturas. Puede hacer scroll, hover, clic directamente.
- **El agente ve capturas** (PNG de `CapturePreview`) + snapshot de accesibilidad (DOM). No ve la página en vivo.
- **El agente controla** por CDP/JS. El usuario puede estar interactuando simultáneamente (riesgo de contención — ver §Riesgos).
- **Anotaciones del usuario** sobre la webview (canvas — **riesgo confirmado de z-order**, ver sección correspondiente). Se envían al agente como contexto.

### Anotaciones del usuario (v2: overlay sobre la webview — riesgo técnico)

- Canvas HTML5 superpuesto (`position: absolute`) sobre la webview hija.
- Herramientas: flecha, círculo, rectángulo, texto, resaltador.
- Al activar anotaciones, el overlay captura eventos del ratón. La webview deja de recibir input hasta desactivar.
- Al guardar: `Anotacion[]` → backend → próximo turno del agente.
- **Riesgo técnico confirmado (corrección v3):** La webview hija de Tauri 2 es un child HWND nativo (Windows) que se renderiza fuera del DOM de la ventana principal. Un canvas HTML con `position: absolute` no puede garantizar superposición sobre un HWND nativo porque el z-order lo gestiona el sistema de ventanas, no CSS. **Esto no se ha prototipado.**
  - Alternativas si falla: (a) overlay en una segunda ventana transparente flotante; (b) anotaciones sobre la captura (no en vivo, como ChatGPT Codex); (c) integrar como capa WebView2 interna vía `SetHostObject` + JS injection.
  - Documentar en F4 como riesgo técnico con prototipo obligatorio antes de implementar.
  - Por ahora, F4 se describe asumiendo que funciona; si el prototipo falla, la implementación real será sobre captura.

---

## 3. Presupuesto de RAM y restricciones (v2 — sin Obscura)

| Componente | RAM estimada | Notas |
|---|---|---|
| Desktop actual (WebView2 + Rust) | ≤ 140 MB (medido F6: 40 MB WS release) | Objetivo cumplido |
| WebView2 hijo (página pesada) | ≤ 30 MB adicional (estimado, NO verificado) | WebView2 es multiproceso: browser + renderer + GPU. Una webview hija puede añadir procesos hijos Chromium, no solo heap compartido. Medir con `Get-Process`. |
| Panel navegador + overlay | ~ 3 MB | DOM ligero, canvas nativo |
| Captura en memoria (base64) | ≤ 5 MB (pico, una captura a la vez) | Se descarta al reemplazar |
| **Total estimado (sin verificar)** | **≤ 178 MB** | Medición real pendiente. Si el multiproceso añade >30 MB, ajustar objetivo. |

**Restricciones:**
- Cero timers en background (regla del área). El navegador solo consume CPU cuando el agente o el usuario lo usan. Sin embargo, la webview hija puede mantener timers/websockets de la página cargada aunque no se interactúe (p.ej. React polling, Google Analytics). El agente debe cerrar la webview (`navegador_cerrar` → destroy) cuando termine, no solo ocultarla: `hide()` no libera procesos hijos ni detiene JS en ejecución.
- La webview hija existe mientras el panel está abierto. Al cerrar el panel, se destruye (no solo oculta).
- Captura única a la vez (no videostream): el agente pide screenshot cuando necesita ver el estado.
- Build en `C:\tmp`, límite 7 GB.

---

## 4. Fases del plan (v2 — WebView2 nativo, en orden de dependencia)

### Arquitectura de fases rediseñada:

El cambio de Obscura a WebView2 nativo elimina las fases de MCP + subproceso externo. Las nuevas fases son:

| Fase | Antes (Obscura) | Ahora (WebView2) |
|---|---|---|
| F1 | Cablear MCP + lanzar Obscura | **Feature `unstable` + webview hija (`add_child`)** |
| F2 | Evento CapturaPantalla + IPC | **Comandos IPC del navegador (navegador_*)** |
| F3 | Panel navegador en UI | **Panel navegador + webview hija visible** |
| F4 | Anotaciones del usuario | **Overlay de anotaciones + ToolBrowser eventos** |
| F5 | Visualización del agente controlando | **Integración agente → herramientas nativas** |
| F6 | Gestión ciclo de vida Obscura | **Pipeline multimodal opcional (diferible)** |

---

### Fase 1 — Feature `unstable` en Tauri + webview hija (`add_child`)

**Esfuerzo: medio** (cambiar Cargo.toml, probar `add_child`, exponer webview hija)

- [x] Activar `features = ["unstable"]` en la dependencia `tauri` de `desktop/src-tauri/Cargo.toml`; la compilación confirmó la API con la versión fijada y no requirió añadir `tauri-runtime` como dependencia directa.
- [x] Crear módulo `desktop/src-tauri/src/navegador.rs` con el estado de la webview hija:
  ```rust
  struct EstadoNavegador {
      webview: Option<tauri::Webview>,
  }
  ```
- [x] Función `crear_webview_hija(window: &Window, url: &str) -> Result<Webview>`:
  - Llama `Window::add_child()` (feature `unstable`) con tamaño/posición inicial.
  - Devuelve el `Webview` para almacenar en estado.
  - Maneja error si `unstable` no está disponible.
- [x] Comando IPC `navegador_abrir(url: String)`: llama `crear_webview_hija` y guarda en estado. La emisión de eventos queda para F4, cuando exista la variante `ToolBrowser`.
- [x] Comando IPC `navegador_cerrar`: cierra la webview hija y libera su estado. `hide()` queda reservado para ocultación temporal: no equivale a destruir ni garantiza detener scripts, timers o procesos hijos.
- [x] Comando IPC `navegador_redimensionar(ancho: u32, alto: u32)`: cambia tamaño de la webview hija (para responsive).
- [ ] Capability Tauri nueva `navegador.json`:
  ```json
  {
    "identifier": "navegador:default",
    "windows": ["main"],
    "permissions": ["core:default"]
  }
  ```
  - La webview hija hereda capabilities de la ventana principal (diseño Tauri 2: `add_child` comparte window).
  - Si es necesario: `remote: { urls: ["https://*"] }` para conceder IPC a páginas de origen remoto (NO para habilitar navegación: `remote` controla acceso IPC, no la capacidad de navegar que ya tiene la webview por defecto).
- [ ] Verificación: llamar `navegador_abrir("https://example.com")` → se ve el webview cargando la página en la ventana.
- [ ] Tests: integración con mock de Tauri (o test manual, porque `add_child` es nativo de ventana).

### Fase 2 — Comandos IPC del navegador (control CDP/JS)

**Esfuerzo: alto** (acceder a CoreWebView2 nativo, implementar CapturePreview y CDP)

- [x] Usar `Webview::with_webview(|pw| ...)` para acceder al `PlatformWebview` nativo.
  - En Windows: `pw` expone el controller nativo y se obtiene `ICoreWebView2` mediante `controller.CoreWebView2()`.
- [x] Comando IPC `navegador_navegar(url: String)`: llama `webview.navigate()` con URL validada.
- [x] Comando IPC `navegador_capturar() -> String`: llama `CapturePreview` PNG, lee el `IStream`, limita a 16 MiB y devuelve Base64.
  - En macOS/Linux queda stub explícito: la implementación nativa actual es Windows-first.
  - **Fallo controlado**: si no hay webview hija, devuelve error explícito.
- [x] Comando IPC `navegador_cdp(metodo: String, params: String) -> String`: llama `CallDevToolsProtocolMethod`, valida método/parámetros JSON y aplica límites.
- [x] Comando IPC `navegador_js(codigo: String) -> String`: evalúa JavaScript con `ExecuteScript`, con límite de 128 KiB y timeout de 15 s.
- [x] Comando IPC `navegador_snapshot() -> String`: ejecuta `document.body.innerText` y limita la respuesta a 256 KiB.
- [x] Comando IPC `navegador_click(selector: String)`: ejecuta un click sobre el primer elemento encontrado, serializando el selector como JSON.
- [x] Comando IPC `navegador_rellenar(selector: String, valor: String)`: actualiza el control y emite eventos `input`/`change`, serializando ambos valores como JSON.
- [x] **Nota de seguridad**: todos los comandos validan la existencia de la webview; URL, selector, código, valor, CDP y captura tienen límites. Los errores COM, callbacks cerrados y timeouts se devuelven explícitamente.
- [ ] Verificación runtime: llamar cada comando y verificar comportamiento real en la webview visible.
- [ ] Tests: unitarios de la lógica de estado; integración manual con página de prueba HTML conocida.

### Fase 3 — Panel de navegador en UI (`panelNavegador.ts`) + webview visible

**Esfuerzo: medio** (reutiliza patrón de `panelChat.ts`)

- [ ] Nueva fábrica `montarPanelNavegador(opts)` en `desktop/ui/src/componentes/panelNavegador.ts`:
  - **Región de la webview**: `<div id="webview-contenedor">` representa la geometría lógica del área. La webview hija es un child webview nativo posicionado por Tauri/OS; no se asume que Tauri la inyecte dentro del DOM ni que el div sea un ancla de z-order.
  - **Barra de URL**: `<input>` + botón "Ir"/Enter para navegación manual del usuario.
  - **Botones**: Atrás, Adelante, Recargar, Capturar (forzar screenshot).
  - **Log de acciones del agente**: `<ol>` con cada tool llamada: "🔍 Navegó a X", "🖱️ Click en Y".
  - **Anotaciones** (vacías inicialmente, se activan con botón "Anotar"). La estrategia de overlay queda condicionada al prototipo de z-order de F4; si el canvas DOM no puede cubrir el child webview, se usará ventana transparente o anotación sobre captura.
  - **Botón "Cerrar navegador"**: destruye la webview hija.
  - API: `{raiz, setUrl(url), setNavegando(bool), agregarAccion(texto, icono), setAnotaciones(modo)}`.
- [ ] Integrar en `main.ts`:
  - Nueva opción en la sidebar ("Navegador") que abre/cierra el panel.
  - Al abrir: llama `invoke("navegador_abrir")` con URL por defecto (blank o página de inicio configurable).
  - Al cerrar: llama `invoke("navegador_cerrar")`.
  - Conectar eventos `ToolBrowser` → `panelNavegador.agregarAccion()`.
- [ ] CSS: la webview hija debe ocupar el 100% del contenedor. Estilo monocromo, barra URL 1px border, log opacity .5.
- [ ] Verificación: abrir panel → se ve webview cargando página → barra URL muestra la URL correcta → botón atrás funciona.

### Fase 4 — Anotaciones del usuario sobre la webview + ToolBrowser eventos

**Esfuerzo: medio** (canvas overlay, herramientas de dibujo, eventos en vivo)

- [ ] `desktop/ui/src/componentes/anotaciones.ts`: fábrica `montarAnotaciones(opts)`:
  - Canvas HTML5 del mismo tamaño que el área lógica, `position: absolute` solo como primera hipótesis; el prototipo debe confirmar que puede cubrir el child webview nativo.
  - Herramientas: **flecha** (click+arrastre), **círculo** (click+arrastre), **rectángulo**, **texto** (click → input), **resaltador** (pincel semitransparente).
  - Al activar modo anotación, el canvas captura eventos del ratón si el z-order lo permite. En caso contrario, la interacción se resolverá con la estrategia alternativa elegida por el prototipo; no se afirma todavía que la webview hija deje de recibir input.
  - Al guardar: `Anotacion[]` como JSON `{ tipo, x, y, w, h, texto?, color }` → backend → próximo turno del agente.
  - API: `{raiz, setTamano(w,h), limpiar(), getAnotaciones(), activar(bool), estaActivo()}`.
- [ ] Evento `ToolBrowser` v2 (variante en `AgenteEvento`):
  ```rust
  ToolBrowser {
      accion: String,       // "navigate" | "click" | "fill" | "screenshot" | "snapshot"
      url: Option<String>,
      selector: Option<String>,
      resultado: String,     // resumen para la UI
  }
  ```
  - Emitir desde los comandos IPC (F2) cuando el agente los invoca.
  - La UI recibe y muestra en el log con icono y timestamp.
- [ ] Resaltado momentáneo en la webview: cuando el agente hace click, el backend puede ejecutar un breve resaltado CSS en la webview (`ExecuteScript("element.style.outline='2px solid red'; setTimeout(()=>element.style.outline='', 1000)")`).
- [ ] Verificación: primero probar z-order, hit-testing y foco con una fixture local; solo si el overlay DOM funciona, abrir webview → activar anotaciones → dibujar flecha → guardar → JSON correcto. Si falla, implementar la alternativa elegida (ventana transparente o captura) y repetir la prueba.

### Fase 5 — Herramienta nativa `navegador_reflejo` en el núcleo

**Esfuerzo: alto** (crear herramienta en core que el agente use como tool nativa, no MCP)

- [ ] Nueva herramienta en `core/src/herramientas/tools_navegador.rs`:
  ```rust
  pub struct ToolNavegador;
  
  impl Tool for ToolNavegador {
      fn nombre(&self) -> &str { "navegador_reflejo" }
      fn descripcion(&self) -> &str { "Controla el navegador interno visible. Acciones: navigate, click, fill, screenshot, snapshot, js. El usuario ve los cambios en vivo." }
      fn parametros(&self) -> ai_hub::Parametros { ... }
      fn ejecutar(&self, args: &Map<String, Valor>, ...) -> Result<String> { ... }
  }
  ```
  - Parámetros: `{ accion: "navigate" | "click" | "fill" | "screenshot" | "snapshot" | "js", url?: string, selector?: string, valor?: string, codigo?: string }`.
  - Ejecución: usa `PortNavegador` inyectado en `AgentToolContext`; el backend Tauri implementa el puerto sobre CoreWebView2.
- [ ] **Decisión de arquitectura (corrección v3):** La herramienta `navegador_reflejo` vive en `core/` (Rust puro, sin dependencia Tauri), pero necesita hablar con la webview hija que solo existe en `desktop/` (backend Tauri). `RealAdapter::invoke_navegador` en TS (`desktop/ui`) implicaría enrutar la tool del núcleo por el frontend y se descarta. La solución prevista es:
  - Definir un trait `PortNavegador` en `core/src/ports/` con métodos como `navegar(url)`, `capturar() -> Vec<u8>`, `ejecutar_js(codigo) -> String`.
  - El backend Tauri (`desktop/src-tauri/src/main.rs`) implementa `PortNavegador` usando CoreWebView2.
  - El runtime del desktop inyecta el `PortNavegador` en `AgentToolContext` cuando la sesión tiene webview hija.
  - `ToolNavegador::ejecutar` llama a `ctx.navegador.navegar(url)` sin pasar por IPC ni TS.
  - Esto elimina el rodeo `core → IPC (TS) → invoke → backend`.
- [ ] No implementar `RealAdapter::invoke_navegador(...)` para la tool del agente: `desktop/ui` queda limitado a la navegación manual, el panel y la presentación de eventos. Si el panel necesita IPC, sus comandos no constituyen el canal de ejecución de `ToolNavegador`.
- [ ] Verificación: el agente puede llamar `navegador_reflejo` con `{ accion: "navigate", url: "..." }` → webview navega → usuario lo ve.
- [ ] Tests: unitarios del parseo de args; prueba del puerto con fake `PortNavegador` y prueba de integración del backend con una página fixture local (sin depender de una web externa).

### Fase 6 — Pipeline multimodal (opcional, diferible)

**Esfuerzo: alto** (depende del proveedor LLM)

- [ ] Evaluar si el proveedor LLM activo (gloryapi/commandcode) soporta `image_url`.
- [ ] Si sí: el agente recibe la captura en el prompt (`AiMessage::contenido` con `Role::User`, bloque `image_url`).
- [ ] Si no: el agente solo recibe snapshot de texto (DOM accesible de `navegador_snapshot`).
- [ ] Por ahora: **el agente recibe snapshot de texto** (DOM) y la captura es para el usuario. La imagen solo entra al prompt cuando el proveedor LLM lo soporte.
- [ ] Documentar limitación y dejarlo como mejora futura.

---

## 5. Gate y evidencia (v2)

- **Gate canónico:** el proyecto declara `sentinel check --` en `sentinel.config.json` y exige `taskIdRequired`; antes de ejecutar el gate hay que generar/confirmar el manifest de etapas que acepte Sentinel 0.7.8. No se debe ejecutar literalmente `sentinel check 069A-1 --stages ...` sin verificar la ayuda y el adapter local.
- **Doctor previo:** `npm run quality:doctor` y `git status --short --branch`; el cierre exige `readyForGate: true`, política válida, commits/lock alineados y reporte estructurado.
- **Rama primaria:** `main`
- **Evidencia esperada por fase:**
  - F1: feature `unstable` compila; `navegador_abrir` crea webview visible.
  - F2: cada comando IPC responde correctamente; `capturar` devuelve base64 válido.
  - F3: panel muestra webview + barra URL + log.
  - F4: anotaciones se serializan; eventos ToolBrowser se muestran en vivo.
  - F5: el agente llama `navegador_reflejo` y la webview responde.
  - F6: documentación del soporte multimodal actual y decisión explícita sobre captura/imagen.
- **Type-check:** `cd desktop/ui; npx tsc --noEmit`
- **Build UI:** `cd desktop/ui; npm run build`
- **Tests Rust:** `cargo test --workspace`
- **Clippy:** `cargo clippy -p glory-harness -p glory-harness-core --all-targets -- -D warnings`
- **RAM:** medir con `Get-Process` antes/después de cada fase.

## 6. Riesgos y mitigaciones (v2)

| Riesgo | Impacto | Mitigación |
|---|---|---|
| `Window::add_child()` requiere `feature = "unstable"` que puede cambiar en futuras versiones de Tauri | Medio | Documentar dependencia; si Tauri estabiliza la API sin cambios, actualizar; si rompe, migrar a API estable. |
| CoreWebView2 `CapturePreview` no funciona en webview hija (límite de seguridad) | Alto | Prototipar F2 primero con capture en la webview principal; si falla en hija, usar CDP `Page.captureScreenshot` vía `CallDevToolsProtocolMethod`. |
| Acceso a `with_webview` requiere `unstable` y puede no estar disponible en todas las plataformas | Alto | Envolver en `#[cfg(windows)]`; en macOS/Linux usar webkit2gtk nativo (Tauri ya abstrae). El plan es Windows-first. |
| Contención entre input del usuario y control del agente (ambos operan sobre la misma webview) | Medio | El agente solo controla por CDP/JS, no bloquea el input del usuario. Documentar que si el usuario y el agente interactúan simultáneamente, el resultado puede ser impredecible. Usar modo "bloqueo de input" opcional. |
| Anotaciones canvas se ven mal o no responden | Alto | No asumir que CSS cubre el child HWND. F4 debe ejecutar un prototipo de z-order/input; si falla, elegir ventana transparente o anotación sobre captura antes de implementar herramientas de dibujo. |
| El agente no descubre las tools del navegador (schema requiere descripciones) | Bajo | La tool `navegador_reflejo` tiene schema fijo en código. Se documenta en `tool.rs`. |
| **C:\tmp\glory-target** supera 7 GB por compilar con `unstable` | Bajo | `cargo build` usa `C:\tmp`; compilación incremental. Sin riesgo adicional. |
| CDP via `CallDevToolsProtocolMethod` puede ser lento o no soportar ciertos métodos | Medio | Tener fallback a `ExecuteScript` para manipulación DOM básica. CDP es el canal principal. |
| La webview hija puede crashear independientemente (cada webview2 tiene su proceso renderer) | Medio | WebView2 asigna proceso renderer independiente por webview. Si la hija crashea, la principal sobrevive y viceversa. El manejador `webview2.ProcessFailed` permite recargar la webview hija automáticamente. |

---

## 7. Queda fuera del alcance (actualizado v2)

- Videostream en vivo del navegador (solo capturas bajo demanda del agente, el usuario ve en vivo).
- Obscura como motor headless (opcional diferido; el plan usa WebView2 nativo).
- Múltiples navegadores simultáneos (M1: un solo navegador, un solo turno).
- Navegador en el CLI/REPL (solo en desktop Tauri).
- Integración con DevTools/consola JS del navegador.
- Proxy/stealth/anti-detect.
- Soporte macOS/Linux nativo (Windows-first, el plan asume `#[cfg(windows)]` para el acceso a CoreWebView2).

---

## 8. IDs y roadmap (v2)

- **ID del plan base:** 069A-1 (refinado v2 2026-09-06)
- **Fases:** 069A-1.1 a 069A-1.6. Cada fase produce un commit con su ID.
- Al cerrar el plan base, mover a `Agente/planes/completados/` y registrar evidencia en `Agente/completados/tareas-YYYY-MM-DD.md`.
- Enlazar desde `roadmap.md` como plan activo tras aprobación del usuario.
- Cada fase tiene su propio gate `sentinel check 069A-1.<F>`.

## 9. Veredicto de arquitectura (supervisor-thinking, v3)

**VEREDICTO: VIABLE CON RESERVAS.**

### Problema y no-goals

- **Resultado:** navegador visible e interactivo dentro de Glory Harness, controlable por el agente y observable por el usuario, sin introducir Obscura como proceso externo.
- **No-goals:** navegador CLI, múltiples sesiones simultáneas, videostream, proxy/stealth, DevTools expuesto al usuario y soporte nativo macOS/Linux en este bloque.

### SOLID y núcleo

- **SRP:** separar ciclo de vida/geometría de la child webview, adaptador CoreWebView2, comandos IPC del panel y tool del agente.
- **DIP:** `core` define únicamente `PortNavegador` si el segundo consumidor real es la sesión desktop; Tauri implementa el puerto. No introducir dependencia de Tauri ni de TypeScript en `core`.
- **YAGNI:** no añadir `AddHostObjectToScript` ni una abstracción multiplataforma hasta que el prototipo F1/F2 demuestre su necesidad. CDP/`ExecuteScript` cubren el primer slice.
- **Contrato de contexto:** `AgentToolContext` hoy expone puertos concretos; la inyección de `PortNavegador` debe quedar diseñada y compilada antes de F5. Si el contrato no permite un puerto opcional sin ampliar el alcance, F5 se separa como tarea de integración.

### Eficiencia, rendimiento y escalabilidad

- Modelo M1: una child webview, un turno de agente y capturas bajo demanda. No se promete concurrencia.
- WebView2 es multiproceso; el presupuesto de 178 MB es una hipótesis, no un límite probado. Medir el árbol de procesos WebView2 y el working set/PSS disponible en Windows con página fixture ligera y pesada.
- Limitar tamaño de URL, código JS, selector, snapshot, captura y tiempo de cada operación. No acumular base64 en historial; descartar la captura después de entregarla salvo que exista una decisión explícita de persistencia.
- Las páginas remotas pueden mantener timers, workers y websockets. `navegador_cerrar` debe destruir la webview; ocultarla no es una política de suspensión.

### Seguridad

- Validar URL y permitir únicamente esquemas explícitos (`https`, y `http` solo si se documenta para fixtures locales). Rechazar `file:`, `javascript:`, `data:` y navegación fuera de la política.
- No interpolar selectores/valores/código directamente en scripts: serializar argumentos como JSON y aplicar límites. La capacidad `remote` no se usa como permiso de navegación.
- `navegador_js` y CDP son superficie de ejecución arbitraria dentro de la página: mantenerlos fuera del schema público o exigir aprobación explícita por acción. No exponer secretos del host a páginas remotas.
- Aplicar timeout, cancelación y estado explícito para webview ausente, navegación pendiente, proceso fallido y captura concurrente.

### Diseño UI / reutilización

- Reutilizar el orquestador de `main.ts`, el patrón de `panelChat.ts`, los tokens CSS existentes y el sistema actual de eventos Tauri. No afirmar que un `<div>` DOM contiene a la child webview: su geometría se sincroniza con el backend/OS.
- El overlay canvas es una hipótesis bloqueada por z-order. F4 empieza con un prototipo de hit-testing y superposición; si falla, escoger la alternativa antes de crear componentes de dibujo.
- Estados obligatorios: cargando, vacío, error de navegación, proceso caído, captura en curso, anotación activa/inactiva, foco y teclado.

### Mitigaciones de riesgo

1. **API Tauri/feature unstable:** F1 compila un spike mínimo y fija las firmas/versiones reales antes de diseñar módulos.
2. **CoreWebView2 y captura:** F2 usa una página fixture local; si `CapturePreview` no funciona, cambia a CDP `Page.captureScreenshot` manteniendo el contrato de captura.
3. **Z-order/input:** F4 no se considera terminado sin evidencia de superposición e interacción; si falla, usar ventana transparente o captura.
4. **Routing core/backend:** F5 no usa `RealAdapter`; define fake/trait en `core` y una implementación Tauri, con evento UI separado.
5. **RAM/ciclo de vida:** medir árbol completo, cerrar por destroy y registrar el resultado por fase; no declarar PASS por estimación.
6. **Páginas no confiables:** navegación allowlisted, sanitización contextual, permisos mínimos y aprobación para JS/CDP.

### Documentación / entropía

- Mantener este plan como fuente activa hasta la aprobación técnica.
- Tras el primer bloque ejecutable, actualizar `roadmap.md` con 069A-1 y registrar la evidencia en `Agente/completados/` solo al cerrar una fase real.
- Si el prototipo revela una limitación reusable de Tauri/WebView2, actualizar `Agente/documentacion/`; no duplicar el hallazgo en varios manuales.

### Gate / evidencia

- Preflight: `npm run quality:doctor`, estado Git y versiones/ayuda del Sentinel fijado.
- Validación proporcional: `cargo check`/tests del workspace, type-check/build UI, fixture local WebView2 y prueba funcional de captura, navegación y destroy.
- Gate de cierre: usar el contrato real de `sentinel.config.json` y el adapter local; requiere `readyForGate: true`, reporte asociado al commit y no afirmar PASS sin las etapas ejecutadas.

### Criterios de aceptación del diseño

- F1 demuestra child webview visible y redimensionable con la versión fijada de Tauri.
- F2 navega una fixture local, ejecuta una operación estructurada, captura PNG/JPEG y devuelve errores explícitos.
- F3 muestra la webview en vivo sin tratarla como visor de screenshots.
- F4 demuestra overlay/input o documenta la alternativa elegida con prueba real.
- F5 ejecuta la tool por `PortNavegador` sin roundtrip por TypeScript.
- RAM y ciclo de vida se reportan con medición reproducible del árbol de procesos.

### Estado de autorización

- **AUTORIZADO PARA EJECUTAR:** edición local del plan, spike F1, pruebas locales, validación de UI, gate y commits coherentes.
- **NO AUTORIZADO POR ESTE VEREDICTO:** push, deploy, escrituras remotas, producción o SSH.

### SIGUIENTE ACCIÓN

Completar F2 en Windows: exponer `navegador_capturar`, CDP/JS y acciones estructuradas con APIs reales de CoreWebView2, después ejecutar `tauri dev` con una fixture local para verificar creación visible, navegación, redimensionado, captura, cierre y árbol de procesos WebView2. No avanzar a F3/F5 con una validación solo de compilación.

## 10. Lo que hay que decidir (preguntas al usuario, v3)

1. **WebView2 via `add_child` + `unstable` como motor principal.** ¿Aprobado? (ESTO SUSTITUYE a Obscura como motor principal; Obscura queda como alternativa headless diferida.)
2. **¿Arrancar con F1+F2 (feature `unstable` + comandos IPC) que dan la base técnica?** F3 (panel UI) depende de F1.
3. **Windows-first vs multiplataforma desde el inicio.** Propongo Windows-first: implementar `navegador_capturar` vía CoreWebView2 (`#[cfg(windows)]`), y dejar macOS/Linux como TODO documentado para una fase posterior.
4. **Anotaciones: ¿overlay sobre la webview (el usuario anota directamente sobre la página en vivo) o sobre una captura (como ChatGPT Codex)?** Propongo overlay sobre la webview en vivo, toggle activación. Es más potente.
5. **¿Incluir F5 (herramienta `navegador_reflejo` en el núcleo) en el plan base o diferir?** Propongo incluirla, porque sin ella el agente no puede controlar el navegador. Es la pieza que cierra el círculo agente↔webview.