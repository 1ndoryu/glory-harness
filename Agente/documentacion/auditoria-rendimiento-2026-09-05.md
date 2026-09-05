# Auditoría de rendimiento — glory-harness (S6, 2026-09-05)

Plan: `Agente/planes/plan-saneamiento-calidad-2026-09-05.md` (S6).
Método: instrumentación corta, micro-benchmark puntual en la TUI y revisión de
los puntos calientes candidatos del checklist. Se corrige **solo lo demostrado**;
lo que resulta "no problema" queda documentado con evidencia para evitar
regresiones y optimización prematura.

Checklist S6, ítem por ítem:

## 1. Copia de contexto por turno — *no problema (acotado, dominado por red)*

Ruta por llamada LLM (`core/src/nucleo/runtime/`):

1. `turno/mod.rs` `un_paso_de_iteracion` hace `mem::take(&mut estado.mensajes)`
   (mueve, **no copia**) y llama `preparar_contexto_iteracion`.
2. `preparar_con` (`context.rs:216`) en la ruta sin compactar devuelve
   `mensajes.to_vec()` → **1 clon completo** de `Vec<AiMessage>` por iteración.
3. `llm_llamada` (`runtime/tools.rs:7`) hace `mensajes.to_vec()` +
   `schemas.to_vec()` para `enviar_chat_stream` → **1 clon más** + schemas.

Total: **2 clones completos de `Vec<AiMessage>` + 1 de schemas por llamada LLM**,
antes de una llamada de red que tarda órdenes de 10⁶× más (100 ms–60 s frente a
~10–100 µs por clon de un contexto típico de decenas de miles de tokens). No hay
copias en el bucle de tools ni N+1 de clonado por evento.

**Veredicto:** no se optimiza (sería prematuro y añadiría índices/deltas frágiles);
se documenta el invariante de no añadir clones dentro del bucle de ejecución de
tools ni por evento SSE.

## 2. `registry.ids()` por iteración — *no problema*

`un_paso_de_iteracion` llama `self.registry.ids()` **una vez por llamada LLM**
(no por tool ejecutada ni por evento). Coste O(n_tools) con ~20 tools registradas:
despreciable frente a la serialización de schemas (que sí ocurre una vez por
llamada, no por bucle interno). Caché por turno innecesaria: el set de tools no
cambia dentro de un turno y la frecuencia es la de las rondas LLM, no la de los
tokens.

**Veredicto:** no problema; si algún día el turno emite decenas de rondas por
segundo (hoy imposible: cada ronda es una llamada de red), cachear por turno.

## 3. Persistencia SQLite — *hallazgo real, archivo ajeno (039A-3)*

`cli/src/persistencia_sqlite.rs` (coordinado con 039A-3 — solo lectura aquí):

- **Carga de historial:** `listar_mensajes` es un único `SELECT … WHERE
  conversacion_id ORDER BY creado_en` → **sin N+1**. ✅
- **Escrituras:** `guardar_turno`, `guardar_mensaje` y `registrar_accion` hacen
  cada uno su propio `INSERT` en autocommit → **1 commit por operación**. Un turno
  típico = 1 turno + N mensajes + M acciones ≈ **12–15 commits**, cada uno con su
  fsync (WAL activo suaviza pero no elimina el coste).
- **Bloqueo del runtime:** `bloquear(&self.conn)` toma un `std::sync::Mutex` y
  ejecuta rusqlite **síncrono dentro del runtime async** (el único `spawn_blocking`
  del archivo es un comentario, no código). Cada escritura bloquea el hilo del
  executor durante I/O de disco.

**Veredicto:** hallazgo real pero en archivo ajeno coordinado con 039A-3. Mejora
propuesta (no aplicada aquí): envolver en `spawn_blocking` y agrupar las
escrituras del turno en **una sola transacción** (`BEGIN` … `COMMIT`). Queda como
recomendación para 039A-3.

## 4. Streaming SSE — *no problema (ya coalescido)*

`red.rs` `hojear_stream` consume el SSE del proveedor **delta a delta** y llama
`on_token` por delta, pero el runtime (`turno/mod.rs`) **solo acumula** en
`ultimo_contenido` y emite `AgenteEvento::Token` **una vez por ronda LLM** con el
contenido completo (más 1 `Usage` y 1 `ContextoDetalle` por ronda).

Volumen real por turno: **O(rondas LLM + tools ejecutadas)**, no O(tokens). No
existe inundación de eventos por token; la serialización por evento es mínima.
Nota de UX (fuera del alcance de rendimiento): el consumidor ve el texto por
rondas, no por token; si se quisiera streaming fino real, el emisor por delta
tendría que vivir en el runtime — decisión de producto, no de rendimiento.

