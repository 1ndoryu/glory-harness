# Plan 119A-7 — Agente sólido: auditoría del núcleo vs referencias + bench propio

> ID: **119A-7** · Fecha: 2026-09-11 (profundizado con `supervisor-thinking`)
> · Estado: **activo (planificado, sin implementar)**.
> Origen: pide revisar qué falta antes de probar GH para que funcione bien —
> no la interfaz, sino el entorno donde trabajan las IAs (la inteligencia):
> comparar con todas las referencias de principio a fin y medir con
> benchmarks de agentes para corregir defectos.
> **Solo planificación: sin código tocado.**

## 0. Veredicto de arquitectura (skill `supervisor-thinking`)

**VIABLE CON RESERVAS.** Reservas que este plan ya incorpora: (a) «sólido»
sin número no es exigible → §6 lo define; (b) los runs autónomos de F2/F3
ejecutan comandos generados por el modelo en la máquina de desarrollo →
F0 impone jaula; (c) el modelo es no-determinista → §5 impone protocolo de
repetición; (d) el bench lo escribe quien escribe el agente → §5 impone
congelado anti-sobreajuste; (e) SWE-bench/Terminal-Bench completos son
sobreingeniería aquí (Windows, sin Docker, costo) → F3 los evalúa y el
núcleo exigible es el mini-bench propio.

## 1. Problema y no-goals

- **Problema:** se desconoce si el núcleo agente de GH (tools, permisos,
  modos, recuperación, contexto) sostiene trabajo real de principio a fin;
  los defectos se descubren hoy por reporte del usuario, no por medición.
- **No-goals:** evaluar MODELOS (se mide el harness con modelo fijo y
  registrado, no se comparan proveedores); perseguir SWE-bench completo;
  tocar UI; convertir el bench en puerta del gate (es costoso y fluctuante).

## 2. Punto de partida: hechos confirmados vs supuestos

- **Hechos (11-09):** referencias en `data/referencias-cli/` (`opencode`,
  `hermes-agent`, `grok-cli`, `claurst`, `vscode`); tools en
  `core/src/herramientas/` (`file_write`, `file_patch`, lectura, `comando`,
  `content_search`, `repo_map`, `skill`, `tareas`, `todo`, `web_search`,
  `web_fetch` con proveedor inyectado, `navegador`, `mcp`; permisos en
  `tool.rs`); sin infra de evaluación en el repo; runs vía CLI (`run`,
  `schedule run`, `web`, Tauri).
- **Supuestos a confirmar en F1:** qué cubre cada referencia que GH no;
  si el daemon temporiza `schedule run` o el tick es solo manual; si el
  modelo puede auto-gestionar tareas/memoria con las tools actuales.

## 3. Fases

- **F0 — Jaula y protocolo de medición (prerrequisito, sin modelo).**
  Directorio temporal por run fuera del árbol (se limpia solo), workspace
  raíz fijado ahí, sin secretos ni red salvo tarea que la exija, comandos
  destructivos fuera de la jaula prohibidos por construcción, runs
  supervisados. Protocolo: modelo fijado + versión registrada, topes
  (turnos, costo y tiempo por run) y repetición ×3 (pasa si ≥2).
  Verificación: un run de humo que demuestra que la jaula contiene
  (`file_write` fuera de la raíz falla, `comando` no sale del dir).
- **F1 — Matriz de contrato vs las 5 referencias (solo lectura).**
  Contrato GH (tools + calidad de sus descripciones, permisos/approvals,
  modos, skills, memoria/curador, subagentes, MCP, sandbox/límites, truncado,
  errores reintentables, compactación, rewind) cruzado con cada referencia;
  cada gap → tarea F4 o descarte con razón. Salida: tabla + top 10 por
  impacto. Sin código.
- **F2 — Flujo canónico end-to-end (11 pasos, pasa/falla por paso).**
  En jaula F0, con modelo real: 1 crear archivo, 2 leerlo, 3 modificarlo con
  patch, 4 buscar en el repo, 5 ejecutar comando y usar su salida, 6 test
  verde, 7 aprobación de acción con efecto, 8 rewind, 9 compactar sin perder
  lo esencial, 10 turno solo-lectura que no escribe, 11 error provocado
  (comando que falla) que el agente diagnostica sin inventar. Cada fallo =
  defecto con evidencia del turno. Criterio: 11/11 en ≥2 de 3 runs.
- **F3 — Bench: evaluar los públicos, construir el propio.**
  Evaluar SWE-bench / Terminal-Bench contra las restricciones (Windows, sin
  Docker, costo, red): lo inviable se descarta con razón escrita, no se
  persigue. Núcleo: mini-bench versionado (12–20 tareas deterministas sobre
  fixtures, sin red) en 4 familias —operaciones de archivo, búsqueda y patch
  preciso, comandos y diagnóstico, disciplina (aprobaciones, solo-lectura,
  recuperación de error)— graduadas por asserts + comandos, con arnés
  headless que puntúa por tarea. **Anti-sobreajuste:** las tareas se
  congelan antes de F4; F4 no puede editar tareas, solo el agente; se
  reserva un 25% como conjunto ciego que solo se corre al final.
- **F4 — Corrección y re-medición.**
  Top de F1/F2/F3 por impacto; fixes en `core` (contrato/tools), nunca
  parches en el arnés para pasar; segunda pasada de flujo + bench con
  antes/después numérico en la completada.
- **F5 — Regresión periódica manual (solo si F4 cierra).**
  Subconjunto corto (~5 tareas) de cadencia manual; nunca puerta del gate.

## 4. SOLID y dónde vive cada cosa

