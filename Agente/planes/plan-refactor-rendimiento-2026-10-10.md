# Plan 10AA-1 — Refactor y auditoría de rendimiento (re-planificado 2026-10-10)

**Estado:** planificado, sin ejecutar. F0 y F1 listos para empezar en cuanto se apruebe (defaults de D1–D4). F2 y F3 esperan a D1.
**Siguiente paso:** F0 — línea base medida (§5).
**Sustituye como base:** plan `Agente/planes/completados/plan-saneamiento-calidad-2026-09-05.md` (059A-1, cerrado 05-09) y las auditorías de 07-09 y 13-09. La remediación `Agente/planes/completados/plan-139A-8-remediacion-auditoria-2026-09-13.md` (cerrada 14-09) resolvió R2, R4 y S3–S5. Este plan parte del estado actual del código, no de esos documentos.

## 1. Objetivo

Cerrar lo que la auditoría integral de 13-09 deja abierto o parcial, en R (rendimiento) y S (arquitectura), con una medida antes y después de cada cambio de rendimiento. Sin cambios de seguridad.

## 2. Estado de partida (verificado 2026-10-10)

Recuento sobre 27 hallazgos: **5 resueltos, 14 parciales, 8 abiertos**. Evidencia por hallazgo en `Agente/documentacion/auditoria-integral-2026-09-13.md`; los `archivo:línea` de abajo son los actuales.

| ID | Estado | Evidencia actual |
|---|---|---|
| R1 | PARCIAL | `cli/src/persistencia_sqlite.rs:331` `Arc<Mutex<Connection>>` único; I/O ya en `spawn_blocking` (`:363-376`) |
| R2 | RESUELTO | `cli/src/persistencia_sqlite.rs:363-376` |
| R3 | PARCIAL | `cli/src/servicio/sesion.rs:510-518` carga incremental; resto sin verificar |
| R4 | RESUELTO | `cli/src/persistencia_sqlite.rs:253-258` |
| R5 | PARCIAL | `cli/src/comandos/web/turnos.rs:271,560` coalescing 75 ms; quedan `to_value` por token |
| R6 | PARCIAL | `desktop/ui/src/componentes/panelChatTurno.ts:163-174` |
| R7 | PARCIAL | `desktop/ui/src/componentes/panelChatHistorial.ts:78,155` repintado completo |
| R8 | PARCIAL | `cli/src/ui/turno.rs:76,85-89` clones residuales |
| R9 | ABIERTO | `cli/src/persistencia_sqlite/eventos_turno.rs:60-67` SELECT sin LIMIT |
| R10 | ABIERTO | `core/src/herramientas/archivo/content_search.rs:18` Mutex por índice |
| R11 | ABIERTO | `desktop/ui/src/orquestador/arranque.ts:86-94` reloj de 1 s sin pausa |
| R12 | ABIERTO | `Cargo.toml` `[profile.release] lto = "thin"` |
| S1 | PARCIAL | `core/src/herramientas/registro.rs:112-608` (`tool.rs` es fachada; registro mezcla permisos) |
| S2 | PARCIAL | `core/src/nucleo/runtime/mod.rs:81` submódulos creados, struct intacto |
| S3–S5 | RESUELTO | `core/src/contrato/ports.rs` |
| S6 | ABIERTO | `core/src/nucleo/llm/mod.rs:55` `reqwest` concreto en core |
| S7, S8, S9, S10, S13, S14 | PARCIAL | ver auditoría; sin cambios previstos salvo S14 (ver F3) |
| S11 | ABIERTO | `core/src/nucleo/cron.rs:151-159` sin visitor ni exhaustividad |
| S12 | ABIERTO | `core/src/nucleo/runtime/construccion.rs:149-186` registro manual |
| S15 | ABIERTO | `desktop/ui/src/tauri/real.ts:6` barrel `export *` |

Gate: `.quality-reports/analyze.json` (mtime 07-10): 0 errores, 0 avisos, 1 hint (`desktop/ui/src/orquestador/vistaModal.ts:61`), 312 archivos.

