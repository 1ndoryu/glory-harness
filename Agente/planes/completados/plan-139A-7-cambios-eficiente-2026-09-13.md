# Plan 139A-7 — Cambios eficiente (2026-09-13)

## Objetivo
Cambiar de conversación (o recibir ráfagas de triggers) no re-analiza git:
cero consultas git al cambiar de conv, una sola ejecución ante ráfagas y cero
repintado cuando nada cambió. Medido con contadores antes/después.

## Causa raíz
`recargar()` = pipeline monolítico (repos → N estados → listar → repintado
total). Lo disparan: fin de turno, cambio de conversación, abrir la tab,
cambio de área, escrituras vivas (debounce 800ms), rechazos y revelados —
sin coalescar, sin caché y sin comparar. El estado git no depende de la
conversación, pero A→B→A lo re-escanea todo.

## Fases
- **F0 Medición**: fixture `C:\tmp\gh-eficiencia-test.ts` (transportes
  contadores; escenario: carga + ráfaga ×3 + A→B→A). Baseline: ~6 repos,
  ~12 estados, ~6 listar. Driver compatible antes/después
  (`recargarVault?.() ?? recargar()`; `resumen` solo si el panel lo usa).
- **F1 Front** (`panelCambios.ts`, nuevo `cambiosRepos.ts`, nuevo
  `util/vueloUnico.ts`, `main.ts`):
  - Extraer bloque multi-repo (panelPara/pintado/revelarAhora) a
    `cambiosRepos.ts` (límite 300 líneas/archivo).
  - `consultarGit()`: usa `git.resumen?.()` (batch) con fallback al fan-out.
  - Single-flight con trailing (`vueloUnico`): total arrastra a vault.
  - `recargarVault()`: solo listar+pintar vault (cero git); `main.ts`
    `alCambiarConversacion` → `recargarVault()`. Resto sigue full.
  - Pintar-solo-si-cambió: firma global git (JSON estados) y firma vault
    (conv + rutas/turnos + sello vivos + conv pintada); el revelado pendiente
    se consume solo en full y se descarta al cambiar de conv.
- **F2 Backend** (`git.rs` + `main.rs` + `transporteTauri.ts` + `realTipos.ts`):
  comando `workspace_git_resumen` (descubrir + estado por repo en 1 IPC;
  secuencial v1, sin caché backend: sin watcher no hay invalidación fiable;
  la caché vive en el front + botón Actualizar). Fallback intacto para web.
- **F3 Cierre**: `tsc` EXIT 0, fixture eficiencia (objetivo: resumen 2,
  listar 4, repos/estado 0) + 24 checks 139A-6 en verde, `sentinel check
  139A-7` PASS, roadmap + completadas. Sin commit (árbol ahead 1 con cambios
  sin commitear por decisión del usuario).

## DoD
Contadores después ≤ (resumen 2, listar 4, repos 0, estado 0); 24/24 139A-6;
gate PASS; sin regresión visual (secciones planas, colapso, revelado).
