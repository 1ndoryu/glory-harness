# Auditoría SOLID/arquitectura — glory-harness core + cli

- **Fecha:** 05-09-2026 · **Plan:** `plan-saneamiento-calidad-2026-09-05.md`, fase S5.
- **Base:** árbol post-S2/S3/S4 (commits `f33a4de`, `3910477`, `f3d467b`, `79022c7`,
  `8064c41`). La reorganización S4 movió el código a `core/src/{nucleo,herramientas,
  politica,contrato}/` y `cli/src/{comandos,ui,infra}/` **sin cambiar rutas de módulos**
  (re-export plano en `lib.rs`), así que toda ruta citada aquí es la **nueva**.
- **Método:** revisión por pieza con evidencia `ruta:línea`. Cada hallazgo se clasifica
  `[YA corregido en S#]` / `[corregir aquí]` / `[decisión — no corregir, con razón]`.
  Criterio de éxito S5: informe con hallazgos y **cero cambios de comportamiento** fuera
  de S2–S4 (este paso no toca código).

---

## 1. `AgentRuntime` — SRP (¿dios-módulo tras S2?)

**Evidencia de estructura actual** (post-S2/S3/S4):

| Responsabilidad | Dónde vive ahora |
|---|---|
| Orquestar turno (bucle tools + fin) | `core/src/nucleo/runtime/turno/mod.rs` (orquestador 410 ln) |
| Verdicto de permisos / ejecutar tool aprobada | `core/src/nucleo/runtime/turno/permisos.rs` |
| Persistir turno / telemetría / auditoría | `core/src/nucleo/runtime/turno/auditoria.rs` |
| Sesiones hijas (subagente, profundidad ≤1) | `core/src/nucleo/runtime/subagente.rs` |
| Despacho genérico `ejecutar_tool` | `core/src/nucleo/runtime/tools.rs:46` |
| Estado/coordinación del struct | `core/src/nucleo/runtime/mod.rs:225` |

**El struct** (`runtime/mod.rs:225-257`) ya no contiene lógica de turno: solo **puertos**
(`persistencia`, `llm`, `web_search`, `web_fetch`, `dominio`), el `registry` de tools y
estado transversal interior-mutable (telemetría, reglas, guardas, plan_actual,
contadores atómicos). Nada de eso es un "dios" de comportamiento: es el **estado de
coordinación** que el orquestador consulta.

**Antes/después:** `runtime.rs` tenía 1482 líneas efectivas (4 avisos de Sentinel,
`ejecutar_turno` 380 + `ejecutar_subagente` 200). Tras S2+S3+S4 el directorio
`runtime/` suma 0 hallazgos de Sentinel y ningún archivo supera los límites.

**Veredicto:** `[YA corregido en S2–S4]`. SRP satisfecho: orquestación ≠ permisos ≠
auditoría ≠ subagentes; el struct solo coordina y delega.

---

## 2. `LlmProviderService` — SRP/OCP (rotación + nutrición + stream + parseo)

**Evidencia:** el split S2.2 (`3910477`) dividió `llm.rs` (1053 ef, 3 avisos) en un
módulo `llm/` de tres piezas:

- `llm/mod.rs` (552): fachada `LlmProviderService` (estado: `client: reqwest::Client`
  `mod.rs:45`; `keys_para` privado `mod.rs:333`), circuito y `enviar_chat*`.
- `llm/modelo.rs` (565): **catálogo de datos** puro — `PROVIDERS` (`modelo.rs:7`),
  `CHAT_FALLBACK_CHAIN` (`modelo.rs:86`), `url_proveedor` (`modelo.rs:121`),
  `modelo_proveedor` (`modelo.rs:145`), `resolver_candidatos` (`modelo.rs:436`).
- `llm/red.rs` (558): HTTP/stream/reintentos (`cliente: &reqwest::Client` `red.rs:77`)
  y parseo puro.

**OCP:** añadir un proveedor nuevo = **añadir una fila a `PROVIDERS`/cadena de fallback**
(datos), sin tocar la lógica de rotación/stream. El servicio está cerrado a la
modificación para datos nuevos. Solo una **clase nueva de protocolo/auth** (no un
proveedor del mismo protocolo) tocaría `red.rs` — eso es comportamiento nuevo legítimo,
no un cambio OCP.

**Veredicto:** `[YA corregido en S2]`. SRP por pieza (datos / red / fachada) y OCP
data-driven para el caso real (añadir proveedor del mismo protocolo).

---

## 3. `AgentToolRegistry` / trait `AgentTool` — ISP/OCP y acoplamiento del núcleo

**Evidencia:**

- Contrato mínimo: `AgentToolContext` (`tool.rs:29`), `AgentToolResult` (`tool.rs:61`),
  trait `AgentTool` (`tool.rs:112`) con `id` + `schema` + `ejecutar` — sin más.
