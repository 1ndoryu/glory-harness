# Auditoría SOLID/arquitectura — código nuevo 069A-2 + 079A-1 (F7)

- **Fecha:** 07-09-2026 · **Plan:** `plan-deuda-cero-079A-1-2026-09-07.md`, fase F7.
- **Base:** árbol post-F2–F5b/F6 (commits `8024e08`, `cff9289`, `a787146`,
  `16fadef`, `922cf9a`, `baa7a2e`, `3db5f32`; Sentinel repineado `065b445`).
- **Método:** el de S5 (05-09): por pieza con `ruta:línea`, clasifica
  `[ya corregido]` / `[corregir aquí]` / `[decisión]`; **cero cambios de
  comportamiento en este paso**. Cubre solo lo nuevo desde S5: web SSE,
  `web_datos/`, `stream.rs`, navegador (core + desktop), splits desktop
  (turno/sesión/conversaciones/workspaces), `SesionComun`. Lo ya auditado en
  S5 no se re-abre salvo regresión del split.

---

## 1. `SesionComun` — SRP/DIP del ciclo de sesión compartido

**Evidencia:** `cli/src/servicio/sesion.rs:114` (`pub struct SesionComun`),
`abrir` (`:142`), `abrir_con_persistencia` (`:162`), `info` (`:247`),
`reconfigurar` (`:267`), `cambiar_workspace` (`:300`), `preparar_turno`
(`:349`), `cancelar_turno` (`:415`). Los tres consumidores (CLI `chat`/`run`,
web `SesionWeb.comun`, desktop `Sesion.comun`) comparten construcción,
apertura, reconfiguración y preparación de turno; cada consumidor conserva
fuera solo su ciclo de vida (Tauri `Mutex<SesionComun>`, web
`Mutex<SesionComun>` + `tx`, CLI efímera).

**Veredicto:** `[ya corregido]` (069A-2 v3). DIP correcto: el servicio común
no conoce transporte; HTTP y Tauri son adaptadores finos. No hay lógica de
sesión duplicada entre `web.rs` y `main.rs`.

---

## 2. `web.rs` — SRP del servidor + fan-out SSE

**Evidencia:** `cli/src/comandos/web.rs` — auth (`bearer` `:148`,
`cookie_sesion` `:158`, `credencial` `:175`, `origen_valido` `:197`,
`autorizar_sesion` `:229`), cable SSE (`cable` `:274`,
`ready_json_data` `:439`, `partir_cable` `:443`), handlers
(`crear_sesion` `:287`, `actualizar_meta` `:361`, `cerrar_sesion` `:383`,
`eventos_sse` `:395`), `router` `:460`, `run` `:525`. Estado por sesión en
`SesionWeb` (`:78`: `comun` + `conversacion_id` + `tx` + `turno`).

**SRP:** el archivo mezcla transporte (axum), auth y estado de sesión, pero
cada responsabilidad tiene su bloque y los handlers de dominio ya salieron a
`web_datos/` (§3). El `run` (`:525`) devuelve `ExitCode` y respeta el flag
`--fixture` (gaps F1 cerrados en F2).

**Punto abierto (no de esta fase):** el fan-out usa `broadcast::Sender`
(`:87`, `:316`) — F1 pendiente (`broadcast-mutex-riesgo-rs` ×2, únicos
errores del gate). El diseño es consciente (buffer 256 + evento `lagged` en
`:424-426`, snapshot `ready` en `:405-416` sin replay), así que F1 es solo
cambio de transporte, no de protocolo.

**Veredicto:** `[decisión — no corregir aquí]`. Cohesivo como "frontera HTTP";
el split de dominio ya se hizo (`web_datos/`); F1 queda agendada y bloqueada
por colisión ajena, no por diseño.

---

## 3. `web_datos/` — SRP por dominio (F3 079A-1)

**Evidencia:** `cli/src/comandos/web_datos/` — `mod.rs` (95: hub +
`area_activa` + `turno_en_curso` + `sesion_y_comun` + re-exports),
`conversaciones.rs` (238), `configuracion.rs` (177), `areas.rs` (240),
`pruebas.rs` (313, `#![cfg(test)]`). Los handlers se importan desde
`super::web` (`mod.rs:27`) y `SesionComun` (`mod.rs:28`): el hub depende del
contrato, no al revés.

**Veredicto:** `[ya corregido]` (F3). Cada fichero = un dominio; el hub
conserva solo lo compartido real (3 helpers usados por ≥2 dominios). Sin
imports circulares (`web` no usa `web_datos` en tipos, solo el router).

---

## 4. `stream.rs` — pureza del parseo SSE (F2/F3 079A-1)

