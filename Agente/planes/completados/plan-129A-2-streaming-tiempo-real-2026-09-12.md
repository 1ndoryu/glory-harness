# Plan 129A-2 — Streaming en tiempo real (texto + razonamiento + métricas)

> Estado: completado 12-09 · Fecha: 2026-09-12 · Rama: `main`
> Causa raíz (verificada 12-09): el núcleo no emite nada en vivo. `on_token`
> (`turno/mod.rs:340`) solo acumula; el único `Token` sale completo al final
> (`:573`); `reasoning_content` se acumula en silencio (`stream.rs:66-71`).
> El transporte (`chat/turno.rs:185`) y el render (`aplicarEventos.ts:54`)
> ya son en vivo: solo les faltan eventos.

## F1 — Backend en vivo (core)

1. `Token` por delta con throttle (~40 ms o ~240 chars) vía `try_send` en el
   `on_token` de `turno/mod.rs` (+ wrap-up `:610`). Sin `await` (closure sync).
2. Nuevo evento `RazonamientoDelta { texto }`: segundo callback en
   `hojear_stream` → plomero por `red.rs` → `llm_llamada` → `tx`.
   Contrato: `contrato/evento.rs` + `desktop/ui/.../realTipos.ts`.
3. Sin duplicado: si ya se stremeó en vivo, `gestionar_respuesta_final` no
   reemite el `Token` completo (flag desde el bloque de llamada).
4. Revisar consumidores de `Token`: `cron.rs:152`, `subagente.rs`,
   `cli/ui/turno.rs:75` (el REPL gana vivo gratis), mocks del puerto LLM.
5. `red.rs:149` (vía no-stream): un solo `on_token` con todo → cuenta como
   "ya stremeado" para el flag de F1.3.

## F2 — Front en vivo (desktop + CLI)

6. `aplicarEventos.ts`: caso `razonamiento_delta` → crea `RazonamientoVivo`
   ("Razonando…" + spinner + contador) al primer delta, append siguientes;
   `razonamiento` (completo) → `terminar(meta)`; sin deltas previos →
   summary cerrado como hoy (129A-1 intacto).
7. Contador de tokens de razonamiento en el `meta` del summary, en vivo
   (conteo del front sobre lo anexado, heurística chars/4 como el estimador
   del core; al cerrar se fija el total + segundos).
8. Medidor tok/s en el `pie-turno` (`mensajesNucleo.ts:131`): al cierre,
   `tokens_complecion / segundos de stream` (Usage real + t0 de `montar`);
   por llamada, no por turno.
9. CSS: efecto de actividad del bloque vivo (shimmer/puntos + estado stall)
   en `estilos/`, con tokens de `variables.css` (sin literales en componentes).
10. CLI: `chat.rs`/`tui` manejan `RazonamientoDelta` (línea "Razonando… N tok");
    el `Token` en vivo ya fluye por `on_evento`.

## F3 — Verificación (completado 12-09)

11. `tsc` EXIT 0 + `vite build` OK (113 módulos, 176.69 kB).
    `sentinel check 129A-2` PASS final: coverage/sccache/sentinel/rust PASS,
    453 tests ok (3 nuevos de `EmisorVivo`: umbral, intervalo, rebuffer),
    0 errores; INFOs preexistentes (`sidebar.ts` ISP, `context.rs:972` todo).
12. Funcional: contrato `razonamiento_delta` testeado en Rust; vivo extremo a
    extremo pendiente de `tauri dev` con modelo de razonamiento (ver
    completada). Límite conocido: CLI/TUI resume el razonamiento (stderr /
    línea de estado), no pinta bloque vivo.

## No alcance

Sin cambios de severidades, umbrales, persistencia (las filas `reasoning` de
129A-1 siguen igual), ni backdoor de `cargo` directo (todo vía gate).
