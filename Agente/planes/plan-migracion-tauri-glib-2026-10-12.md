# Plan: migración Tauri para cerrar glib 0.18.5 (alerta Dependabot #1)

**Estado:** activo en vigilancia. F0 hecha: upstream sin gtk4 → F1b (sin migración hoy).
**Origen:** decisión de la usuaria, 2026-10-12: «Planificar migración Tauri». Enlazado desde `roadmap.md` (entrada 08AA-3).
**Siguiente paso:** revisión mensual de upstream (F1b), próxima el 2026-11-12.

## Objetivo

Eliminar `glib` 0.18.5 de `Cargo.lock` (RUSTSEC-2024-0429 / GHSA-wrw7-89jp-8q8g, severidad medium; versión fija glib ≥0.20.0) migrando la cadena GTK3 de Tauri a GTK4. Si upstream no ofrece esa migración, documentar la espera.

## Diagnóstico (verificado 2026-10-12)

- Único consumidor del workspace: `glory-harness-desktop` (`desktop/src-tauri/Cargo.toml:14`, `tauri = "2"`).
- Cadena: `tauri 2.11.5` → `gtk 0.18.2` → `glib 0.18.5` (`Cargo.lock:1744`, `:1886`, `:4899`). Además `tauri` → `webkit2gtk 2.0.2` → glib; `tauri` → `tauri-runtime-wry 2.11.4` → `tao 0.35.3` / `wry 0.55.1` → gtk (`Cargo.lock:6687`); `tauri` → `tray-icon 0.24.2` → `libappindicator 0.9.0` → glib.
- Solo se compila en Linux/BSD (`tauri-2.11.5/Cargo.toml:335,339`). Dependabot lee `Cargo.lock` sin filtrar target, por eso la alerta aparece también en Windows.
- glib ≥0.20 pertenece al stack gtk4-rs (`gtk4 0.9.6` exige `glib 0.20`). `gtk 0.18` (gtk3) no puede subir de glib.
- Upstream, según docs.rs: `wry 0.57.0` y `tauri-runtime-wry 2.12.1` siguen con `gtk "0.18"`. `tauri 2.12.3` (publicado 2026-10-10) también sigue con gtk 0.18 (verificado en F0).
- Conclusión: no es un bump de versión. Depende de que upstream migre a gtk4 o de un fork.

## Alcance

- **Dentro:** F0 (verificación upstream), decisión según resultado, migración si upstream existe, verificación del build Linux.
- **Fuera:** Tauri v1, reescritura de la capa de ventana, silenciar o descartar la alerta en GitHub (queda visible hasta F1a, según la indicación de la usuaria de no silenciar alertas de forma permanente).

## Fases

### F0: verificar upstream (sin cambios en el repo)

- Comprobar en docs.rs / crates.io el `Cargo.toml` de `tauri 2.12.3`, `tauri-runtime-wry` y `wry` (última versión estable) buscando `gtk` 0.18 o gtk4.
- Resultado (2026-10-12, crates.io `/api/v1/crates/<crate>/<version>/dependencies`): `tauri 2.12.3` → `gtk ^0.18` (Linux/BSD); `tauri-runtime-wry 2.12.1` → `gtk ^0.18`; `wry 0.57.0` → `gtk ^0.18`. Ningún upstream publicado usa gtk4.
- **Decisión: B** (ver F1b). Sin migración posible hoy.
- Verificación: `gh api repos/1ndoryu/glory-harness/dependabot/alerts` sigue mostrando la alerta #1 abierta.

### F1a: si upstream ya usa gtk4 (decisión A)

- Rama `migracion-tauri-gtk4`. Subir `tauri`, `tauri-build` y plugins a la versión que lo trae.
- Verificación: `cargo tree -i glib` muestra ≥0.20; build Linux de `glory-harness-desktop` OK (requiere entorno Linux o contenedor: Windows no compila ese target); smoke de ventana en Linux si es viable. Registrar la limitación si no lo es.
- Integrar en `main` tras verificar (AGENTS §5). Alerta #1 cerrada por Dependabot tras el push.

### F1b: si upstream no ha migrado (decisión B)

- **Decidido:** esperar upstream, sin fork. Motivo: un fork de `tauri-runtime-wry` con gtk4 es coste alto y riesgo sobre la ventana de escritorio, y la alerta es de severidad medium con superficie solo Linux/BSD.
- Mantener la alerta visible. Registrar en `roadmap.md` la condición de reapertura: publicación de `wry` o `tauri-runtime-wry` con gtk4.
- Revisar mensualmente con `gh api repos/1ndoryu/glory-harness/dependabot/alerts` y `docs.rs`.
- Pendiente de hecho externo: publicación upstream de gtk4.

### F2: cierre

- Solo F1a se cierra con evidencia: entrada en `Agente/completados/tareas-YYYY-MM-DD.md`, plan a `Agente/planes/completados/` y roadmap limpio.
- Con F1b el plan queda activo como vigilancia.

## Definition of Done

- F1a: alerta #1 cerrada en GitHub, `cargo tree -i glib` ≥0.20, build Linux verificado o limitación registrada, entrada en completados.
- F1b: alerta visible, condición de reapertura en roadmap, primera revisión programada en el roadmap.
