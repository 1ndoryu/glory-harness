# Hallazgos del primer `tauri dev` (039A-1 · debug, 2026-09-04)

Primer arranque real de la app Tauri (PID `glory-harness-desktop`) con el backend in-process.
El usuario señaló en vivo los siguientes problemas y mejoras. **Corregidos (039A-1, 2026-09-04):**
los puntos 1–7 quedaron resueltos (frontend y backend); el punto 8 queda como registro de mejoras
futuras no abordadas en este bloque. Resumen de cierre al final de cada sección.

## 1. Panel «meta» visible siempre, sin meta
- Síntoma: el cuadro de meta (`panel-meta` con `pm-meta`, placeholder «meta…») aparece siempre en
  la entrada, aunque no haya meta definida ni se esté en modo meta.
- Esperado: solo debe mostrarse cuando hay una meta (o cuando el modo es `meta`).
- Causa probable: `main.ts` inserta `panelMeta.raiz` en `#entrada` incondicionalmente
  (`entrada.raiz.insertBefore(panelMeta.raiz, ...)`); `montarPanelMeta` monta la fila siempre.
- Archivos: `desktop/ui/src/componentes/panelMeta.ts`, `desktop/ui/src/main.ts`.

## 2. El mensaje escrito no aparece hasta recargar
- Síntoma: al escribir un mensaje y enviarlo, no se ve en el chat; aparece recién al recargar.
- Esperado: el mensaje del usuario debe renderizarse al enviar (y la respuesta ir llegando).
- Nota: `real.ts` en `montar()` hace `mensajes?.appendChild(crearMensajeUsuario(texto))`; revisar
  si el render del turno falla o el append no llega al contenedor correcto (¿`#mensajes` real?).

## 3. Workspace incorrecto mostrado en configuración
- Síntoma: en configuración/workspace aparece `C:\area-trabajo\glory-harness`, ruta que no parece
  real (la real es `C:\Users\Owner\OneDrive\Documentos\area-trabajo\glory-harness`).
- Causa probable: la UI abre la sesión con `dir: null` (`asegurarSesion` → `abrir_sesion`); con
  `dir: None`, `construir_harness_con` resuelve el **cwd del proceso** Tauri, no el workspace del
  proyecto. Verificar de dónde sale realmente esa ruta (¿cwd al lanzar? ¿symlink?).
- Archivos: `desktop/ui/src/tauri/real.ts` (envía `dir: null`), `desktop/src-tauri/src/main.rs`
  (`abrir_sesion_interna` con `dir: None`), `desktop/ui/src/main.ts` (arranque).

## 4. Conversación nueva en cada arranque y «todas abiertas»
- Síntoma: cada vez que inicia la app aparece una conversación nueva; además parecen quedar todas
  abiertas/seleccionadas en la lista.
- Causa probable: `abrir_sesion_interna` crea SIEMPRE una conversación nueva
  (`conversacion_crear(user_id, "Nueva conversación")`) en cada apertura, sin reutilizar la última
  activa. En el arranque real `main.ts` hace `asegurarSesion` + resincroniza la lista; la vacía
  recién creada queda en la lista y la lógica «reabrir donde se quedó» salta a la segunda.
- Esperado: reutilizar la última conversación activa no archivada en el arranque (no crear vacía
  nueva si ya hay hilo). Bug de backend (`abrir_sesion_interna`) + arranque de `main.ts`.

## 5. El título de la conversación no se auto-genera tras el primer mensaje
- Síntoma: queda siempre «Nueva conversación».
- Esperado: tras enviar el primer mensaje, generar un **nombre breve automático** (como en las
  referencias: primeras palabras del primer mensaje del usuario), no «Nueva conversación».
- Nota: ver dónde conviene generarlo (backend `enviar_turno` o comando de renombrado implícito).

## 6. Al recargar no reaparece el resumen («summary») de la acción ejecutada
- Síntoma: el usuario pidió crear un archivo y sí se creó; pero al recargar no vio el bloque de
  resumen/resultado de la acción (el «summary» que el boceto/simulación mostraba al ejecutar una
  herramienta).
- Esperado: el resumen de la ejecución (diff / resultado) debería persistir y reaparecer en el
  historial al recargar, como en el boceto.
- Nota: ver cómo se guarda/recupera el `tool_result` (resumen/diff) en el historial y cómo lo pinta
  `pintarHistorial` (hoy solo `user`/`assistant`; el detalle de herramientas no se repinta).

