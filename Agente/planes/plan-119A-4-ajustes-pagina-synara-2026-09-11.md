# Plan 119A-4 — Ajustes estilo Synara: página completa + todas las opciones aplicables

> ID: **119A-4** · Fecha: 2026-09-11 · Estado: **activo (planificado, sin implementar)**.
> Origen: captura del usuario con la página de ajustes de Synara (nav por
> secciones + buscador + «Back to app»). Pide: (a) listar todas las
> configuraciones, (b) aplicar en GH todas las que se puedan, (c) que los
> ajustes dejen de ser un modal y se vean como en Synara.

## 1. Inventario Synara → GH (verificado 11-09)

Fuente: `area-trabajo/synara` (`settingsNavigation.ts:6-179`,
`_chat.settings.tsx`, `components/settings/*`). GH actual:
`desktop/ui/src/dominio/opciones.ts` (Modelo, Ejecución, Permisos,
Apariencia, Contexto, Memorias).

| Sección Synara | Veredicto para GH |
|---|---|
| General (provider por defecto, órdenes, toggles de secciones/panel Environment) | **Parcial**: orden de proyectos/hilos (sale de 119A-3) + confirmaciones de borrado/archivado (alimentan 119A-2 F4) + idioma (ya existe). El resto es de Synara (Environment, Studio, automatizaciones): no aplica. |
| Profile (stats, rachas, heatmap) | **No**: GH no tiene cuentas ni histórico agregado. Lo medible en local (tokens/tiempo) va a Uso (F3). |
| Appearance (tema, densidad, anchos, fuentes, formato hora) | **Parcial**: modo oscuro (ya existe) + tamaño de fuente base + formato de hora (si los mensajes muestran hora; verificar en F2, si no se cae). Densidad/anchos/terminal: la identidad GH es fija, no entran. |
| Notifications (toasts, notificaciones SO) | **Parcial**: toast al terminar el turno / pedir aprobación (hay `toastGlobal`, es un booleano). Notificaciones de SO: sin plugin de notificación en Tauri → se cae. |
| Chat behavior (streaming, confirmaciones, follow-up, diffs) | **Sí**: streaming on/off, confirmación antes de borrar/archivar, wrap del diff y colores del diff (el visor Git existe, 089A-17), cola vs redirección de mensajes durante el turno (verificar viabilidad en F2; si el turno M1 no admite cola, se cae con razón). |
| Keybindings | **Sí, fase tardía (F4)**: GH no tiene sistema de atajos; es un subsistema nuevo (captura, persistencia, edición). Alcance acotado: acciones principales (nueva conversación, enviar, cancelar turno, toggles). |
| Usage & limits | **Parcial (F3)**: solo lo medible en local (turno actual + acumulado de sesión desde el panel meta); cuotas de proveedor: no hay API → no se inventan. |
| AppSnap | **No**: captura de ventanas macOS, sin equivalente. |
| MCP connections | **No**: GH no tiene agentes externos ni MCP. |
| Agent providers | **Como está**: proveedor/modelo + claves ya existen; toggles por proveedor solo si el core los soporta (verificar en F3; si no, no entra). |
| Models & writing | **Como está**: razonamiento, temperatura, tokens y estilo ya existen; modelo de escritura git y slugs: GH no los usa → no entran. |
| Agent skills | **Sí (F3)**: el permiso global existe y el core tiene catálogo (`comando_expandir`); toggles por skill si el catálogo es listable (verificar en F3). |
| Managed worktrees | **No**: los workspaces GH no son worktrees git. |
| System tools | **Parcial (F3)**: versión visible + «reparar estado» (reindexar `content_search`, que sí existe) + reparar índices. Sin sesión que revocar. |
| Archived threads | **Sí (F3)**: vista de gestión (restaurar/borrar) sobre las archivadas de la sidebar; si falta `desarchivar` en backend, es un comando pequeño (verificar en F3). |

## 2. Migración modal → página (F1, sin opciones nuevas)

