# Roadmap — glory-harness

Glory Harness es un workspace Rust (`core` + `cli` + `desktop/src-tauri`) con UI Tauri 2 y
TypeScript vanilla en `desktop/ui`; rama primaria `main` y gate de cierre Sentinel/VarSense en
`.quality-tools-harness`. Se compila siempre en `C:\tmp` (`CARGO_TARGET_DIR`); desde 16-10 (16AA-1)
el desktop corre nativo en la VPS 89 (`scripts/desktop-correr-linux.sh`, tmux `gh-desktop`), y el
navegador interno WebView2 sigue siendo exclusivo de Windows.

## Siguiente bloque

- [ ] `129A-9` — **Configuraciones Synara: inventario, decisión y réplica.**
  F1 tabla panel→opciones→equivalente GH en el plan §F1 (decisión del usuario registrada 12-09:
  replicar todo, empezar por Perfil y tema; tema ya existe en 129A-12). F2 bloques Perfil +
  Proveedores HECHOS 07-10 (`nombre_perfil` + `handle_perfil`; lista ESTADO read-only con vivo +
  `en uso` + `recargar`; F3 smoke vivo de Proveedores HECHO con Chromium headless: 4 filas
  `disponible · 1 clave(s)`, `4 de 4`, 0 errores JS; F3 de Perfil pendiente). F2 bloque Modelos
  HECHO 07-10 (modelos personalizados por proveedor: editor con validación + fusión en el selector +
  persistencia `modelos_personalizados`; sin "generation defaults": GH no genera commits/PRs; F3
  smoke vivo HECHO con Chromium headless: añadir/duplicado/vacío/selector/quitar/recarga, 0 errores
  JS). Mostrar el nombre en barra/sidebar = bloque posterior. 12 paneles inventariados.
  Plan: `Agente/planes/plan-129A-9-configs-synara-2026-09-12.md`.

## Pendientes

- [ ] `149A-2` — **Precisión reglas 149A-1 (11 FP workspace + allowlist).**
  Derivado del gap H11-(b) registrado en plan §F4: path-join sin proveniencia (6 FP:
  literales/UUID/slug/hash/`.git`), sqlite sin análisis de dependencias (5 FP, `join!` imposible);
  cablear `htmlProductoresPermitidos` con `mensajesUtil.ts`; evaluar 2 TP-bajo
  (`content_search.rs:399`, `skill.rs:183`). Propiedad: repos dueño.

- [ ] `119A-3` — **Ordenar proyectos e hilos.**
  F1 hilos implementado y gate PASS 11-09 (441 tests OK; pendiente verificación `tauri dev` +
  commit): botón de orden en la cabecera `Proyectos` (actividad / creación, con
  `sidebarOrdenHilos.ts`); backend expone `creada_en` en las 3 queries + constructor desktop (los 4
  literales `InfoConversacion` actualizados). Front `tsc` + `vite` OK. Extra:
  `onRenombrarProyecto/onEliminarProyecto` movidos a `orquestador/barraLateralProyecto.ts`. F2–F4
  abiertos. Plan: `Agente/planes/plan-119A-3-orden-proyectos-2026-09-11.md`.

- [ ] `119A-5` — **Meta: fix `meta_leer` + tareas fijas y colapsables.**
  F1 implementado y verificado 11-09 (front PASS + Rust vía PASS 119A-3 mismo árbol; pendiente
  verificación Tauri + commit; F2–F3 abiertos): el front invocaba con `conversacion_id` pero Tauri 2
  espera camelCase (`conversacionId`), así que leer/fijar/pausar/reanudar/lograr la meta fallaban en
  la app (en web no); F1 pasa las claves a camelCase y audita `panel_id`/`solo_lectura`/`turno_id`
  (caían a default en silencio). F2 saca las tareas del transcript a zona fija colapsable como la
  meta; F3 distingue «sin meta» y no dispara comandos en blur sin cambios.
  Plan: `Agente/planes/plan-119A-5-meta-leer-tareas-fijas-2026-09-11.md`.