## 3. Alcance y no alcance

**Dentro:** R1, R5–R12; S1, S2, S6, S11, S12, S14, S15 (solo lo que siga abierto o parcial con evidencia).

**Fuera:**
- K1–K13 de seguridad (K1 es CRITICAL: inyección de shell en la tool `comando`). Tarea aparte; mezclarla con refactor oculta el riesgo.
- Features del plan `Agente/planes/plan-119A-7-agente-solido-bench-2026-09-11.md`. Se reutiliza su harness si encaja; no se duplica.
- Reformateo masivo (`cargo fmt`) y diffs de rustfmt preexistentes.

## 4. Dependencias y solapes

- **119A-7 (activo, F2 en curso):** solapa en `core/src/herramientas/tool.rs`, permisos y modos (S1, S2, S12) y en sandbox, eventos y bench sin red (S6, S7, S11). Regla: F2 y F3 no tocan esos archivos hasta cerrar su F2 con run válido 11/11, o hasta acordarlo en D1.
- **129A-9 (en curso, configuraciones Synara):** toca `desktop/ui`. R6, R7, R11 y S14/S15 van en commits separados y tras confirmar que 129A-9 no toca los mismos ficheros.
- **Disco:** `C:\tmp` ≤ 20 GB (AGENTS §0.2). Build con `scripts/run-cargo.mjs` y `CARGO_TARGET_DIR=C:\tmp\glory-target\<rama>`. Un build completo tarda ~7 min: preferir `cargo check` y tests por crate.

## 5. Fases

### F0 — Línea base (sin cambios de código)

Métricas, cada una con método reproducible, 5 repeticiones, mediana, mismo equipo:

- **M1 (R1):** p95 de lecturas concurrentes (8 hilos) mientras escribe un turno. Test de integración nuevo en `cli/`.
- **M2 (R9):** `eventos_turno_listar` con 10 000 eventos: ms y filas devueltas.
- **M3 (R7):** ms para repintar el historial con 500 mensajes. Si no hay harness en `desktop/ui`, medir en la ventana Tauri y documentar el método.
- **M4 (R11):** despertares por segundo del reloj con la app en segundo plano.
- **M5 (R12):** tiempo de `cargo build --release` y tamaño del binario con `thin` frente a `fat`. Solo medir.
- **M6 (R5):** serializaciones `to_value` por token en un turno sintético de 2 000 tokens.

Además: `cargo test --workspace` (delegado a `verificador`, solo fallos) para fijar el número verde de partida.

Salida: `Agente/documentacion/auditoria-rendimiento-2026-10-10.md` con la tabla «antes».

### F1 — Rendimiento de bajo riesgo (no toca los solapes de 119A-7)

- **R9:** LIMIT o paginación en `eventos_turno_listar`; revisar que el índice de R4 cubra `(turno_id, id)`. Criterio: M2 baja ≥ 10× o queda < 50 ms; orden de emisión intacto (test).
- **R11:** el reloj solo corre con turno en curso y ventana visible; se para con `document.hidden`. Criterio: M4 = 0 con turno inactivo.
- **R8:** quitar clones residuales en `cli/src/ui/turno.rs:76,85-89`. Criterio: tests existentes verdes, salida igual.
- **R6:** completar el fix del cierre de turno en `panelChatTurno.ts:163-174`. Criterio: sin regresión en la ventana Tauri.
- **S15:** exports explícitos en `desktop/ui/src/tauri/real.ts:6`. Criterio: type-check y build de UI verdes.

### F2 — Rendimiento estructural (no bloqueado por 119A-7)

- **R1:** separar lecturas de la escritura (WAL + conexiones de lectura), escritura serializada. Decisión según M1: si sin la separación el p95 de lectura es < 5 ms con escritura concurrente, no se hace y se documenta.
- **R10:** `RwLock` o conexión por búsqueda en `content_search`. Criterio: búsquedas concurrentes sin serialización (medido).
- **R7:** repintado incremental de `panelChatHistorial.ts` (solo filas nuevas o cambiadas). Criterio: M3 baja ≥ 5×; scroll y selección intactos.
- **R5:** serializar el payload del token una sola vez, fuera del bucle. Criterio: M6 = 1 serialización por token o menos.
- **R12:** LTO según M5. Default: mantener `thin` salvo que `fat` mejore binario o tiempo ≥ 10 % con un build aceptable.

