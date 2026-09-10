# Plan 089A-18 — Coherencia de layout, persistencia y workspace

**Fecha:** 2026-09-09  
**Estado:** Hecho (validación funcional completada; gate bloqueado por deuda preexistente fuera del alcance)  
**Proyecto:** glory-harness  
**Rama:** `main`

## Objetivo

Eliminar cuatro inconsistencias observables del rediseño Synara:

1. al ocultar el panel derecho también desaparece su barra de tabs, sin ocultar la botonera de ventana ni el toggle;
2. el layout del panel derecho se conserva entre recargas (visibilidad, ancho, tabs y tab activa), junto con la restauración ya existente de la sidebar;
3. los estados hover, active y focus usan una receta monocroma común y la tab activa tiene una señal visual real;
4. elegir un área de trabajo desde un chat lateral prepara ese chat lateral como borrador, en lugar de mutar siempre el principal.

La semántica de conversación nueva sigue siendo create-on-write: cambiar de área solo cambia el destino y deja el panel originador en borrador; no crea filas hasta enviar el primer mensaje.

## Diagnóstico confirmado

- `panelDerecho.raiz` se desmonta al ocultar, pero `.tabs-barra` vive en `barraSuperior` y queda visible porque es un nodo externo.
- `leerSidebar()` retorna `null` en Tauri; el arranque solo lee las claves de la sidebar. El ancho derecho se guarda pero no se restaura en el flujo real.
- Las tabs y la tab activa existen únicamente en el `Map` de `montarPanelDerecho`; no hay serialización declarativa.
- `crearPanel.ts` contiene el mismo callback de workspace en `DepsPanel` y en las opciones de `montarPanelChat`; ambos dirigen al principal.
- El CSS mezcla opacidad, inversión, `color-mix` y grises literales; Sentinel/VarSense no ejecutan interacción de UI ni incluyen CSS en su análisis efectivo, por lo que no pueden detectar estos contratos funcionales o visuales por sí solos.

## Alcance

- `desktop/ui/src/orquestador/persistencia.ts`, `arranque.ts`, `panelDerecho.ts` y `main.ts` para lectura/escritura y restauración.
- `desktop/ui/src/componentes/panelDerecho.ts` para estado declarativo de tabs y API de restauración.
- `desktop/ui/src/orquestador/crearPanel.ts` para el destino del selector de workspace.
- `desktop/ui/src/estilos/variables.css`, `tabs.css`, `barraSuperior.css`, `layout.css`, `entrada.css`, `navegador.css`, `files.css`, `git.css`, `launcher.css` y `tema.css` solo donde sea necesario para unificar estados.
- Este plan, `roadmap.md` y la evidencia de cierre en `Agente/completados/`.

## No alcance

- No implementar historial atrás/adelante de `089A-5`.
- No ampliar Sentinel para convertirlo en un navegador E2E ni modificar el core de Sentinel/VarSense en esta tarea.
- No cambiar la persistencia de conversaciones ni crear una conversación al seleccionar workspace.
- No cambiar la arquitectura del backend, Tauri, WebView2, Files o Git fuera de los efectos de visibilidad/restauración necesarios.
- No push, deploy ni escrituras externas.

## Diseño y decisiones

### Estado del panel derecho

La fuente de verdad será un estado declarativo mínimo del panel derecho:

- `visible: boolean`;
- `ancho: number` (la clave histórica `lateral_ancho` se conserva por compatibilidad);
- `tabs: TabDerechaId[]` con una allowlist de ids restaurables (`files`, `git`, `navegador`, `chat:<id>`; se descartan ids malformados o chats nuevos);
- `activa: TabDerechaId | null`.

El componente expone lectura/serialización/restauración sin serializar DOM. La restauración usa las aperturas existentes (`abrirFiles`, `abrirGit`, `abrirNavegador`, `abrirEnLateral`) una sola vez por id y después activa la tab guardada. La persistencia se guarda mediante la abstracción existente `configGuardar` en Tauri y `localStorage` en web, con una sola clave JSON versionable para el modelo y la clave histórica para el ancho.

El método de ocultación solo desmonta el contenido/grip y marca la barra de tabs como oculta mediante la API de `BarraSuperior`; el toggle y los controles de ventana permanecen visibles. Mostrar el panel vuelve a aplicar el estado de tabs sin crear duplicados.

### Workspace y panel originador

Se mantiene un único helper local para el callback de cambio de workspace. El destino es `panel` cuando la selección nace en la entrada de cualquier chat. La sidebar conserva su callback independiente hacia el panel principal. Tras activar el workspace, el panel originador pasa a borrador y se enfoca; el refresco de proyectos sincroniza los selectores.

### Estados visuales

Se añaden tokens semánticos para `hover`, `active` y `focus` en `variables.css`. Los componentes reutilizan inversión monocroma para acciones principales y un realce tenue tokenizado para controles secundarios. `:focus-visible` conserva un contorno accesible. El hover rojo de cerrar ventana sigue siendo una excepción explícita del diseño Paseo.

## Fases verificables