- **El núcleo no interpreta contenido de dominio**: comentario de contrato en
  `tool.rs:9` ("el consumidor downcastea; el núcleo nunca lo interpreta (DIP)") y
  `tool.rs:47`. La única referencia a `downcast` en el núcleo es **documental**
  (`runtime/mod.rs:212`): el slot `dominio: Option<Arc<dyn Any + Send + Sync>>`
  (`mod.rs:233`) solo lo downcastea el **consumidor**.
- El runtime despacha vía trait: `ejecutar_tool` (`runtime/tools.rs:46`) y
  `ejecutar_tool_aprobada` (`runtime/turno/permisos.rs:191`) — no hay `match` por id de
  tool con lógica acoplada dentro del núcleo.
- **OCP del registro:** `registrar`/`registrar_mcp`/`registrar_sandbox`/`registrar_todo`
  (`tool.rs:205-253`) permiten añadir tools sin tocar el runtime (así se añadieron las
  tools de dominio de task y `web_fetch`).

**Veredicto:** `[YA corregido]` (F5 318A-15 + F3 318A-16 + B3). ISP: el trait es el
mínimo que el runtime necesita; OCP: registro abierto; DIP: núcleo contra contrato, el
downcast de dominio vive en el consumidor.

---

## 4. `tool.rs` (1144 líneas) — SRP del archivo

**Evidencia:** tras las fases previas, `tool.rs` ya no contiene tools concretas: el
directorio `herramientas/` separa `tools_archivo.rs`, `tools_web.rs`, `comando.rs`,
`scheduler.rs`, `tareas.rs`, `todo.rs`, `skill.rs`, `mcp.rs`. Lo que queda en `tool.rs`
es el **contrato + registro + política de permisos del registro** (trait, context,
result, `registrar*`, `ids`, `schemas_openai`, `permiso_para_llamada`,
`clasificar_llamada`, peticiones/preguntas) — cohesivo: es "el registro y su política".

**Sentinel:** `tool.rs` **no reporta** hoy (0 hallazgos core+cli en el analyze
post-S4); el archivo cargó históricamente ~900+ líneas efectivas bajo el umbral de
`limite-lineas` porque la mitad son doc-comentarios de contrato.

**Veredicto:** `[decisión — no corregir ahora]`. Un split adicional de `tool.rs` sería
movimiento mecánico (contrato vs registro) sin ganancia de gate ni de comportamiento, y
con riesgo de tocar el punto más referenciado del núcleo. Queda **agendado como mejora
opcional** si el archivo crece o reporta. La separación física real (tools concretas
fuera) ya está hecha.

---

## 5. `tui.rs`/`UiEstado` — UI-estado vs eventos vs render

**Evidencia (post-S2.3/S3/S4, `cli/src/ui/tui/`):**

- `UiEstado` (`tui/mod.rs:127`) solo tiene estado de **presentación**: `mensajes: Vec<
  Bloque>` (contenido ya renderizable), `entrada`, `cursor` (índice de caracteres,
  `mod.rs:131-134`), `estado`, `ocupado`, `salir`, `siguiendo_final`, `scroll_manual`.
  **No** contiene canal SSE, ni handles de persistencia, ni tipos del runtime.
- La lógica de eventos vive en `bucle.rs` (`spawn_worker` y helpers extraídos en S3);
  el render en `render.rs` (helpers extraídos en S3); el ajuste de texto en `texto.rs`.

**Veredicto:** `[YA corregido en S2/S3]`. `UiEstado` no conoce SSE ni persistencia; los
eventos SSE entran por el canal y se traducen a `Bloque`s fuera del estado.

---

## 6. Puertos (`ports.rs`) — ISP: ¿struct grande con `Option` o traits por rol?

**Evidencia:** `ports.rs` define **siete traits de rol** (no un dios):

| Trait | Línea | Rol |
|---|---|---|
| `AgentPersistence` | `ports.rs:138` | turnos/mensajes/memoria/acciones |
| `ProgramadorTareas` | `ports.rs:191` | tareas programadas |
| `WebSearchProvider` | `ports.rs:222` | búsqueda web |
| `WebFetchProvider` | `ports.rs:241` | fetch de URL |
| `McpProveedor` | `ports.rs:266` | MCP |
| `EjecutorComando` | `ports.rs:299` | shell acotado |
| `ProviderPort` | `ports.rs:345` | proveedor LLM |

El runtime los compone como campos `Arc<dyn Rol>` independientes, con `Option` solo
para los **opcionales reales** (`web_search`/`web_fetch`/`dominio` en
`runtime/mod.rs:231-233`, fail-closed si ausentes) — no como un struct de 20 campos con
la mitad a `None`.

**Veredicto:** `[verificado — ya granular]`. Dividir más (p. ej. partir
`AgentPersistence` por agregado) rompería las implementaciones concretas de PROYECTO
TASKS (repositorios reales) sin un segundo consumidor que lo justifique — se documenta
como límite deliberado (regla "abstracciones sin segundo caso real").

---

## 7. DIP en llm — ¿reqwest concreto o trait inyectable?

