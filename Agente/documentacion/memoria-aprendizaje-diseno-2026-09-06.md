# Diseño: memoria de aprendizaje (Bloque 3, Fase 8b — solo diseño)

- **Fecha:** 06-09-2026 · **Estado:** diseño, sin implementar.
- **Evidencia:** hermes-agent `agent/memory_manager.py` (`prefetch_all`,
  `sync_all`, proveedores de memoria, `sanitize_context`), `agent/curator.py`
  (curación periódica: intervalo, idle, stale, archive, prune, consolidate).
- **Relación con F8a:** el ejecutor del cron (`core/src/nucleo/cron.rs`,
  `MotorTurno::ejecutar`) es el punto de inserción del `prefetch`/`sync`;
  el curador cabe como una tarea recurrente más del `schedule`.

## 1. Qué replica de hermes (y qué no)

| Hermes | Decisión para GH |
|---|---|
| `MemoryManager` con proveedores (`MemoryProvider`) + `prefetch_all(query)` → bloque de contexto | **[REPLICAR]** como puerto del núcleo (misma forma que `WebSearchProvider`): el core no sabe dónde vive la memoria. |
| `sync_all(...)` tras el turno (extraer y guardar) | **[REPLICAR]** como paso explícito post-turno, con lo extraído visible en auditoría (nunca escritura silenciosa). |
| Tools de memoria expuestas al agente (`inject_memory_provider_tools`) | **[EVALUAR]** después: primero prefetch/sync automáticos; las tools `memoria_*` ya existen y bastan para v1. |
| `sanitize_context` / `StreamingContextScrubber` | **[REPLICAR]** mínimo: la memoria nunca guarda secretos (regla: si parece credencial, no se persiste; lista de patrones en el núcleo). |
| `curator.py` (poda, archivo, consolidación, estado en fichero) | **[REPLICAR]** como tarea recurrente del propio cron + políticas configurables; el estado vive en la tienda del consumidor (tabla o `config`), no en fichero suelto. |
| Revisión con LLM de cada recuerdo (`review_engine`) | **[NO]** en v1: la extracción la propone el turno y el curador aplica reglas deterministas (edad, uso, duplicados). Re-scoring con LLM queda como fase posterior. |

## 2. Superficie propuesta (núcleo)

```rust
/// Proveedor de memoria de aprendizaje (puerto, como `WebSearchProvider`).
#[async_trait]
pub trait ProveedorMemoria: Send + Sync {
    /// Recupera contexto relevante para `query` (devolución acotada en
    /// caracteres; el núcleo lo anexa al system prompt tras `[REGLAS]`).
    async fn prefetch(&self, user_id: Uuid, query: &str, limite: usize) -> Result<String>;
    /// Extrae y guarda lo aprendido del turno (devuelve lo guardado para
    /// auditoría; el consumidor persiste vía `memoria_upsert`).
    async fn sync(&self, user_id: Uuid, resumen_turno: &str) -> Result<Vec<MemoriaEntrada>>;
}
```

- `AgentRuntime` recibe `memoria: Option<Arc<dyn ProveedorMemoria>>` en
  `PuertosHarness` (`None` = sin memoria, fail-closed: el turno funciona igual).
- `MotorTurno::ejecutar` (F8a) llamaría a `prefetch` antes del turno y a
  `sync` tras `tarea_finalizar`: el cron aprende de sus propias ejecuciones.
- Hueco detectado: el puerto de skills es de solo lectura (`skills_listar`);
  promover un recuerdo a skill necesita `skills_registrar` (o el curador
  escribe en la tienda del consumidor por fuera del puerto). Decidir en la
  implementación.

## 3. Curador (tarea recurrente)

- Se programa con el propio `schedule create --cuando "diario a las 4"`.
- Cada pasada: recupera entradas (`memoria_listar`), aplica políticas
  deterministas — `stale_after_days` (marcar obsoleta), `archive_after_days`
  (archivar), `prune` (borrar duplicadas/contradichas con `memoria_borrar`),
  `consolidate` (fusionar misma clave vía `memoria_upsert`) — y entrega su
  resumen en `tarea_logs` como cualquier ejecución del cron.
- Parámetros con defaults hermes-compatibles: intervalo 24h, idle mínimo 1h,
  stale 30d, archive 90d. Sin LLM en v1 (ver §1).

## 4. Reglas de seguridad y privacidad

1. La memoria es por `user_id` (como `memoria_*` hoy): el cron solo prefiere
   la del dueño de la tarea.
2. Sanitizado antes de persistir (patrones de secretos; lista en el núcleo,
   ampliable por consumidor). Un falso positivo se pierde; una credencial
   filtrada no se recupera: sesgo a no guardar.
3. Todo lo guardado por `sync`/curador queda con fecha y origen (qué turno o
   pasada lo produjo) para auditar y revertir (`memoria_borrar`).

## 5. Criterios de aceptación (para la fase de implementación)

1. `prefetch` acotado (límite de caracteres configurable) y ausente sin
   proveedor (el turno no cambia).
2. `sync` devuelve lo guardado; `schedule logs` de un turno muestra qué
   recuerdos produjo.
3. El curador como tarea recurrente deja su resumen en `tarea_logs` y no toca
   entradas usadas en los últimos N días.
4. Ningún secreto de los fixtures de test termina persistido (test con
   patrones tipo `sk-...`, `Bearer ...`).
5. Gate `sentinel check` del ID que la implemente en verde (0 errores).

## 6. Estimación y orden

1. Puerto `ProveedorMemoria` + cableado en `PuertosHarness`/`MotorTurno` (pequeño).
2. Implementación base sobre `AgentPersistence::memoria_*` (mediano; incluye
   sanitizado + tests de secretos).
3. Curador como tarea recurrente + políticas (mediano).
4. `skills_registrar` o vía del consumidor (decisión de diseño pendiente).
5. Tools de memoria al agente (evaluar tras 1–3).
