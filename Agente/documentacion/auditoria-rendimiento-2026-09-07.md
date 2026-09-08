# Auditoría de rendimiento — código nuevo 069A-2 + 079A-1 (F8)

Plan: `Agente/planes/plan-deuda-cero-079A-1-2026-09-07.md` (F8).
Método: el de S6 (05-09): checklist ítem por ítem, se corrige **solo lo
demostrado con medición**; focos del plan: fan-out SSE post-F1, clones de
contexto por iteración, SQLite WAL bajo 2 procesos, bundle UI. Cero cambios
de código en este paso.

## 1. Fan-out SSE (`broadcast`, F1 pendiente) — *hallazgo real ya agendado,
sin medir contención*

Ruta (`cli/src/comandos/web.rs`): cada sesión crea
`broadcast::channel(256)` (`:316`); `eventos_sse` (`:395`) suscribe un
`BroadcastStream` por cliente SSE + 1 evento `ready` snapshot (`:405-421`).
El `send` recorre suscriptores bajo el Mutex interno del broadcast
(regla `broadcast-mutex-riesgo-rs` ×2, únicos errores del gate).

Análisis: con el uso real (1–2 clientes SSE por sesión, eventos O(rondas +
tools) por el coalescido de S6-ítem 4, payloads JSON compactos pequeños), el
`send` es O(suscriptores) con N≈1 y sin contención medible: el coste por
evento es un `clone()` de `String` por suscriptor + wake. La regla cita el
incidente 096A (contención bajo muchos suscriptores), escenario que hoy no
existe (sin fan-out multi-cliente real). El buffer 256 + `lagged` (`:424`)
acota el peor caso ante un lector lento.

**Veredicto:** no se optimiza aquí (F1 ya agenda el cambio a mpsc por
suscriptor por corrección de diseño, no por medición; medir contención con
N=1 sería teatro). Invariante: si aparecen ≥10 suscriptores/sesión o
payloads >100 KB/evento, medir `send` p50/p99 antes y después de F1.

## 2. Clones de contexto por iteración — *sin cambios (veredicto S6 vigente)*

Los splits F2–F5b no añadieron clones en la ruta caliente: `PaqueteTurno`
(`desktop/src-tauri/src/turno.rs`) agrupa `historial_previo: Vec<AiMessage>`
por movimiento (se construye una vez por turno en `preparar_paquete`); los
re-exports (`web_datos/mod.rs`, `navegador/mod.rs`) son alias de módulo,
cero coste; `sesion_y_comun` (`web_datos/mod.rs:86`) clona `SesionComun`
una vez por request HTTP (no por iteración ni por token).

**Veredicto:** no problema; invariante S6 intacto (2 clones/llamada LLM,
dominado por red).

## 3. SQLite WAL bajo 2 procesos (CLI web + desktop) — *recomendación S6
vigente, sin medir de nuevo*

Sin cambios en `persistencia_sqlite.rs` en este bloque (ajeno 039A-3; el
único añadido cercano es un `mod tests` ajeno en `cli/src/servicio/
sesion.rs`, sin I/O nuevo). El escenario 2-procesos (servidor web + Tauri
sobre la misma BD) sigue con WAL + 1 commit/op + rusqlite síncrono bajo
Mutex (S6-ítem 3). No se observó corrupción ni bloqueo en las pruebas E2E
del período (069A-2 E2E puerto 18099, 069A-4 E2E ventana real).

**Veredicto:** se mantiene la recomendación a 039A-3 (`spawn_blocking` +
transacción por turno); no se mide aquí (requeriría carga concurrente real
de 2 procesos, fuera del alcance de F8).

## 4. Bundle UI / artefactos nuevos — *sin regresión*

Los splits solo añaden módulos Rust (monomorfización idéntica, sin nuevas
dependencias: `Cargo.lock` sin cambios en este bloque). Sin cambios en
`desktop/ui` propios (los `M` en `ui/` son ajenos). Tiempos de `cargo check
-p glory-harness-desktop` del período: ~5 s incremental (sin regresión de
compilación observable).

**Veredicto:** no problema.

## 5. `hojear_stream` (nuevo `stream.rs`) — *no problema*

El bucle procesa línea a línea con `String::from_utf8_lossy` por chunk y
`push_str` incremental (`stream.rs:22-80`): 1 copia del contenido total
(el acumulado, inevitable) + `serde_json::from_str` por línea `data:`.
`fusionar_tool_call` concatena fragmentos de argumentos con 1 `format!`
por fragmento (O(n²) teórico en el largo de arguments; arguments típicos
<10 KB → irrelevante; el tope real lo pone el proveedor, no el parseo).
Sin asignaciones por token fuera del acumulado.

**Veredicto:** no problema; invariante: si un proveedor emitiera
`arguments` >1 MB troceado en miles de deltas, pre-reservar con
`String::with_capacity`.

## 6. Objetivos fijados

- **Gate:** full 2E/0W (solo F1 ajena pendiente); `check desktop` 0
  warnings propios; suites Rust 257 + 81 verdes (losdesktop `--lib` no
  enlaza por disco lleno — pendiente de infraestructura, no de código).
- **Streaming SSE web:** O(eventos de ronda), 1 `clone` de cable por
  suscriptor; keep-alive 15 s (`web.rs:431-435`).
- **Compilación:** sin nuevas deps; check incremental desktop ~5 s.

---

## Resumen

| Ítem | Veredicto | Evidencia |
|---|---|---|
| 1 Fan-out SSE | Real pero ya agendado (F1); sin contención medible con N≈1 | `web.rs:87,316,395` |
| 2 Clones contexto | No problema (invariante S6 intacto) | `PaqueteTurno`, `sesion_y_comun` |
| 3 SQLite 2 procesos | Recomendación S6 vigente (ajeno) | sin cambios en el bloque |
| 4 Bundle/compilación | No problema | sin nuevas deps, check ~5 s |
| 5 `hojear_stream` | No problema | `stream.rs:22-80,105-138` |
| 6 Objetivos | Fijados con números | gate 2E/0W, 257+81 tests |

**Cero correcciones aplicadas** (correcto según el método: nada demostrado
con medición salvo lo ya agendado en F1).
