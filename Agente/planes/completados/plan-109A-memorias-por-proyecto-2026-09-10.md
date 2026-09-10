# Plan 109A — Memorias por proyecto + hook pre-compact (gaps vs VS Code)

> IDs roadmap: **109A-1/2/3** · Fecha: 2026-09-10 · Estado: **COMPLETADO**
> (F1, F2 y F3 hechas; cerrado 2026-09-10)
> Origen: comparativa VS Code (§ memorias/compactación, 09-09): VS Code aporta
> hook `PreCompact` y memoria versionable por ámbitos; GH compacta y cura mejor
> pero su memoria es global por usuario y no tiene hook previo.

## 1. Estado actual (verificado)

- Memoria solo por `user_id` → **global entre proyectos**: `memoria_listar/upsert/borrar`
  (`cli/src/persistencia_sqlite/puerto.rs:145-223`), tabla `memoria`
  (`cli/src/persistencia_sqlite.rs:93` + `ALTER` origen/usos/ultimo_uso `:147-153`).
- Skills (`con_skills_base`) globales por usuario — **se quedan globales** (no alcance).
- Compactación automática en `core/src/nucleo/context.rs`: umbral 0.5, piso 0.75
  (<512K), degenerado 0.85, `pct_compactar` por consumidor (F6), anti-thrash,
  conserva cola, no interrumpe tool (`:198-299`). Sin punto de extensión previo.
- Curador nativo (`nucleo/memoria/curador.rs`, `es_peticion_curador` en `cron.rs:65`);
  sanitize anti-secretos (`nucleo/memoria/sanitize.rs`); CLI
  `memoria <listar|recordar|guardar|borrar|curar>` (`cli/src/main.rs:210,258`).
- Identidad de proyecto: tabla `workspaces` + `comun.workspace` (`elegir_workspace`).
- Modal de configuración declarativo (`desktop/ui/src/dominio/opciones.ts`: controles
  texto/seleccion/segmentado/lectura/valor/booleano; montar en
  `componentes/modal.ts` + `orquestador/vistaModal.ts`).

## 2. Objetivo

Memorias **estrictamente por proyecto** (nunca mezcladas), hook ejecutable antes de
compactar, export/import por ámbito estilo VS Code (`project` versionable / `local`),
y sección "Memorias" en Configuración para gestionarlas.

## 3. Fases

### F1 — 109A-1 Hook pre-compactación (HECHA 2026-09-10)
- `ContextoConfig.gancho_pre_compact: Option<ComandoGancho>` se ejecuta antes
  de resumir y recibe JSON por stdin (ocupación, mensajes y resumen candidato).
- Runner de comando: timeout cubre spawn/escritura/espera/lectura, stdout limitado
  a 64 KiB, `exit 2` veta, salida JSON permite únicamente `resumen` o
  `resumen_llm`; fallo/timeout/JSON inválido registra warning y continúa.
- Runner HTTP: errores y timeout no exponen URL, credenciales, query, fragmento
  ni ruta; solo se registra origen `esquema://host:puerto`.
- Configuración durable: resolver compartido con warning para JSON inválido;
  aplicado a sesión, `run`, `chat`, `tui` y `schedule run`; `null` elimina el
  hook y la UI devuelve `null` al leerlo.
- Evidencia: `cargo test -p glory-harness-core --lib --quiet` (273/273),
  `cargo test -p glory-harness --lib --quiet` (102/102), Clippy de core+CLI
  con `-D warnings` limpio, build UI `tsc --noEmit && vite build` correcto,
  análisis reducido Sentinel 0 errores/0 warnings en 18 archivos.
- Gate canónico ejecutado dos veces: FAIL por `sccache-no-configurado` y
  317 errores/206 warnings heredados de `data/referencias-cli/**`; no se
  declara PASS del gate global.

