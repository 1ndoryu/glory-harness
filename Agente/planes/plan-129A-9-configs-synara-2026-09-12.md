# Plan 129A-9 — Configuraciones Synara: inventario, decisión y réplica

Fecha: 2026-09-12. Estado: activo (BLOQUEADO en F1 hasta decisión del
usuario). Roadmap: `129A-9`. El usuario quiere replicar casi todas.

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
  (replicar / adaptar / no). **Requiere decisión explícita del usuario;
  bloquea F2+.** Registrar la decisión en este plan.
- F2+: réplica por bloques en `ajustes-pagina` (un bloque = una sección
  Synara; commit + gate por bloque, no todo junto). Límite 300 líneas por
  archivo de front vigente.
- F3: smoke `tauri dev` por bloque (persistencia real de cada ajuste).

## DoD

- Decisión registrada; cada bloque replicado guarda/carga de verdad y pasa
  gate; ningún ajuste existente pierde su valor (migración si cambia la
  clave).