- [ ] `119A-6` — **Automatizaciones estilo Synara.**
  F1 hecho 11-09, F2 implementado y verificado 11-09 (gate `sentinel check 119A-6` PASS: 456 tests
  ok, 0 fallidos, clippy 0); F3–F5 planificados. `ScheduleTarea`
  (`manual|una_vez|intervalo|diario|entre_semana|semanal|cron`) + zona IANA (`chrono-tz`),
  `tarea_logs` extendida (no tabla nueva), reprogramación tz-aware en `ejecutar_lista`/`ciclo_scheduler`,
  `schedule run <id>` explícito, modos standalone/dedicated (+heartbeat runs con estados, políticas y
  resultado taxonomizado, autoría por el modelo y vista UI); base GH (`ProgramadorTareas` +
  `schedule run`) se reutiliza; worktree/Environment/Studio no entran. Nav temporal sin
  Agentes/Flujo/Complementos (verificado `tsc` + `vite build`).
  Plan: `Agente/planes/plan-119A-6-automatizaciones-synara-2026-09-11.md`.

- [ ] `119A-7` — **Agente sólido: auditoría vs referencias + bench.**
  Planificado y profundizado 11-09, veredicto `VIABLE CON RESERVAS`. No es UI sino la inteligencia:
  F0 jaula + protocolo ×3 (prerrequisito); F1 matriz del contrato agente contra las 5 referencias
  (`opencode`, `hermes-agent`, `grok-cli`, `claurst`, `vscode`); F2 flujo canónico de 11 pasos con
  modelo real (crear→modificar→comandos→tests→aprobación→rewind→compactar→solo-lectura→diagnóstico de
  error); F3 bench (evaluar SWE-bench/Terminal-Bench y construir mini-bench propio congelado con 25%
  ciego); F4 corrección en `core` + re-medición; F5 regresión manual. Aceptación: F2 11/11 en ≥2 de 3
  runs + bench antes/después con costo. Siguiente: F2 flujo 11 pasos (F0-protocolo ×3 cerrado 11-09
  noche: 3/3 PASS con glory/auto, sandbox bloquea fuga en los 3 runs, evidencia
  `C:\tmp\jaula-f0-chat*.log`; `cargo build` SÍ permitido — el guard solo bloquea
  check/clippy/test/bench/fmt; `run` one-shot no ejecuta tools, `chat` con stdin guionizado sí;
  autorizado ciclo local; SSH prohibido).
  Plan: `Agente/planes/plan-119A-7-agente-solido-bench-2026-09-11.md`.

- [ ] `209A-1` — **Consola lateral: visor de ejecuciones en vivo con tope anti-fuga.**
  Planificado 20-09; reapertura acotada de 089A-9, vía eficiente sin PTY por decisión del usuario:
  tab "Consola" en el panel derecho con panel interno (lista de consolas + visor + matar/cerrar/limpiar);
  streaming de `comando` por eventos; topes innegociables (`MAX_CONSOLAS_VIVAS=4`, ring 128 KB,
  transcript 32 KB, `stdin(null)`, reap global, cero polling UI).
  Plan: `Agente/planes/plan-209A-1-consola-lateral-2026-09-20.md` (+revisión 20-09: comando literal
  verbatim, botón abrir-terminal por fila, botón segundo-plano = desacoplar sin matar, `comando_lista`
  para el agente, reap al cerrar conversación o app). Estado: F1 streaming backend cerrado, F2 topes
  + fondo operable cerrado, F3.1–F3.3 UI cerrado, F4-backend cerrado (21-09: F4-resto cerrado con
  retención executor/web/daemon/desktop + tab × con `consola_matar` idempotente). Pendiente 21-09: F3
  parcial + F5 sin evidencia (E2E real en `:8799` con turno vivo + gate). Detalle y correcciones al
  diseño en el plan §Estado/§Correcciones.

