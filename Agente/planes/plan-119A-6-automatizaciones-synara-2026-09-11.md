# Plan 119A-6 — Automatizaciones estilo Synara + nav temporal sin Agentes/Flujo/Complementos

> ID: **119A-6** · Fecha: 2026-09-11 · Estado: **activo (F1 ejecutado 11-09; F2–F5 planificados)**.
> Origen: pide copiar el sistema de automatización de Synara a GH y ocultar
> temporalmente Agentes, Flujo y Complementos del menú.

## 1. Qué tiene Synara (verificado 11-09 en `area-trabajo/synara`)

- **Modelo** (`packages/contracts/src/automation.ts:37-113`): schedule
  `manual | once | interval | daily | weekdays | weekly | cron` con timezone;
  modos `standalone` (hilo+turno nuevo por run), `heartbeat` (continúa un
  hilo existente) y `dedicated` (un hilo propio que crece); triggers solo
  `manual | scheduled` (sin webhooks entrantes); políticas de notificación
  (`all | failed-runs-only`), de fallo y de completitud; resultado del run
  (`findings | no-findings | changed-files | needs-attention`).
- **Motor** (`apps/server/src/automation/`): `AutomationService` (CRUD +
  `runDueOnce` + `reconcileActiveRuns`), `AutomationScheduler` (bucle: tick
  60 s + wakeup por eventos `definition-upserted/deleted`,
  `Layers/AutomationScheduler.ts:27-90`), `AutomationRunReactor`
  (eventos del ciclo), estados
  `pending → claimed → running → waiting-for-approval → succeeded | failed |
  cancelled | interrupted | skipped`.
- **Autoría**: tools para que el modelo cree/gestione automatizaciones
  (`agentGateway/automationTools.ts` + `automationAuthoringGuidance.ts`) y UX
  de creación conversacional con borrador (`plans/003-*`, `automationDraft.ts`,
  `automationInlineDraft.ts`); propuestas con aceptar/descartar.
- **UI**: rutas lista/detalle (`_chat.automations*.tsx`) con historial de runs.
- **No entra** (sin equivalente o propio de Synara): modo `worktree`,
  panel Environment, captura Studio, perfiles de subagente.

## 2. Qué ya tiene GH (base a reutilizar, no reescribir)

- `ProgramadorTareas` (ports) + impl SQLite (`persistencia_sqlite/tareas.rs`)
  y memoria; CLI `schedule list|create|remove|logs|run` (`schedule_cmd.rs`);
  `frase_a_cron` (NL→cron) + `proxima_ejecucion`; `cron::ejecutar_lista`
  ejecuta vencidas como turnos; `tareas_recuperar_interrumpidas` (huérfanas).
- Límite actual: solo cron recurrente, sin timezone, sin estados de run, sin
  políticas, sin autoría por el modelo, sin UI, y el tick es invocación
  manual (`schedule run`) — verificar en F2 si el daemon lo temporiza.

## 3. Fases

- **F1 — Nav temporal (HECHO 11-09).** Los tres botones salen del nav de la
  sidebar (`sidebar.ts`); se conservan helper, iconos y rutas `onAccionNav`
  para el retorno. Vuelve el que corresponda cuando su vista exista (F5 trae
  Automatizaciones al nav).
- **F2 — Modelo y persistencia (RETADO 11-09, ver §7).** `ScheduleTarea`
  (`manual | una_vez | intervalo | diario | entre_semana | semanal | cron`)
  + `zona_horaria` IANA (nueva dep `chrono-tz`) como lenguaje canónico;
  columnas `programacion` + `zona_horaria` + políticas en `tareas`
  (migración estilo MIGRACIONES); `tarea_logs` EXTENDIDA (no tabla nueva:
  `iniciado_en`, `finalizado_en`, `resultado` opcional con taxonomía
  honesta); reprogramación de `ejecutar_lista` y `ciclo_scheduler` por la
  vía nueva (tz-aware); CLI: `create --programacion/--en/--zona`,
  `run <id>` explícito para manuales, `list` muestra clase+zona+política.
  Verificación: tests del crate sobre BDs temporales (patrón existente
  `temp_dir`), sin tocar la BD real (sin override de ruta).
- **F3 — Motor.** Bucle planificador (tick + wakeup al crear/borrar;
  `runDueOnce` + reconciliación extendiendo `recuperar_interrumpidas`);
  modos `standalone` (= turno en hilo nuevo, lo actual) y `dedicated`
  (reutiliza el hilo de la tarea); `heartbeat` solo si continuar hilo
  existente es seguro con M1 (verificar; si no, se cae con razón). Runs que
  piden aprobación viajan por las tarjetas existentes. Sin avisos en
  desatendido (criterio 069A-3 ya vigente).
- **F4 — Autoría por el modelo.** Tool `schedule` para crear/listar/cancelar
  (con confirmación según clasificación) + guía de autoría + creación
  conversacional (`/automatizar` o menú `/`, a decidir en la fase) con
  borrador confirmable antes de persistir.
- **F5 — UI.** Vista Automatizaciones (lista, detalle con historial/logs,
  crear/pausar/cancelar) con la familia visual de la app; entrada en el nav
  al cerrar la fase. Reutiliza patrones de Files/Git (pane + recarga al
  cambiar de área).

## 4. Reglas

Esquema y errores en español; turnos desatendidos sin toast salvo fallo con
política `all`; nada mudo (logs de run consultables); límites de líneas
vigentes; cada fase con su verificación (tests `cli`/`core`, `tsc`+`vite`
para F5, E2E de un `once` y un `interval` reales en F3).

## 5. Gate y Definition of Done