- El arnés (runner, graduadores, fixtures) es herramienta de medición, no
  producto: vive fuera de `core`/`cli` como módulo propio de evaluación
  (ubicación exacta a decidir en F3; si solo hay scripts + fixtures, sin
  crate nuevo — YAGNI).
- Los fixes van al contrato (`core`: tools, permisos, truncado, errores) o
  al consumidor (`cli`), nunca al arnés para maquillar el número.
- Graduadores puros por tarea (una responsabilidad cada uno); el runner solo
  orquesta, puntúa y registra (modelo, versión, costo, seed si existe).

## 5. Eficiencia, costo y riesgos con mitigación

- Mini-bench antes que benchmarks públicos: una tarea propia cuesta
  céntimos y minutos; SWE-bench completo cuesta ordenes más y exige Docker.
- **Costo:** cada número publicado lleva su costo (modelo + llamadas);
  topes F0 cortan runs desbocados.
- **No-determinismo:** protocolo ×3 del F0; un defecto que aparece 1/3 se
  registra como fluctuante, no como sólido.
- **Seguridad (runs autónomos):** jaula F0 + supervisión; si una tarea
  necesita red o escritura fuera, se rediseña o se cae.
- **Sobreajuste:** congelado + conjunto ciego (§3 F3); quien fija puede
  proponer tareas nuevas, nunca editar las congeladas.

## 6. Criterios de aceptación («sólido» = números)

- Flujo F2: 11/11 pasos en ≥2 de 3 runs, con modelo y costo registrados.
- Bench: puntuación publicada antes/después; el conjunto ciego solo se abre
  una vez y su número acompaña al principal.
- Cada defecto F4 lleva reproducción mínima + fix + test; sin eso no cierra.

## 7. Gate, evidencia y documentación

- Gate canónico `sentinel check 119A-7 --stages
  scripts/quality/stages.json` PASS en fases con código; `cargo test` +
  clippy del arnés; el bench NO entra al gate.
- Evidencia en `Agente/completados/` (matriz F1, tabla 11 pasos, tabla
  bench antes/después con modelo y costo); prevención en
  `Agente/prevencion/` si aparece un modo de fallo repetible (jaula,
  fluctuación, sobreajuste); roadmap se actualiza al cerrar cada fase.

## 8. Riesgos abiertos

Modelo base que cambia bajo los pies (fijar versión mitiga, no elimina);
fixtures que se vuelven obsoletos; referencias que divergen de su upstream.

## 9. Estado y siguiente paso

## 10. Reto F0/F1 (11-09, verificado contra el código)

Sin código tocado; correcciones al plan antes de arrancar:

1. **§2 inventario impreciso.** Las tools viven en
   `archivo/{tools_archivo.rs,content_search.rs}`, `comando.rs`,
   `repo_map.rs`, `skill.rs`, `tareas.rs`, `todo.rs`, `tools_web.rs`,
   `mcp.rs`, `tool.rs` (registro+permisos) y dir `navegador/`. F1 debe
   partir de este inventario, no del listado aproximado de §2.
2. **Compactación YA existe** (no es gap): automática por ocupación
   (`nucleo/context.rs`, anti-thrash, piso 512K), manual `/compactar`
   (`turno/mod.rs:460`), hooks `PreCompact` vetables (`hooks.rs:84`).
   F2-paso 9 mide su calidad, no su existencia.
3. **Rewind a nivel archivo YA existe pero SIN exponer**: `historial.rs`
   (`tomar_checkpoint`/`revertir_ultimo`, pila LIFO acotada, fail-closed
   fuera del sandbox) solo lo usa `aplicar_plan_con_checkpoint`
   (`plan.rs:169`) + tests. F2-paso 8 debe fijar ANTES el driver
   (¿tool `deshacer`? ¿comando CLI? ¿vía shell enjaulado?) — candidato
   F4 ya visible: exponer undo al modelo.
4. **Supuesto §2 resuelto: el tick es solo manual.** `daemon.rs` es un
   daemon de sesiones TCP NDJSON en loopback, NO un loop de
   scheduler; `schedule run`/`ciclo_scheduler` solo corren a mano
   (el loop único en daemon es 119A-6 F3, pendiente). F0/F2 no pueden
   asumir disparos periódicos.
5. **Jaula F0 viable sin cambio de arquitectura**: `SandboxArchivos`
   (`contrato/sandbox.rs:60`) canonicaliza la raíz, solo acepta rutas
   relativas, prohíbe `..`, resuelve symlinks y deniega secretos
   (`.env`, `*_KEY`, `*.pem`…); el runtime la construye sobre un
   `workspace` configurable (`runtime/mod.rs:695`). F0 = apuntar el
   workspace a un temporal + humo (`file_write` fuera falla).
   Punto abierto a verificar en F0: confinamiento de `comando`
   (cwd enjaulado, sin escape vía shell).
6. **Aprobaciones existen**: flujo plan→aprobar con checkpoint
   (`plan.rs` e2e `e2e_aprobar_con_checkpoint_y_undo`). F2-paso 7 lo
   ejerce, no lo construye.

Veredicto del reto: el plan sigue **VIABLE**; F0/F1 arrancan con los
puntos 1–6 incorporados. Sin cambios en fases, gate ni DoD.

**SIGUIENTE ACCIÓN:** arrancar F0 (jaula + protocolo) y luego F1 (matriz,
solo lectura). **AUTORIZADO PARA EJECUTAR** el ciclo local (investigar,
editar, probar, gate, commit) cuando se arranque; nunca deploy ni
escrituras fuera de la jaula; SSH prohibido siempre.
