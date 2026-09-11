# Prevención: claves de `invoke` Tauri siempre en camelCase

> Fecha: 2026-09-11 · Origen: 119A-5 F1 (toast `meta_leer missing required key conversacionId`).

## Caso mínimo

- Front enviaba `{ conversacion_id }` a `meta_leer` (`transporteTauri.ts`);
  Tauri 2 espera `conversacionId` y rechaza con `missing required key`.
- Las claves **opcionales** en snake (`panel_id`, `solo_lectura`) no dan
  error: Tauri las ignora y caen al default (`None`/principal). Síntoma
  tramposo: «funciona pero en el panel equivocado» o «el `/meta`
  solo-lectura no tiene efecto».

## Regla

- Todo `invoke` multi-palabra usa **camelCase** en JS (`panelId`,
  `soloLectura`, `turnoId`, `conversacionId`, `rutaRelativa`,
  `hastaMensajeId`); Rust sigue en snake (`panel_id`, …). Tauri traduce
  solo en esa dirección.
- Prueba viva en el repo: `workspace_leer_archivo` con `{ rutaRelativa }`
  funciona en la ventana real; `navegador_abrir` con `{ posX, posY }`
  también.
- Los tipos internos del front (`ComandoMetaVisible`,
  `AgenteEvento`) conservan snake: la traducción vive **solo** en las
  claves del objeto que se pasa a `invoke`, nunca en los accesos al tipo.

## Detección esperada

- `tsc` NO lo detecta (las claves son un objeto libre para `invoke`).
- Señal: `invalid args '<camel>' for command '<cmd>': missing required
  key <camel>` (clave requerida) o comportamiento «en el sitio
  equivocado» sin error (clave opcional).
- Sonda futura (pendiente): cruzar claves de `invoke` en
  `transporteTauri.ts` contra parámetros de `#[tauri::command]` en
  `desktop/src-tauri/src` en el gate.
