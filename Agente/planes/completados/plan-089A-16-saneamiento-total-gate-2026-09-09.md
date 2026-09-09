# Plan 089A-16 — Saneamiento total del gate (cero deuda tras 089A-15)

> ID roadmap: **089A-16** · Fecha: 2026-09-09 · Estado: HECHO (gate full PASS 0/0/0, 09-09)
> Origen: 089A-13 reveló 14 errores + 116 warnings + 7 info en el frontend TS;
> el gate además arrastra warnings Rust preexistentes (serie 318A + 16 en
> `navegador/reflejo.rs`). Objetivo final: gate full **PASS 0/0/0** con la
> etapa `coverage` de 089A-15 en verde.

## 1. Objetivo

Dejar el árbol en verde total: cero errores, cero warnings, cero info
pendiente en Sentinel/VarSense, con cobertura completa. Es el plan que devuelve
el "cierre con evidencia" a todas las tareas futuras.

## 2. Alcance / no alcance

- SÍ: todo hallazgo del reporte 089A-13 (`sentinel.json` por fichero) +
  lo nuevo que revele la etapa `coverage` de 089A-15 + warnings Rust vigentes.
- NO: nuevas features; cambios visuales (sin alterar UI salvo que un fix lo
  exija); reeditar la skill global; tocar `tools/sentinel`; reabrir 089A-5
  (historial atrás/adelante) salvo que un fix lo pida.

## 3. Precondición y coordinación (bloqueante)

1. **089A-15 completo**: la etapa `coverage` debe estar verde antes de empezar;
   si revela fuentes nuevas (p. ej. `tsx/js`), sus hallazgos entran a este plan.
2. **Releer el reporte**: regenerar el gate al arrancar (el otro agente integra
   a diario; los hallazgos de hoy pueden haber cambiado). Nunca sanear sobre un
   reporte viejo.
3. **Reparto con el otro agente**: sus 11 ficheros a medias tocan el mismo TS
   (`iconos.ts`, `panelFiles.ts`, `tipos.ts`, `files.css`, `real.ts`, `api.ts`,
   más `main.rs`/`filesystem.rs`/`navegador/mod.rs` en Tauri). Regla: no editar
   ficheros con trabajo suyo sin integrar; o reparto explícito por fichero
   antes de empezar. Si integra primero, este plan arranca sobre su código.

## 4. Fase 1 — Errores TS (bloquean el árbol; primero)

1. **XSS `innerHTML` ×10** (los únicos con riesgo de seguridad real):
   `entrada.ts:584,588`, `mensajes.ts:73,178,216,226,351`, `iconos.ts:89`,
   `panelMeta.ts:119`, `util/dom.ts:14`. Migración a `textContent` + construcción
   DOM explícita; donde haya HTML legítimo (iconos SVG estáticos), mover a
   plantilla constante auditada o sanitizador mínimo con justificación en el
   diff. Verificación: buscar `innerHTML` debe dar 0 en `desktop/ui/src`.
2. **`catch` vacíos ×3** (`panelNavegador.ts:423,491,521`): logging explícito
   (vía el logger del proyecto, no `console` suelto — ver fase 2.5) + estado de
   error visible si el fallo afecta al usuario. Cero `catch {}` silenciosos.
3. **`main.ts` 1182 ef. (límite 300, nivel-3)**: partir por responsabilidad
   (orquestación vs. paneles vs. adaptadores). Límite duro: ningún fichero
   resultante >300. Misma receta para los otros 7 ficheros >300 de la fase 2.4.
4. Gate intermedio tras la fase: deben quedar 0 errores aunque sigan warnings.

## 5. Fase 2 — Warnings TS (por lotes, de mecánico a lógica)

1. **`barras-decorativas` ×62** (mecánico, lote único): eliminar barras de
   comentarios; conservar el "por qué" en comentarios normales. Riesgo ~0.
2. **`window-reference` ×22 + `dom-access` ×19**: todo acceso a `window`/DOM
   fuera de la capa plataforma debe pasar por `esEntornoTauri()`/adaptador.
   Es el lote con más riesgo de romper web-vs-app: verificar en ambos modos
   (navegador + `tauri dev`) lo tocado.
3. **`limite-lineas` ×8** (resto tras `main.ts`): misma receta que §4.3.
4. **Interfaces grandes ×3**: partir tipos por dominio en `dominio/tipos.ts`
   (coordinar: el otro agente está tocando ese fichero).
5. **`console` ×1 + `dir` ×1**: migrar al logger; prohibido `console` suelto.
6. **Info ×7** (3 interfaces + 4 `todo-pendiente`): cada `todo` se resuelve o se
   convierte en tarea del roadmap con ID; no queda ningún `todo` sin dueño.

## 6. Fase 3 — Warnings Rust (preexistentes, no bloquean el árbol TS)

- 16 warnings `rustc` en `navegador/reflejo.rs` + serie 318A
  (`ejecutar_turno` 232, `enviar_turno` 172-174, `abrir_sesion_interna`
  118-126, `bucle_ui` 120, `dibujar` 117, `ejecutar_request_stream` 112,
  `despachar` 113; servicios 792-1111 nivel-2). Son refactors de función
  larga, no riesgo funcional.
- **Válvula de descope**: si un servicio nivel-2 (792-1111 líneas) exige
  rediseño, se extrae a tarea propia con ID en vez de bloquear este plan. El
  plan cierra con el gate en 0/0/0 o con la tarea hija creada y justificada.

## 7. Verificación y cierre

- Tras cada fase: `tsc --noEmit` + `vite build` + `cargo test`/`clippy` en lo
  tocado (compilar no basta: verificación funcional en navegador real y, para
  el lote 2.2, también `tauri dev`).
- Gate final full: PASS 0 errores / 0 warnings / 0 info + etapa `coverage`
  verde. Reporte en `.quality-reports/check/089A-16/`.
- Docs: entrada en `Agente/completados/tareas-*.md` (ID, ficheros, evidencia,
  decisiones), archivar este plan a `Agente/planes/completados/`, commit por
  fase con mensaje `{id}: descripcion`.
- Definition of Done: gate full 0/0/0 + `coverage` verde + commit + roadmap
  actualizado (089A-16 fuera, hijas creadas si aplica).

## 8. Riesgos

- **Colisión con el otro agente** (mismo TS/Tauri): mitigación en §3.3; si
  integra a mitad de fase, rebase funcional + repetir gate de la fase.
- Lote 2.2 puede romper modo web o app: verificación dual obligatoria.
- `main.ts` y servicios Rust grandes pueden esconder acoplamientos: partir por
  seams existentes (módulos/ports), sin rediseño encubierto.
- Coste: fases 1-2 ~1-2 sesiones; fase 3 según descope. `C:\tmp` bajo 7 GB
  antes de compilar (vigilar `glory-target`, hoy ~2.3 GB libres).