### F2 — 109A-2 Memoria por proyecto + export/import (HECHA 2026-09-10)
- Contrato: `AmbitoMemoria { Global | Proyecto(Uuid) }` en `core/src/contrato/ports.rs`;
  `memoria_listar/upsert/borrar` reciben ámbito y `memoria_ambitos` (método por defecto)
  enumera los ámbitos con contenido + el global. `AgentToolContext.ambito_memoria` y
  `TurnoConfig.ambito_memoria` propagan el ámbito hasta las tools y el proveedor.
- Migración real: SQLite no permite alterar la PK, así que `abrir_conexion` renombra la
  tabla a `memoria_pre_109a2`, crea la nueva con `workspace_id TEXT NOT NULL DEFAULT ''`
  y `UNIQUE (user_id, workspace_id, clave)`, copia las filas como globales, elimina la
  vieja y confirma en una transacción; es idempotente (detecta la columna por
  `PRAGMA table_info`).
- Centinela `''` en vez de `NULL`: en SQLite los `NULL` no colisionan en `UNIQUE`, y con
  `NULL` el `ON CONFLICT` del upsert dejaría de ser fiable. Los recuerdos previos quedan
  en el ámbito global (se conservan, no se reasignan a proyectos).
- Ámbito del turno: `ambito_de_ruta` resuelve la carpeta activa contra `workspaces`
  (canoniza y quita el prefijo verbatim de Windows); sin área registrada o si la consulta
  falla, degrada a global con aviso. CLI: `--global`, `--proyecto <uuid|ruta>`, `--todos`;
  un `--proyecto` inexistente es error explícito, nunca un fallback silencioso.
- Curador: `ejecutar_curador_todos` recorre todos los ámbitos (el cron ya no deja sin curar
  los de proyecto) y anota `[proyecto {uuid}]` en las claves del reporte.
- Export/import markdown por recuerdo (`cli/src/infra/memoria_io.rs`): frontmatter
  `clave/origen/usos/ultimo_uso/actualizada_en` + cuerpo; destino `local` (carpeta de datos)
  o `project` (`.glory/memorias` del área, versionable); reexportar reutiliza el archivo y
  el slug desambigua con sufijo. El import aplica `sanitize_para_memoria` a clave y cuerpo:
  un archivo con credencial se omite con motivo y no guarda a medias.
- Evidencia: 393 tests lib verdes (core 117 + CLI 276), clippy `-D warnings` limpio en
  ambos crates, `npm.cmd --prefix desktop/ui run build` OK (85 módulos), `git diff --check`
  limpio. Aislamiento cubierto por pruebas reales: `memoria_aislada_entre_global_y_proyectos`,
  `upsert_repite_clave_solo_en_su_ambito`, `memoria_no_cruza_usuarios`,
  `migra_memoria_legacy_al_ambito_global_sin_perder_datos`, `prefetch_no_mezcla_ambitos`,
  `sync_escribe_solo_en_su_ambito`, `curador_todos_cura_cada_ambito_por_separado` y la ida
  y vuelta de archivos. Gate: ver `Agente/completados/tareas-2026-09-10.md` (FAIL ajeno,
  0 hallazgos en `cli/src`/`core/src`).
- Limitación registrada: el binario CLI no se pudo recompilar para la prueba manual porque
  `glory-harness.exe` estaba en ejecución (bloqueo de escritura); la verificación del
  subcomando se hizo con pruebas de las funciones reales (`destino_export`, `ambito_pedido`,
  ida y vuelta de archivos) en vez de con el proceso manual.

### F3 — 109A-3 Sección "Memorias" en Configuración (HECHA 2026-09-10)
- El esquema declarativo de `opciones.ts` no admite listas gestoras → panel custom
  `componentes/memorias.ts` (261 líneas; ≤300), montado como sección del modal
  (`modal.ts`/`vistaModal.ts`), tokens de `variables.css`, sin literales.
- Funciones implementadas: listar **solo proyecto activo** + buscar (clave y
  contenido), ver detalle, borrar con confirmación en dos pasos ("¿olvidar?"),
  curar, exportar/importar; distintivo de ámbito (`global` invertido vs
  `proyecto`) y aviso explícito cuando el área activa no está registrada (ámbito
  global) para no atribuir al proyecto recuerdos compartidos. Nunca muestra
  recuerdos de otros proyectos: el front no envía identificador alguno.