### F3 — SOLID (tras cerrar 119A-7 F2, o según D1)

Orden por impacto y riesgo:

1. **S11:** `core/src/nucleo/cron.rs:151-159`, match exhaustivo sin catch-all. Criterio: añadir una variante rompe la compilación (test).
2. **S1:** partir `core/src/herramientas/registro.rs` (permisos frente a registro). `tool.rs` queda como fachada. Criterio: API pública del core sin cambios, o documentada; tests de herramientas verdes.
3. **S2:** `core/src/nucleo/runtime/mod.rs:81`, mover el struct a su submódulo. Criterio: tests del runtime verdes.
4. **S6:** `reqwest` detrás de un port; `core` no depende del cliente HTTP concreto. Criterio: el bench sin red de 119A-7 sigue corriendo.
5. **S14:** `desktop/ui/src/adaptadores/apiArchivos.ts:6` deja de importar un componente. Solo si es trivial.
6. **S12:** diferido. Coste alto y bajo beneficio; se registra en prevención si F3 no deja holgura.

S7, S8, S9, S10, S13 quedan PARCIAL y documentados, sin cambios previstos.

### F5 — Cierre

- Tabla «después» (M1–M6) en `auditoria-rendimiento-2026-10-10.md`.
- Estado nuevo de R y S en ese mismo documento. La auditoría de 13-09 recibe solo una línea de puntero, sin reescribirla.
- Gate: `cargo test --workspace` + tests de UI + lint del proyecto. Sentinel según D2.
- Plan a `Agente/planes/completados/`; evidencia en `Agente/completados/tareas-<fecha>.md`; tarea fuera del roadmap.
- Ramas: integrar en `main`, copia `git bundle` antes de borrar cualquier rama.

## 6. Riesgos

- Colisión con 119A-7 en `tool.rs`, `runtime/` y sandbox. Mitigación: D1 y commits pequeños por hallazgo.
- R1 cambia la persistencia que usan los turnos. Mitigación: M1 decide antes de tocar; tests de la base de datos existentes.
- Disco y tiempo de build. Mitigación: `cargo check` y tests por crate durante el trabajo; build completo solo en F5.

## 7. Decisiones (ordenadas por recomendación)

- **D1 — Coordinación con 119A-7.** Recomendado: F0 y F1, y de F2 lo que no toca sus archivos (R1, R5, R7, R10, R12), ya; F3 (S1, S2, S6, S11) tras su F2 válido 11/11. Alternativa: congelar 119A-7 hasta cerrar este plan.
- **D2 — Gate de cierre.** Sentinel figura en mantenimiento. Recomendado: tests del workspace + tests de UI + lint; Sentinel solo si la usuaria confirma que está operativo. Alternativa: ejecutar Sentinel en F5.
- **D3 — Alcance de seguridad.** Recomendado: K1–K13 fuera; K1 como tarea aparte. Alternativa: incluir K1 aquí (no recomendado).
- **D4 — Lectura del encargo.** Recomendado: glory-harness 059A-1. Alternativa: MN-Inmobiliaria 08AA-6 (broadcast-mutex y god-object), que sería otro plan en `MN-Inmobiliaria/Agente/planes/`.

## 8. Verificación y Definition of Done

- Cada fase: tests del crate afectado (filtro por archivo) y `cargo check` del workspace al cerrar el bloque.
- Cambios visibles (R6, R7, R11): recorrer el flujo en la ventana Tauri: enviar turno, historial largo, cambiar de hilo.
- DoD: M1–M6 con números antes y después; estado nuevo de R y S con `archivo:línea`; roadmap, plan, evidencia y documentación actualizados; sin cambios de seguridad; ramas integradas.