Gate canónico `sentinel check 119A-6 --stages
scripts/quality/stages.json` PASS antes de cada commit de fase; commits en
español con ID; evidencia en `Agente/completados/`.

## 6. Estado y siguiente paso

**Estado 11-09 (tarde):** F2 implementado y verificado —gate
`sentinel check 119A-6` PASS (456 tests ok, 0 fallidos, clippy 0;
reporte en `.quality-reports/check/119A-6/latest.md`). Núcleo
(`schedule.rs` + `ciclo_scheduler` tz-aware + `ejecutar_lista` con
`Plan::Repetir/UnaVez/Rota` + `planificar()` extraído), SQLite/memoria
migrados (columnas `programacion/zona_horaria/notificacion/reintentos`,
logs con `iniciado_en/finalizado_en/resultado`), CLI (`create
--programacion/--en/--zona`, `run <id>`, `list` con clase+zona+política),
tool NL con `zona` fail-closed. Desbloqueo de disco autorizado por el
usuario («borra todo tmp si es necesario»): borrado `%TEMP%` +
`vscode-test-cssvar` (1,1 GB) + caché sccache (3,69 GB) → 10,1 GB libres.
Regresión encontrada por el gate y corregida: `texto()` normaliza
`diario/entre_semana/semanal` a cron de 5 campos pero `parse()` solo
aceptaba la forma corta `M H` (2 fallos tz-aware) → `nueva()` acepta
también la forma normalizada validando el DOW de la clase
(`cron_desde_hora_o_cron`) + test de roundtrip
`texto_parse_redondo_en_todas_las_clases`.

**Siguiente paso:** evidencia en
`Agente/completados/tareas-2026-09-11.md` → commit F2. Luego 119A-7
F0/F1. F1 verificado: `tsc --noEmit` EXIT 0 + `vite build` OK el 11-09
tras ocultar el nav.

## 7. Reto F2 (11-09, verificado contra el código)

- **C1 Motor duplicado.** `correr_scheduler`/`ciclo_scheduler` existen y
  están testeados pero SIN llamador productivo; el camino productivo es
  `schedule run` → `cron::ejecutar_lista`. F2 hace `ejecutar_lista`
  consciente del schedule; F3 cablea UN loop (candidato: `daemon.rs`, que
  ya existe con SSE+token, no un proceso nuevo) y converge los dos
  caminos en vez de sumar un tercero.
- **C2 Timezone real exige `chrono-tz`.** No hay base tz en el árbol
  (lock sin `chrono-tz`); canonicalizar a UTC en creación deriva con el
  DST. `chrono-tz` es puro Rust (encaja con rusqlite bundled/rustls, cero
  deps de sistema). Costo aceptado: descarga + compilación. Zona IANA
  validada en creación (fail-closed); defecto UTC explícito.
- **C3 Kinds casi existentes.** intervalo→`cadaN`, diario→`diario`/cron,
  semanal→cron DOW, cron→v2, una_vez→`una_vez`+proxima ya funcionan; lo
  nuevo es el lenguaje canónico + zona. `weekdays` (§1) se había caído de
  la lista F2: se recupera como `entre_semana`.
- **C4 cron v2 no acepta rangos.** `parse_rango` (scheduler.rs:143) solo
  número o `*`: `1-5` es inválido hoy. Corrección F2: DOW acepta `1-5` y
  listas (`1,2,3,4,5`); resto de campos sin cambios (mínimo).
- **C5 `manual` hoy no se puede ejecutar.** `schedule run` solo corre
  vencidas (`seleccionar_vencidas` exige proxima pasada). Corrección F2:
  `schedule run <id>` explícito (sirve también como reintento manual).
- **C6 Granularidad mínima 1 min.** `parse_cantidad_unidad` ya rechaza
  N<=0; intervalos sub-minuto se rechazan con error (el tick es manual;
  hueco documentado vs segundos de Synara, no promesa).
- **C7 Runs: extender `tarea_logs`, no crear tabla.** El ledger existe y
  tiene escritor vivo (`ejecutar_lista` → `tarea_registrar_log`): F2
  añade `iniciado_en/finalizado_en/resultado` y extiende la firma del
  trait (implementadores en-repo: sqlite, memoria y fakes de tests).
- **C8 Taxonomía honesta.** Solo `archivos_cambiados` (tools de escritura
  en la salida) y `necesita_atencion` (fallo) son mecánicamente
  decidibles; `hallazgos/sin_hallazgos` requieren autodeclaración del
  modelo (F4): `resultado: Option`, `None` = sin clasificar.
- **C9 Estados: vocabulario GH, sin `claimed`.** El claim es `tarea_tomar`
  atómico (sin estado intermedio); `interrupted` de Synara equivale a
  reencolar vía `recuperar_interrumpidas`, no a estado terminal;
  `en_aprobacion` es F3 con las tarjetas existentes. Mapeo documentado,
  sin renombrar estados.
- **C10 Políticas dormidas hasta F3 (aceptado).** Notificación default
  `fallos` (=069A-3 desatendido sin avisos), reintentos default 0; F2
  persiste+valida+muestra, F3 aplica.
- **C11 Sin humo contra la BD real.** `abrir_tiendas_durables` no tiene
  override de ruta: F2 se verifica con tests sobre BDs temporales
  (precedente `glory-memoria-*.db`, `gh-web-test-*.db`). E2E `once` +
  `interval` reales quedan en F3 con entorno aislado.
- **C12 Disco.** C: 7,95 GB < 8 GB del preflight `rust` el 11-09: revisar
  antes del gate F2; si bloquea, liberar (sweep/targets viejos).