**Evidencia:** `LlmProviderService` construye y retiene `reqwest::Client` concreto
(`llm/mod.rs:45`, `llm/red.rs:77`). El runtime lo recibe como objeto inyectado
(`runtime/mod.rs:230`, `Arc<LlmProviderService>`); el núcleo **no crea** el cliente en
el turno y el servicio es agnóstico de proveedor (catálogo de datos, §2).

**Límite de testabilidad documentado:** los tests del núcleo **no tocan red**: el parseo
y la lógica pura (candidatos, reintentos transitorios, `parsear_tool_calls`) se prueban
sin HTTP; los E2E deterministas usan fixture/stub offline en la capa de turno. Un test
que ejercitara reintentos HTTP reales exigiría abstraer `reqwest` tras un trait
`HttpClient` — **no justificado** hoy (un solo consumidor real, y el servicio ya es
inyectable como objeto para tests de integración del consumidor).

**Veredicto:** `[decisión — no corregir ahora]`. DIP parcial correcto para el caso real:
el servicio concreto es intercambiable en el runtime; la abstracción adicional de HTTP
se agrega solo si aparece un segundo consumidor o un test de reintentos justificado.

---

## 8. Errores — tipos vs `Error::Validacion(String)` y `unwrap`/locks

**Evidencia:**

- `Error` tiene **11 variantes tipadas** (`contrato/error.rs:8-30`): `Argumentos`,
  `ToolDesconocida`, `Proveedor { detalle, causa }`, `Validacion`, `Persistencia`,
  `Sandbox`, `NoEncontrado`, `Limite`, `Cancelado`, `Interno`. El contexto no se pierde
  en un `String` global; `Proveedor` separa el detalle presentable de la causa interna
  que **no** se expone al LLM (`error.rs:13-15`).
- **`unwrap`/`expect` en producción:** las reglas del gate `unwrap-produccion-rs` y
  `expect-produccion-rs` (implementadas en S1/S7, glory-sentinel `902c45e`) reportan
  **0 hits** en core+cli (los 3 sitios reales se corrigieron en S7; los 2 restantes son
  `desktop/src-tauri/src/vault.rs`, ajeno 039A-3).
- **Locks envenenados:** patrón generalizado `unwrap_or_else(|p| p.into_inner())` —
  `contrato/sandbox.rs:307`, `herramientas/tool.rs:314,321,332` — recuperación segura,
  no panic.

**Veredicto:** `[YA corregido en S7 / verificado]`. Tipos de error ricos y sin
`unwrap` de producción bajo las reglas del gate.

---

## 9. Persistencia — puerto `AgentPersistence`, sin SQL en el CLI ni fuga al núcleo

**Evidencia:**

- `cli/src/infra/persistencia.rs` (cabecera, ln 1-10): implementación **en memoria** de
  `AgentPersistence` — el binario standalone "no tiene base de datos propia… permite
  usar el runtime sin acoplar el binario a SQLx ni a las tablas de task (R5 del plan
  318A-13: el núcleo no sabe qué persistencia usa el consumidor)".
- **El único SQL del CLI vive en `cli/src/persistencia_sqlite.rs`** (grep de
  `sqlx::query`/DML sobre `cli/src`: un solo archivo, **ajeno 039A-3**, no movido en S4
  y fuera del alcance de este saneamiento). El núcleo (`core/`) no contiene SQL.
- El núcleo depende del **puerto** (`AgentPersistence`, `ProgramadorTareas`); los tipos
  de contrato (`TurnoPersistido`, `MensajePersistido`, `AccionAuditable`…) los define
  `ports.rs`, no el adaptador.

**Veredicto:** `[verificado]`. La frontera DIP es correcta: núcleo contra contrato;
consumidor elige adaptador (memoria para el binario, repositorios SQLx para task);
ningún SQL del CLI se filtra al núcleo.

---

## Resumen de clasificaciones

| Pieza | Verdicto |
|---|---|
| 1. `AgentRuntime` SRP | YA corregido en S2–S4 |
| 2. `LlmProviderService` SRP/OCP | YA corregido en S2 (split datos/red/fachada) |
| 3. Registry/trait `AgentTool` ISP/OCP/DIP | YA corregido (F5/F3/B3) — verificado |
| 4. `tool.rs` 1144 ln | Decisión — no corregir (cohesivo; split opcional futuro) |
| 5. `UiEstado` UI vs eventos vs render | YA corregido en S2/S3 |
| 6. Puertos ISP | Verificado — traits por rol ya granulares |
| 7. DIP llm (reqwest) | Decisión — no corregir (límite documentado) |
| 8. Errores / unwrap / locks | YA corregido en S7 / verificado |
| 9. Persistencia (puerto, sin SQL propio) | Verificado |

**Deuda opcional agendada (no bloqueante):** split de `tool.rs` si crece o reporta
(§4). **Fuera de alcance:** `desktop/` y `persistencia_sqlite.rs` (ajenos 039A-3).

Sin cambios de comportamiento en esta fase: S5 es solo lectura + informe.