## Bloqueos

- Bloqueo `10AA-1` — refactor y auditoría de rendimiento (plan `Agente/planes/plan-refactor-rendimiento-2026-10-10.md`, planificado sin ejecutar): espera las decisiones D1 (coordinar con `119A-7` F2 en `tool.rs`/`runtime/`) y D2 (Sentinel en mantenimiento). Sin ellas no arranca F0; se desbloquea cuando la usuaria las decida.
- Bloqueo `119A-2` — menú contextual de proyecto: F1–F4 hechos y verificados; falta la prueba en ventana Tauri real, que es de la usuaria.
- Bloqueo `119A-4` — ajustes en página: F1 hecho; F2–F4 esperan la decisión de la usuaria sobre F2 y la comprobación visual fina.
- Alerta Dependabot #1 `glib 0.18.5` (08AA-3): bloqueada por upstream — `wry 0.57.0`,
  `tauri-runtime-wry 2.12.1` y `tauri 2.12.3` siguen con gtk 0.18, sin migración a gtk4 posible hoy.
  En vigilancia con revisión mensual, próxima 2026-11-12 (`plan-migracion-tauri-glib-2026-10-12.md`).
- Divergencia conocida (04-09, carril ajeno): el runtime solo persiste turno + respuesta; `chat`/`run`/`tui`
  del CLI no llaman a `guardar_mensaje` para el usuario, así que su historial entre turnos pierde el
  lado usuario. No se toca por ser carril ajeno.
- Defecto de `workspace-manager` (10-09, no de este repo): los números de línea de la consola van
  0-based; al triar un hallazgo hay que mirar la línea siguiente.

## Planes activos

- `Agente/planes/plan-refactor-rendimiento-2026-10-10.md` — `10AA-1` — refactor SOLID y rendimiento; planificado, sin ejecutar (F0 línea base al aprobar).
- `Agente/planes/plan-119A-2-menu-contextual-proyecto-2026-09-11.md` — `119A-2` — menú contextual de proyecto; F2–F4 hechos, pendiente prueba en ventana Tauri real.
- `Agente/planes/plan-119A-3-orden-proyectos-2026-09-11.md` — `119A-3` — orden de proyectos e hilos; F1 hecho, pendiente verificación `tauri dev` + commit.
- `Agente/planes/plan-119A-4-ajustes-pagina-synara-2026-09-11.md` — `119A-4` — ajustes como página estilo Synara; F1 hecha, F2 pendiente de decisión.
- `Agente/planes/plan-119A-5-meta-leer-tareas-fijas-2026-09-11.md` — `119A-5` — fix `meta_leer` camelCase + tareas fijas/colapsables; F3 parcial.
- `Agente/planes/plan-119A-6-automatizaciones-synara-2026-09-11.md` — `119A-6` — automatizaciones estilo Synara; F1–F2 hechos, F3–F5 planificados.
- `Agente/planes/plan-119A-7-agente-solido-bench-2026-09-11.md` — `119A-7` — auditoría del núcleo vs referencias + bench propio; F2 en curso, F3–F5 pendientes.
- `Agente/planes/plan-129A-9-configs-synara-2026-09-12.md` — `129A-9` — configuraciones Synara (inventario, decisión y réplica); F2 bloques 1–3 hechos, quedan F3 Perfil + bloques Archivados/Notificaciones/Avanzadas + nombre en barra/sidebar.
- `Agente/planes/plan-209A-1-consola-lateral-2026-09-20.md` — `209A-1` — consola lateral con visor en vivo + topes anti-fuga; F1/F2/F4 hechas, F3 parcial, F5 pendiente.
- `Agente/planes/plan-migracion-tauri-glib-2026-10-12.md` — `08AA-3` — migración Tauri a gtk4 para cerrar `glib 0.18.5`; activo en vigilancia (próxima revisión 2026-11-12).