## 7. Selector de nivel de razonamiento no funciona (barra)
- Síntoma: el control «Medio» de la barra (`#control-razonamiento`) no responde; no abre ningún
  menú ni permite cambiar low/medium/high.
- Causa: en `entrada.ts` es un `<span class="control">` estático con `textContent = 'Medio'` y sin
  handler (`// control: razonamiento (estático en el boceto)`). `setRazonamiento()` solo actualiza
  el texto; el único lugar donde se cambia hoy es el modal (select en `opciones.ts`,
  `nivelRazonamiento`), y ese valor NO viaja al backend (el turno no lo envía: `opcionesTurno()`
  manda proveedor/modelo/modo únicamente).
- Esperado: selector de razonamiento funcional (menú igual al de modo con Bajo/Medio/Alto) y que
  el nivel elegido se aplique al turno real (persistencia/contrato del núcleo).
- Archivos: `desktop/ui/src/componentes/entrada.ts`, `desktop/ui/src/main.ts` (`opcionesTurno`),
  `desktop/src-tauri/src/main.rs` (si el backend recibe razonamiento), `desktop/ui/src/dominio/opciones.ts`.

## Resumen de correcciones aplicadas (bloque 039A-1, 2026-09-04)

1. **H1 (panel meta)** — `panelMeta.ts` expone `mostrar(visible)`/`visible()`; CSS añade
   `.panel-meta.oculto { display:none }`; `main.ts` añade `sincronizarPanelMeta()` que muestra el
   panel solo si hay meta o el modo es `meta`. Se llama al montar, al cambiar modo y al editar meta.
   Verificado en navegador (VITE_MOCK): oculto en modo predeterminado, visible al entrar en `meta`.
2. **H2 (mensaje no visible al enviar)** — `real.ts` baja el scroll del chat al añadir el mensaje
   del usuario (`montar`) y en cada evento (`token`/`tool_start`) para que la respuesta fluya a la
   vista. `aviso` también baja el scroll.
3. **H3 (workspace incorrecto)** — `opciones.ts`: el campo workspace de Contexto queda vacío por
   defecto con nota «se abre con el workspace real del backend»; el backend persiste el workspace
   resuelto y `onSesion` en `main.ts` rellena el modal con `info.workspace` real al abrir/reconfigurar.
4. **H4 (conversación nueva por arranque)** — backend `abrir_sesion_interna` reutiliza la última
   conversación no archivada (no crea vacía nueva si hay hilo); `main.ts` ya no salta a una segunda
   candidata y carga `candidatas[0]` con su historial real.
5. **H5 (título no auto-generado)** — backend `enviar_turno` genera un nombre breve automático
   desde el primer mensaje cuando el título es «Nueva conversación»; al terminar el turno el front
   refresca la sidebar y el título de la cabecera (`alTerminar`).
6. **H6 (resumen de acción no reaparece al recargar)** — backend `cargar_conversacion` devuelve
   `acciones` (`AccionRecuperada`: tool/ok/resumen/argumentos/diff/turno_en); front repinta bloques
   `.herramienta` intercalados en `pintarHistorial` (user → acciones → assistant) con resumen/diff.
7. **H7 (selector de razonamiento)** — `entrada.ts`: botón real con menú Bajo/Medio/Alto
   (patrón de modo), API `setRazonamientoValor`/`getRazonamiento`; `real.ts` envía `razonamiento` en
   `abrir_sesion`/`reconfigurar_sesion` y en `OpcionesTurno`; `main.ts` lo propaga barra ↔ modal y lo
   persiste (`nivelRazonamiento`); backend lo resuelve del parámetro o config. Verificado en navegador:
   el menú abre y cambia el nivel mostrado.

Validación: `npm run type-check` y `npm run build` del front limpios; verificación visual en
VITE_MOCK para H1/H7. El flujo completo con el núcleo se valida en `tauri dev` (backend in-process).

## 8. Más problemas y mejoras pendientes
- El usuario indicó que hay más cosas que señalar en vivo; se están anotando contra el front en el
  navegador (Vite con `VITE_MOCK=1`, contenido de ejemplo/simulación).

## Estado del primer arranque
- `tauri dev` compiló OK en `C:\tmp\glory-harness-target` y la ventana real abrió (F5 visual check
  en curso). Capabilities B1 (`capabilities/default.json`) presentes; B2 (detección `__TAURI__`)
  verificable en la webview.
- La UI del navegador NO replica el backend (IPC Tauri solo existe en la webview). Para señalar la
  UI en vivo en el navegador se usa `VITE_MOCK=1` (simulación con contenido de ejemplo).
