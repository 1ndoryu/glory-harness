# Plan 219A-4 — Consola propia del usuario (2026-09-21)

## Objetivo
La tab Consola deja de ser solo visor del agente: el usuario puede abrir sus
propias consolas reales (shell del sistema, línea a línea, sin PTY) y gestionar
todas (suyas + agente) desde la barra interna. Boceto aprobado por el usuario:

```text
+---------------------------------------------------------------+
| Consola                                        [+ Nueva]      |
+----------------------------------------+----------------------+
| $ python servidor.py                    | ● Mi shell        |
| * Running on http://127.0.0.1:5000     | ○ Agente: cargo   |
| $ _                                     | ○ Agente: pytest  |
+----------------------------------------+----------------------+
| > [escribo aquí, Enter = enviar]                          ✕  |
+---------------------------------------------------------------+
```

## Alcance / no alcance
- SÍ: botón `[+ Nueva]` (abre shell por defecto del SO), barra interna con
  todas marcando dueño (mía / agente), clic alterna la visible, `✕` cierra
  (mata) la activa, stdin existente para interactuar.
- SÍ: reutilizar la infra 209A-1/219A-3 (anillo, transcript, escribir, matar).
- NO: PTY (formato no-TTY honesto, como hasta ahora); comandos remotos fuera
  de la sesión loopback; terminal gráfica (vim/tui interactivos no funcionan).

## Fases
- F1 investigar: ruta de spawn del agente (`ejecutar_fondo_con_id`,
  `EjecutorCliente`, quién la llama) + shell por defecto por SO.
- F2 backend: endpoint crear-consola propia + campo dueño
  (`usuario`/`agente`) en la info de consola (core → cli/web).
- F3 transporte: comando Tauri + adaptador web + tipos (`realTipos.ts`).
- F4 UI: reescritura `panelConsola.ts` al layout del boceto (activa + barra
  + Nueva + cerrar + dueño); estilos en `consola.css`; mismo canon de
  botones (28×28 sin borde).
- F5 verificación: `type-check` + `build` + gate Sentinel + funcional en vivo
  (`:8799`: crear, escribir, leer, matar) + commit.

## Estado
- ACTIVO 21-09. Roadmap: entrada 219A-4.
- F1 HECHA 21-09: spawn del agente = `EjecutorComando::ejecutar(fondo)` →
  `ejecutar_fondo_con_id` (privado, 2 llamadas internas + tests) con
  `construir_comando` = `jaula::construir_directo` SIN shell + sandbox
  (niega shells `bash/cmd/powershell`, sintaxis shell, escalada).
  `lista()` = vivas (`vivas` map) + archivadas (`resultados` map).
  Sin campo dueño en ningún nivel.
- Decisión F1 (clave): la jaula protege del MODELO; el usuario en loopback
  es otro nivel de confianza (ya puede matar/escribir en cualquier consola).
  `ejecutar_propia` NO pasa por la jaula: `Command` directo al shell del SO
  (`cmd` en Windows, `sh` en Unix), solo en `EjecutorCliente` (default del
  trait = `NoSoportado`, mocks ni se enteran). NO se tocan las reglas de la
  jaula.
- Decisión F1 (dueño): enum `OrigenConsola { Agente, Usuario }` (default
  Agente) en `ConsolaViva` + `InfoConsola` (+ `TranscriptConsola` por
  coherencia); archivadas quedan Agente (la UI solo etiqueta vivas).
