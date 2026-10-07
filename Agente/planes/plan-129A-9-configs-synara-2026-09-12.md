# Plan 129A-9 — Configuraciones Synara: inventario, decisión y réplica

Fecha: 2026-09-12. Estado: activo (F2 en curso 07-10, bloques Perfil + Proveedores).
Roadmap: `129A-9`. El usuario quiere replicar casi todas.

## Inventario verificado (fuente: `synara/apps/web/src/components/settings/`)

1. `ProfileSettingsPanel.tsx` — perfil de usuario.
2. `ProvidersSettingsPanel.tsx` — proveedores/instalación (+
   `isProviderInstallSettingsDirty`, `createProviderInstallResetPatch`).
3. `ProviderUsageSettingsPanel.tsx` — uso por proveedor.
4. `ModelsSettingsPanel.tsx` — modelos (+ `validateCustomModelInput`,
   modelos personalizados).
5. `SkillsSettingsPanel.tsx` + `skillsSettingsModel.ts` — skills agrupadas
   por origen/proveedor.
6. `ExternalMcpSettingsPanel.tsx` + `externalMcpSetup.ts` — MCP externos (+
   generador de prompt de setup y ejemplo por proyecto).
7. `ConversationStorageSettingsPanels.tsx` — `WorktreesSettingsPanel`,
   `ArchivedSettingsPanel` (almacenamiento de conversaciones).
8. `DesktopSettingsPanels.tsx` (+ `.browser.tsx`) — `NotificationsSettingsPanel`,
   `AppSnapSettingsPanel`, `AppSnapShortcutControl`, `AppIconPicker`.
9. `KeyboardShortcutsSettingsPanel.tsx` — atajos de teclado.
10. `AdvancedSettingsPanel.tsx` (+ `.browser.tsx`) — avanzadas.
11. `ThemeModePicker.tsx` — modo de tema.
12. Primitivas (`SettingsPanelPrimitives.tsx`, `SettingControls.tsx`,
    `DebouncedSettingTextInput.tsx`): `SettingsCard/Section/Row/ListRow`,
    `Select/SegmentedControl`, `SettingResetButton`, restore-signal.
- Agregador: `synara/apps/web/src/routes/_chat.settings.tsx`. Casa GH:
  `ajustes-pagina` (`desktop/ui/src/componentes/modal.ts:60`,
  `estilos/ajustes.css`), opciones en `dominio/opciones.ts`.

## Decisión del usuario (12-09, registrada — desbloquea F2+)

- Replicar TODO lo listado: bloque proveedores completo (Proveedores,
  Modelos personalizados, Uso por proveedor, Skills, MCP externos),
  almacenamiento (Worktrees + Archivados), escritorio
  (Notificaciones, AppSnap + icono, Atajos de teclado), más Perfil y Tema.
- Orden: empezar por Perfil y tema.

## Fases verificables

- F1 (sin código): tabla panel→opciones→equivalente GH→propuesta
  (ver §F1 abajo; decisión del usuario registrada 12-09, F2 desbloqueado).
- F2+: réplica por bloques en `ajustes-pagina` (un bloque = una sección
  Synara; commit + gate por bloque, no todo junto). Límite 300 líneas por
  archivo de front vigente. Orden: Perfil y tema primero (tema ya existe:
  129A-12).
- F3: smoke `tauri dev` por bloque (persistencia real de cada ajuste).

## F1 — Tabla panel Synara → equivalente GH → propuesta (07-10, verificada
con grep sobre `desktop/ui/src`)