**Evidencia:** `core/src/nucleo/llm/stream.rs` (138) — `hojear_stream`
(`:9`, `pub(crate)`, I/O + acumulación), `extraer_evento_sse` (`:93`, pura),
`fusionar_tool_call` (`:107`, mutación local del acumulado).

**Veredicto:** `[ya corregido]` (F2/F3). Las dos funciones puras no tocan
red ni estado global; `hojear_stream` es el único sitio que acopla
bytes→eventos→acumulado, y su firma lo declara (tupla cruda). OCP: un
proveedor con otro framing añadiría otro extractor, sin tocar el bucle.

---

## 5. Navegador core — `navegador/` (F4 079A-1)

**Evidencia:** `core/src/herramientas/navegador/` — `mod.rs` (20: fachada +
re-export `ToolNavegadorReflejo`), `reflejo.rs` (235: la tool),
`operaciones.rs` (122: `arg_str`/`op_script`/`op_dom`/`op_capturar` de F2),
`pruebas.rs` (206, `#![cfg(test)]`). La tool solo existe si el consumidor
aporta `NavegadorPort` (fail-closed, `mod.rs:6-7`).

**Veredicto:** `[ya corregido]` (F4). SRP por pieza (contrato tool /
operaciones / tests); DIP intacto (puerto en el consumidor, §3 de S5 sigue
valiendo).

---

## 6. Desktop — splits F5/F5b

**Evidencia:** `desktop/src-tauri/src/` — `main.rs` (497: tipos
`Estado`/`Sesion`/paneles + `abrir_sesion_interna` + `main()`),
`turno.rs` (314: `PaqueteTurno` + 8 auxiliares F5),
`sesion.rs` (167: config/proveedores),
`conversaciones.rs` (426: CRUD), `workspaces.rs` (241),
`pruebas.rs` (55, `#![cfg(test)]`), `navegador/` (estado+comandos+webview2+
puerto, F5). Superficie cruzada mínima y explícita: `pub(super)` solo en
`conv_id_de_panel*`, `conteos`, `ProveedorConteo{,Info}`,
`CargaConversacion`, `RestauracionTramo`, `ValorWorkspaces`.

**Decisiones registradas:**

- `PaqueteTurno` + `OpcionesApertura` (F5): agrupan params de `enviar_turno`
  y `abrir_sesion_interna` (9 params → struct). `[ya corregido]`.
- `leer_max_ventana` vive en `pruebas.rs` (F5b): 0 llamadas en producción
  (verificado por grep en `desktop/src-tauri/src/*.rs`). Es lógica P6
  huérfana de cableado, no código muerto introducido por el split: el split
  solo la movió con sus tests. `[decisión — no corregir aquí]`: cablearla
  en `preparar_paquete` sería cambio de comportamiento fuera del alcance de
  F5b; queda agendada como tarea (el default 150k viaja hoy por otra vía o
  no viaja — verificar en el cierre funcional Tauri).
- Comandos Tauri en submódulos exigen ruta completa
  (`turno::enviar_turno`) + `pub(crate) mod` (el macro genera envoltorios
  `pub` que no atraviesan re-exports). `[ya corregido]` (F5b).

**Veredicto global:** `[ya corregido]` (F5/F5b). `main.rs` bajo el límite
(497 < 500, 0 warnings propios); cada módulo = un dominio de comandos.

---

## 7. `rustTestScope.ts` (glory-sentinel `1587c59`) — DRY del gate

**Evidencia:** helper `esArchivoSoloTest` extraído a
`src/analyzers/rustTestScope.ts` y reutilizado en `rustAnalyzer.ts`
(unwrap/panic) y `rustReglasNuevas.ts` (expect/block/lock); budget ADR 0001
648/650.

**Veredicto:** `[decisión — correcto]`. Un solo criterio de "fichero
solo-test" para las 5 reglas; el budget obligó a compartir en vez de
duplicar (la alternativa era +19 líneas en `rustAnalyzer.ts`).

---

## Resumen de clasificaciones

| Pieza | Veredicto |
|---|---|
| 1. `SesionComun` | Ya corregido (069A-2 v3) |
| 2. `web.rs` SSE | Decisión — no corregir (F1 agendada, bloqueada por ajeno) |
| 3. `web_datos/` | Ya corregido (F3) |
| 4. `stream.rs` | Ya corregido (F2/F3) |
| 5. Navegador core | Ya corregido (F4) |
| 6. Desktop F5/F5b | Ya corregido; 1 decisión (`leer_max_ventana` huérfana, agendar) |
| 7. `rustTestScope.ts` | Decisión — correcto |

**Deuda nueva agendada (no bloqueante):** cablear o eliminar
`leer_max_ventana` (lógica P6 sin llamadas prod). **Sin cambios de
comportamiento en esta fase:** F7 es solo lectura + informe.