- IPC Tauri nuevos (`desktop/src-tauri/src/memoria.rs`, 256 líneas):
  `memoria_listar_proyecto`, `memoria_borrar` (idempotente, devuelve listado
  fresco), `memoria_curar`, `memoria_exportar` (destino fijo `.glory/memorias`,
  error si no hay área) y `memoria_importar` (error si falta la carpeta,
  devuelve los omitidos con motivo). El ámbito lo resuelve el backend con
  `area_activa` y degrada a global.
- Export/import compartido: las funciones de carpeta (`exportar_carpeta`,
  `archivos_markdown`, `importar_carpeta`, `ResumenImport`) se elevaron a
  `cli/src/infra/memoria_io.rs` para que CLI y escritorio usen UNA
  implementación del formato; `cli/src/comandos/memoria.rs` solo resuelve el
  destino y delega.
- En modo web el transporte **rechaza** las memorias con motivo explícito
  (`adaptadores/apiMemorias.ts`): una lista vacía se leería como "no hay
  recuerdos" y mentiría sobre el estado real.
- Evidencia: `cargo test -p glory-harness --lib` (**120 verdes**, incluye
  `areas_no_mezclan_recuerdos_al_exportar_e_importar`, que usa SQLite real con
  dos áreas registradas), `cargo test -p glory-harness-core --lib` (276),
  `cargo test -p glory-harness-desktop` (11), clippy `-D warnings` limpio en los
  tres crates, `npm.cmd --prefix desktop/ui run build` OK (88 módulos, `tsc`
  EXIT 0). Gate canónico: ver `Agente/completados/tareas-2026-09-10.md`
  (FAIL ajeno por `sccache`, 0 hallazgos nuevos propios; los 10 warnings que
  quedan son preexistentes de 109A-1/109A-2).

## 4. No alcance

- Skills siguen globales por usuario. La migración no reasigna recuerdos viejos a
  proyectos. Sin memoria semántica/vectorial (KV + curador basta por ahora).

## 5. Definition of Done

`cargo clippy --workspace --all-targets` 0 warnings, `cargo test --workspace` verde,
`tsc` EXIT 0, gate full PASS 0/0/0, E2E 2-proyectos sin mezcla, roadmap actualizado y
completada con evidencia por fase.

### Cierre (2026-09-10)

- Clippy `-D warnings` limpio en `glory-harness-core`, `glory-harness` y
  `glory-harness-desktop`; suites verdes (276 core + 120 CLI + 11 escritorio) y
  build de UI EXIT 0.
- Aislamiento entre proyectos cubierto por pruebas reales sobre SQLite (dos áreas
  registradas) en vez del E2E con ventana: abrir una ventana GUI y pulsar botones
  no es viable desde el agente, así que la comprobación manual queda pendiente
  para el usuario y la evidencia es el test de integración más las pruebas del
  DTO del panel. **Limitación registrada, no sustituida por silencio.**
- Gate canónico `sentinel check 109A-3 --stages scripts/quality/stages.json`:
  **FAIL** por `sccache-no-configurado` (ajeno, tarea del roadmap); etapas
  `coverage` PASS 0/0/0, `sentinel` PASS 0 errores con **10 warnings
  preexistentes** (8 `console-production` en el navegador y `limite-lineas` en
  `main.ts`/`panelDerecho.ts`, ya presentes en 109A-1/109A-2). Los dos hallazgos
  nuevos que introdujo la primera pasada de F3 (`limite-lineas` en `api.ts` y
  `large-interface-isp` en `vistaModal.ts`) se corrigieron extrayendo el rechazo
  web a `adaptadores/apiMemorias.ts` y agrupando las acciones en un solo campo
  `memoria`. DoD de "gate PASS 0/0/0" no alcanzable por la etapa `sccache`
  ajena: se registra como deuda, no como PASS falso.