**Veredicto:** no problema; el objetivo "sin pausas > 200 ms por token" no aplica
porque no se emiten eventos por token (coalescido por ronda). Si se añade
streaming fino, reintroducir el objetivo con fixture SSE local.

## 5. Locks en runtime — *no problema*

- `self.contexto.lock().await` (`turno/mod.rs:298`) se retiene **solo durante
  `preparar_con`** (bloque corto, sin red) y se libera antes de `tx.send` y de la
  llamada LLM. No hay lock retenido a través de red.
- `guardas.lock()`, `plan_actual.lock()`, `telemetria.lock()`: `std::sync::Mutex`
  breves, sin `.await` bajo el lock en la ruta caliente.
- El único bloqueo real del runtime async es el Mutex + rusqlite síncrono de la
  persistencia (ítem 3, ajeno).

**Veredicto:** no problema en el núcleo; la contención real está en SQLite (039A-3).

## 6. Subagente/task — *no problema (límites verificados)*

`runtime/subagente.rs` tiene y ejecuta límites reales:

- `SUBAGENTES_EN_CURSO` + `concurrencia_permitida` + `GuardiaConcurrencia`
  (máximo de sesiones hijas simultáneas);
- `profundidad_subagente: AtomicU8` + `GuardiaProfundidad` con **profundidad ≤ 1**
  y fail-closed (contador atómico).

El spawn de un hijo es un runtime ligero en memoria dentro del mismo executor
tokio (sin hilo OS ni proceso): coste de delegación acotado a la construcción de
un `AgentRuntime` + contexto. El padre es secuencial, así que no hay ráfaga de
hijos en paralelo.

**Veredicto:** no problema medible; límites ya presentes y verificados por tests.

## 7. TUI — *corregido (medido antes/después)* ✅

Hotspot demostrado: `a_lineas` re-envolvía **todo el historial** en cada frame
(~20 fps) aunque solo creciera el último bloque en streaming.

- **Antes (release, 80 turnos → 1440 filas, streaming):** **4.15 ms/frame**,
  creciendo lineal con el historial.
- **Después:** **60 µs/frame en streaming** (~70×) y **22 µs/frame idle**,
  ambos **independientes del largo del historial**: caché de filas de bloques
  cerrados + re-envuelto solo de la cola abierta + clonado acotado a la ventana
  visible (alto de pantalla) en `pintar_mensajes`.

Archivos: `cli/src/ui/tui/mod.rs` (caché + `filas_visibles`) y
`cli/src/ui/tui/render.rs` (`pintar_mensajes` con ventana visible).
Test de regresión determinista añadido (35 tests CLI, invariantes: total coherente
con `a_lineas`, visibles ≤ alto, streaming sin re-envuelto completo).

## 8. Objetivos fijados

- **Turno sintético sin red:** suite completa de turnos deterministas (fixtures
  con tools, subagentes, permisos, compactación) en **~3.8 s de ejecución en
  debug** (195 core + 35 CLI + 4 desktop = 234 tests; wall total del workspace
  ~19.5 s incluyendo compilación). Sin regresión de tiempo observada tras S3/S4.
- **Streaming:** acotado por el fix TUI (60 µs/frame, independiente del
  historial) — el objetivo de pausas por token no aplica (ítem 4).
- **RAM:** sin medición de WS del release en esta pasada; los cambios de S6 no
  añaden estado por mensaje (la caché vive en la struct de la TUI y se descarta
  al recrear el frame). Pendiente de medir en el próximo cierre si se desea
  número firme.

---

## Resumen

| Ítem | Veredicto | Evidencia |
|---|---|---|
| 1 Copia de contexto | No problema (2 clones/llamada, dominado por red) | `preparar_con` + `llm_llamada` |
| 2 `registry.ids()` | No problema (1 vez por ronda LLM) | `turno/mod.rs:210` |
| 3 SQLite | Hallazgo real (ajeno 039A-3): 1 commit/op + síncrono en async | `persistencia_sqlite.rs` |
| 4 Streaming SSE | No problema (coalescido a 1 evento/ronda) | `hojear_stream` + `turno/mod.rs` |
| 5 Locks | No problema (sin lock bajo red en el núcleo) | `turno/mod.rs:298` |
| 6 Subagente | No problema (límites de concurrencia/profundidad activos) | `subagente.rs` |
| 7 TUI | **Corregido: 4151 µs → 60 µs/frame** (~70×) | `ui/tui/{mod,render}.rs` + test regresión |
| 8 Objetivos | Fijados con números | suite 234 tests / ~3.8 s ejecución debug |

Única corrección aplicada: ítem 7 (TUI). El hallazgo SQLite (ítem 3) queda
documentado como recomendación coordinada con 039A-3 (archivo ajeno, sin tocar).