**Reto 11-09 (verificado contra el código):** el plan original pedía
`vistaAjustes.ts` nueva + retirar `modal.ts`. Se corrige: `modal.ts` se
**reescribe in situ manteniendo intacta la interfaz `ModalConfiguracion`**
(`raiz/abrir/cerrar/asignarValor/setModelo`), consumida por
`orquestador/vistaModal.ts`, `orquestador/crearPanel.ts` y `main.ts`
(`modal.raiz`, `abrirConfig → modal.abrir()`). Solo cambia el chrome a
página completa; el cableado no se toca. `modalProyecto.ts` es
autocontenido y queda intacto (pero usa las clases base
`.modal-fondo/.modal`: `modal.css` conserva solo esa base compartida).
El reagrupado de secciones (Personal/Proveedor/Sistema) se **difiere a
F2**: F1 conserva las 6 secciones actuales (ya son un nav estilo Synara)
para no tocar las ramas `seccion.id === 'modelo'/'memorias'` ni el
presiembra; el DoD «todas las opciones actuales presentes» queda
trivial. El buscador filtra por DOM vía `dataset.opcion` (cada
fila/control lo lleva) sin tocar `formulario.ts`; índice construido desde
`FORMULARIO_CONFIGURACION` (etiqueta + nota + id + grupo + sección,
normalizado sin tildes).

- Página `componentes/modal.ts` (misma interfaz): ocupa toda el área de
  la app (overlay a pantalla completa, fondo sólido `var(--fondo)`, no
  caja de diálogo), con cabecera «← Volver a la app», buscador y nav
  lateral por secciones; el contenido reutiliza los renderizadores de
  `componentes/formulario.ts` y el panel custom de `memorias.ts` sin
  cambios. Sin backdrop que cierre al clicar fuera; Escape limpia la
  búsqueda o vuelve a la app. Al volver se desmonta la vista (página
  oculta + búsqueda limpia, sin estado fantasma).
- Verificación F1: abrir desde el mismo botón que hoy abre el modal;
  todas las opciones actuales presentes y editables; buscar filtra
  (incluye la sección «Memorias» sin grupos y el panel Modelo con el
  control reemplazado por el selector); volver devuelve a la app;
  `tsc` + `vite build` OK. CSS: `modal.css` conserva solo la base
  `.modal-fondo/.modal` (compartida con `modalProyecto`); la página y las
  reglas de formulario viven en `estilos/ajustes.css` nuevo (import en
  `index.css`); `tema.css` suma `ajustes-*` al modo oscuro y retira
  `config-*` (muertos).

## 3. Fases de opciones nuevas

- **F2 — Lote sin backend (solo config persistida):** confirmaciones
  (borrar/archivar hilo → las usa 119A-2 F4), streaming on/off, toast de
  fin de turno, tamaño de fuente base, formato de hora (si aplica),
  wrap y colores del diff, órdenes de 119A-3 en General, cola de
  mensajes (si viable). Verificación por opción en ventana real.
- **F3 — Lote con backend:** Uso (medición local), skills por skill,
  Sistema (versión + reindexar), Archivadas (gestión + `desarchivar` si
  falta), toggles de proveedor (si el core los soporta). Cada opción con
  su comando y su prueba; lo que no se verifique se cae de la fase con
  razón escrita, no se deja a medias.
- **F4 — Atajos de teclado:** subsistema acotado (acciones principales,
  captura y edición simple, persistencia). Solo tras F1–F3.

## 4. Reglas

Esquema declarativo (`opciones.ts`) para todo lo que sea formulario (sin
excepción: añadir opción = añadir entrada); iconos 100% Lucide; clases
en español; guardado automático como hoy; ningún fallo mudo (toast);
componentes ≤300 líneas (la vista delega por sección si crece).

## 5. Gate y Definition of Done

- `tsc --noEmit` EXIT 0 + `vite build` OK por fase.
- Verificación funcional en `tauri dev` real (la página es DOM real) +
  modo web para lo que aplique.
- Gate canónico `sentinel check 119A-4 --stages
  scripts/quality/stages.json` PASS antes del commit; commit en español
  con ID; evidencia en `Agente/completados/tareas-2026-09-11.md`.

## 6. Estado y siguiente paso

**F1 completada 11-09 (mañana):** página implementada + `tsc` EXIT 0 +
`vite build` OK (172.07 kB JS, 51.18 kB CSS) + gate 119A-4 **PASS**
(coverage/sccache/sentinel/rust; rust 138,3s, 430 ok/0 fallidos, alcance
front-only, shell excluido) + smoke `tauri dev`: shell compiló en 10,26s,
app arrancó con ventana "Glory Harness", sin errores ni panics; árbol dev
apagado y limpio tras la prueba. Disco C: 7,95 GB al cierre. Comprobación
visual pendiente del usuario (abrir ajustes, buscador, Escape).
**Siguiente paso:** F2 (reagrupado Personal/Proveedor/Sistema + accesos
directos Chat/Storage/Git), que consume 119A-2 y 119A-3.
Orden con 119A-2/119A-3: independiente (F2 de este plan consume sus
resultados cuando existan, no antes).
