# Plan 129A-6 — El flag de espera se pierde al reconstruir el runtime

Fecha: 2026-09-12. Estado: completado. Roadmap: `129A-6`.

## Evidencia (verificada, no hipótesis)

- Turno `8330cf44` (conversación `df331860…`, 11:26:31Z, binario CON 129A-5):
  `tool_start file_write` → `peticion_aprobacion f640a30a` →
  `requiere_aprobacion` → `tool_result(false)` en el MISMO segundo →
  `done` → `respuesta_aprobacion_sin_espera {aprobar}` 1 s después.
  Camino clásico sin espera alguna, con el flag supuestamente activo.
- Descartados con evidencia: versión mixta (el exe contiene el string nuevo);
  doble runtime (`preparar_turno_con_modo` = `Arc::clone`, `sesion.rs:554`);
  `tx.closed` (listener `escuchando` único y estable, `turnoReal.ts:155`);
  petición pendiente envenenada (auditoría: 2 peticiones `file_write` hoy,
  ambas con respuesta; veredicto = `Preguntar` limpio).
- Agujero confirmado en código: `SesionComun::reconstruir`
  (`cli/src/servicio/sesion.rs:330`) sustituye `self.runtime` por uno fresco
  cuyo registry trae `espera_en_turno=false`. Lo dispara `reconfigurar`
  (modelo/modo/razonamiento/proveedor, comando de hooks `sesion.rs:249`)
  y `cambiar_workspace`. Tras cualquiera, todos los turnos siguientes van
  en clásico sin que nada lo reporte.

## Fases verificables

- F1: `reconstruir` preserva el flag (lee antes, aplica después; neutro:
  conserva el valor que hubiera, no impone política). Test
  `reconstruir_conserva_espera_en_turno` (abrir en memoria, activar,
  `reconfigurar(None×4)`, afirmar).
- F2: `log::anotar` con el estado del flag al abrir sesión (`main.rs`) y
  tras `reconfigurar_sesion` (`sesion.rs`): la pregunta "¿está el flag
  activo AHORA?" se responde en el log de proceso.
- F3: `eprintln!` en las ramas degradadas del `select!` de
  `esperar_aprobacion_en_turno` (SSE cerrado, petición supersedida): solo
  se alcanzan con el flag activo, nunca es spam del camino clásico.
- F4: gate `sentinel check 129A-6` + commit + completada + releer roadmap.

## DoD

- Cambiar de modelo/modo/razonamiento o tocar hooks ya no desactiva la
  espera; el log dice el estado del flag en apertura y reconfiguración;
  gate PASS asociado al commit. El usuario reinicia `tauri dev` (binario
  nuevo) y repite el test de `df33`: el turno debe pausar y continuar.