1. Registrar esta tarea y el plan en el roadmap.
2. Implementar persistencia declarativa, lectura asíncrona Tauri y restauración de layout/tabs; ocultar exclusivamente tabs con el panel.
3. Corregir el destino del selector de workspace desde laterales.
4. Unificar tokens y recetas hover/active/focus en los estilos afectados, incluyendo tema oscuro.
5. Ejecutar type-check/build de `desktop/ui`, `git diff --check`, `npm run quality:doctor` y el gate canónico con `scripts/quality/stages.json`.
6. Validar en navegador los flujos de ocultar/mostrar, recarga, tabs, selección de workspace y estados hover/focus/active; documentar cualquier límite de modo web/Tauri.
7. Revisar diff y rutas, actualizar evidencia, archivar el plan y crear un commit local explícito. No hacer push ni deploy.

## Criterios de aceptación

- Con panel derecho cerrado no se ve `.tabs-barra`, pero siguen visibles el toggle derecho y, en Tauri, los controles de ventana.
- Ocultar y mostrar no pierde tabs ni su contenido.
- Una recarga restaura visibilidad, ancho dentro de límites, tabs restaurables y tab activa sin duplicados.
- Una tab `chat:nuevo-*` no se persiste como conversación restaurable.
- Seleccionar workspace desde principal afecta al principal; desde un lateral afecta al lateral originador y lo deja en borrador.
- El primer mensaje después del cambio es el único que crea la conversación en modo real.
- Los botones afectados comparten tokens de hover/active/focus y la tab activa se distingue sin depender solo de opacidad.
- Build, diff check, doctor y gate producen evidencia reproducible; la validación visual se registra por separado porque el gate actual no ejecuta E2E ni analiza CSS.

## Riesgos y mitigaciones

- **Restauración antes de montar DOM:** ejecutar la restauración después de `app.appendChild(cuerpo)` y usar las APIs de apertura existentes.
- **Callbacks circulares/TDZ:** inyectar cierres perezosos ya usados por el orquestador y llamarlos solo durante arranque posterior o eventos.
- **Datos de configuración inválidos:** parsear con `try/catch`, validar versión/ids/límites y fallar cerrado a estado vacío sin perder la sesión.
- **WebView2 nativa:** ocultar mediante `navegador_mostrar` al cambiar de tab; no serializar su objeto ni duplicar la vista.
- **Persistencia parcial:** tratar el guardado como best-effort observable mediante el aviso existente; no bloquear la interacción por una preferencia.

## Resultados y evidencia

- Implementación cerrada en los 17 archivos de UI enumerados por `git diff --name-only`, más este plan.
- `desktop/ui`: `npm run type-check` PASS y `npm run build` PASS; Vite transformó 85 módulos.
- `git diff --check` PASS.
- Navegador web `http://127.0.0.1:8799/`: ocultar el panel oculta `.tabs-barra` y conserva el toggle; mostrar restaura Files y la tab activa; cerrar la última tab deja el launcher; Navegador se restaura tras reload; se verificó tema oscuro y tokens de estado.
- Navegador web: se abrieron Navegador, un chat lateral persistente y un chat nuevo; `chat:nuevo-*` quedó fuera del estado persistido. Desde el lateral se cambió `Coolify` a `glory-harness`, el lateral quedó en borrador, el principal no cambió, el conteo de conversaciones permaneció en 8 antes del envío y pasó a 9 únicamente tras el primer mensaje; la respuesta llegó correctamente.
- Limitación: el iframe de `https://example.com` informó `net::ERR_ABORTED` durante reload, sin impedir restaurar la tab. La webview nativa Tauri no se ejecutó en esta sesión.

## Gate y evidencia

Preflight confirmado mediante `npm run quality:doctor`: política válida con hash `cb30d359…`, lock y checkout alineados a `1587c590…` (herramienta fijada **0.7.8**; el runtime global que reporta `doctor.sentinelVersion` es 0.7.4), VarSense 2.2.1 provisionado, `issues: []`, `readyForAnalyze: true` y `readyForGate: true`.

Gate ejecutado: `sentinel check 089A-18 --workspace . --stages scripts/quality/stages.json`. Resultado real: **FAIL**, coverage PASS (0 errores, 0 warnings, 0 info) y Sentinel FAIL (317 errores, 205 warnings, 39 hints). Matiz obligatorio: el reporte del check registra `policy.policyHash: "unavailable"`, `decision.status: "invalid-policy"`, `mode: "observe"` y `blocked: false` — es decir, la identidad de política no llegó al ejecutable, así que ese FAIL es informativo y la única etapa estrictamente *fail-closed* (coverage) pasó. Los hallazgos se reparten en 163 archivos y provienen en su mayoría de `data/referencias-cli/**`, además de deuda Rust preexistente (`block_on` en async, `broadcast::Sender`, `expect`/`unwrap`, límites de líneas); no se atribuyen a los cambios de esta tarea ni se corrigen aquí porque quedan fuera de alcance. El gate no cubre interacción visual ni CSS: no sustituye la validación funcional de navegador realizada arriba.

**Criterio de cierre:** cambios funcionales y documentación completados; el bloqueo del gate queda registrado como deuda base independiente. No se hizo push, deploy ni escritura externa.