| # | Panel Synara | Equivalente GH | Propuesta |
|---|--------------|----------------|-----------|
| 1 | Perfil (nombre, handle, avatar, stats, heatmap, share) | ninguno | **adaptar**: `nombre_perfil` + `handle_perfil` editables y persistidos (claves config como `tema`); sin stats/heatmap/share (GH no tiene BD de actividad); sin avatar (sin UI de avatar). Mostrar el nombre en barra/sidebar = bloque posterior |
| 2 | Proveedores (+ install/dirty/reset) | `OPCIONES_MODELO` + allowlist 6 proveedores + claves env | HECHO bloque 2 (lista ESTADO read-only; sin installs) |
| 3 | Uso por proveedor | ninguno (sin telemetría de uso) | no (sin fuente de datos) |
| 4 | Modelos (+ personalizados) | `selectorModelo` + `OPCIONES_MODELO` | replicar (bloque posterior) |
| 5 | Skills por origen/proveedor | toggle `skills` (`OPCIONES_PERMISOS`) + panel Memorias | adaptar (bloque posterior) |
| 6 | MCP externos (+ setup prompt) | ninguno (`mcp` = 0 matches en el front) | no (sin backend MCP) |
| 7 | Worktrees + Archivados | archivar/desarchivar en sidebar (sin sección en Ajustes); sin worktrees | adaptar solo Archivados (bloque posterior); Worktrees no |
| 8 | Notificaciones, AppSnap, icono app | permiso toast (`notificacionSistema`, arranque 12-09); sin AppSnap/icono | adaptar solo Notificaciones (bloque posterior) |
| 9 | Atajos de teclado | ninguno (sin sistema de atajos) | no (sin base donde enganchar) |
| 10 | Avanzadas | `OPCIONES_CONTEXTO` (hook, ventana, preferencias) | adaptar (bloque posterior) |
| 11 | Tema | `OPCIONES_APARIENCIA` → `tema` claro/oscuro (129A-12) | HECHO (129A-12) |
| 12 | Primitivas (cards, selects, reset, restore-signal) | `componentes/formulario.ts` + esquema `dominio/opciones.ts` | reutilizar (sin código nuevo) |

## F2 bloque 1 — Perfil (07-10, HECHO; F3 smoke vivo pendiente)

`OPCIONES_PERFIL` (`nombre_perfil` + `handle_perfil`, texto con
placeholder/nota) primera en `FORMULARIO_CONFIGURACION` (la página Ajustes
abre en Perfil); `vistaModal` persiste ambas claves (misma allowlist que
`tema`) y las restaura vía `asignarValor`; `arranque` las lee y las pasa en
`SesionGuardadaVista`. Cero cambios Rust: Tauri usa `config_guardar`
genérico (SQLite) y web cae a localStorage (igual que `tema`).
Evidencia: `tsc` EXIT 0; gate `129A-9 --full` PASS (rust 530 ok);
`vite build` OK con `nombre_perfil`/`Nombre visible` en el bundle servido;
render genérico verificado (`formulario.ts` caso `texto` + índice
`formularioDeOpcion` en `modal.ts`). F3 (teclear→recargar→restaurado en
navegador/Tauri vivo) pendiente: esta sesión no tiene navegador.
Mostrar el nombre en barra/sidebar = bloque posterior.

## F2 bloque 2 — Proveedores (07-10, HECHO; F3 smoke vivo pendiente)

Réplica adaptada de `ProvidersSettingsPanel`: sección `Proveedores` tras
Perfil con lista ESTADO read-only (sin install-settings, sin orden
arrastrable, sin checks de update: GH no gestiona installs). Cruza catálogo
`PROVEEDORES` con vivo `ProveedorInfo{id,modelos,claves}` vía
`adaptador.sesion.proveedores()` (Tauri `proveedores_disponibles` / web
`GET providers` con auth, código preexistente sin tocar): chip `disponible ·
N clave(s)`/`sin claves`, `en uso` sobre el modelo actual, botón `recargar`,
nota de que las claves viven en `~/.glory-harness.env` (no se editan aquí).
Gotchas: colisión `Duplicate identifier 'proveedores'` (import + local) →
local renombrado `proveedoresPanel`; el vivo trae proveedores fuera del
catálogo (cerebras, groq) → el conteo solo cubre filas visibles (evita
"6 de 4"). Cero cambios Rust.
Evidencia: `tsc` EXIT 0; `vite build` OK (bundle con `proveedor-fila`/
`proveedores-chip`); gate `129A-9 --full` PASS (9 archivos; warnings solo
prosa ajena en Rust no tocado); backend 8799: `POST /api/v1/session` ok con
6 proveedores con claves>0. F3 (abrir Ajustes→Proveedores en vivo)
pendiente: esta sesión no tiene navegador.

## DoD

- Decisión registrada; cada bloque replicado guarda/carga de verdad y pasa
  gate; ningún ajuste existente pierde su valor (migración si cambia la
  clave).
