# Plan — Explorador real del workspace, cambios/diffs y terminal

Fecha: 2026-09-08  
Estado: **activo — diseño previo a implementación**  
Veredicto de revisión: **VIABLE CON RESERVAS**  
Proyecto: `glory-harness`  
Área: `desktop/ui` + `desktop/src-tauri`  

> **Decisión de alcance:** el objetivo es viable, pero el plan original intentaba resolver filesystem, Git, watcher, snapshots y PTY en una sola hoja de ruta. Se reduce el primer bloque a filesystem real + apertura segura + contratos de procedencia. Git, watcher y terminal quedan detrás de evidencia y presupuestos; no se implementan por anticipado.

## 1. Objetivo

Convertir el visor actual de Glory Harness en una superficie de trabajo real que permita:

1. explorar los archivos reales del workspace activo;
2. abrir un archivo y leer su contenido con seguridad;
3. buscar archivos y texto sin depender de GitHub;
4. resumir los cambios producidos realmente por el agente;
5. mostrar el diff de un archivo seleccionado;
6. distinguir cambios del agente, cambios locales de Git, snapshots y cambios externos;
7. actualizar el estado cuando el filesystem cambie fuera de la aplicación;
8. ofrecer una terminal interactiva real, cuando la fase de terminal esté habilitada.

El resultado debe servir para trabajar sobre el proyecto local, no para mostrar una representación remota o aproximada del repositorio.

## 2. No-goals

Esta tarea no incluye:

- usar GitHub como fuente primaria del árbol, el contenido o el diff;
- convertir el visor en un IDE completo con LSP, depurador o editor avanzado;
- sustituir el sistema de conversaciones, tools o persistencia ya existente;
- mezclar el diff generado por una tool con el diff actual de Git como si fueran la misma evidencia;
- ejecutar comandos remotos, SSH, Docker remoto o despliegues;
- implementar commits, push, pull, merge o rebase desde la terminal inicial;
- añadir una dependencia grande antes de verificar que la plataforma o las dependencias existentes no resuelven el caso;
- mantener un snapshot completo del workspace en memoria sin un límite explícito de tamaño;
- ocultar errores de permisos, lectura, watcher o procesos bajo un estado vacío.

## 3. Estado confirmado

### 3.1 Frontend actual

El frontend es TypeScript vanilla con Vite, sin React ni un sistema de componentes externo. Las piezas relevantes son:

- `desktop/ui/src/main.ts`: orquestación de workspace, paneles, visor, navegador, chat y eventos de cambios.
- `desktop/ui/src/componentes/panelVisor.ts`: visor actual de archivo y cambios de tools.
- `desktop/ui/src/componentes/panelChat.ts`: conversación, borrador lateral y cambios asociados a la conversación.
- `desktop/ui/src/componentes/panelDerecho.ts`: tabs del panel derecho y launcher/estado vacío.
- `desktop/ui/src/componentes/menu.ts`: menú contextual compartido.
- `desktop/ui/src/tauri/real.ts`: adaptación de comandos y eventos Tauri, incluido `onCambioArchivo`.
- `desktop/ui/src/estilos/tabs.css`, `layout.css` y estilos de los paneles: sistema visual monocromo ya existente.

El visor actual puede:

- leer archivos individuales mediante el comando `leer_archivo`;
- mostrar archivos abiertos manualmente;
- mostrar cambios conocidos de `file_write` y `file_patch`;
- conservar y listar cambios asociados al historial de una conversación.

El visor actual todavía no ofrece un árbol del filesystem, búsqueda de workspace, Git status/diff local, watcher de cambios externos ni una terminal PTY.

### 3.2 Backend actual

`desktop/src-tauri/src/archivo.rs` ya valida y lee archivos dentro del workspace. Debe reutilizarse como base de seguridad, pero no crecer hasta convertirse en un módulo que mezcle lectura, Git, watcher y procesos.

El backend ya dispone de comandos IPC para workspace y conversación. La ampliación debe conservar el límite del workspace activo y la validación de rutas existente.

### 3.3 Convenciones del proyecto

- El proyecto usa Rust/Tauri en backend y TypeScript vanilla en frontend.
- Los artefactos de compilación deben ir a `C:\tmp` según `AGENTS.md`.
- La UI reutiliza tokens, estilos y componentes existentes; no se añaden colores, fuentes o medidas visuales arbitrarias en los componentes.
- Los errores de filesystem, procesos, IPC y watcher deben ser estados explícitos.
- Los cambios visuales no relacionados que ya están en el árbol de trabajo deben preservarse.

## 4. Referencias comparadas

La investigación de Synara y OpenCode se usará como referencia de comportamiento, no como motivo para copiar su arquitectura completa.

### 4.1 Synara

Hechos útiles para este proyecto:

- usa el filesystem local/RPC para construir la vista de archivos;
- mantiene el watcher separado de la lectura inicial;
- permite que la UI vuelva a consultar o invalide partes concretas del árbol;
- trata Git local como una fuente adicional de estado;
- no necesita GitHub para mostrar el workspace local.

Aplicación en Glory Harness: adaptar el patrón de fuente local + invalidación, manteniendo Tauri como límite de seguridad y transporte.

### 4.2 OpenCode

Hechos útiles para este proyecto:

- expone un modelo de árbol del workspace con carga incremental;
- obtiene estado y diff desde el VCS local;
- conserva también cambios producidos directamente por tools;
- permite distinguir la herramienta que produjo un cambio del estado VCS posterior;
- usa watcher/refetch para mantener la UI sincronizada;
- implementa terminal mediante PTY real, no mediante una caja de texto que simula salida.

Aplicación en Glory Harness: separar filesystem, tools, Git, snapshots y terminal detrás de adaptadores pequeños y unirlos solamente en el modelo de presentación.

### 4.3 Decisión sobre GitHub

**GitHub no será la fuente primaria.**

La procedencia será:

1. filesystem local: árbol, existencia y contenido actual;
2. Git local opcional: estado, patch y comparación contra la referencia local seleccionada;
3. eventos e historial de tools: cambios que el agente produjo y que la aplicación conoce de forma directa;
4. snapshots/checkpoints: estado guardado por la aplicación, si se implementan;
5. watcher: invalidación y notificación de cambios externos.

GitHub solo podría añadirse posteriormente como fuente explícita de remoto, nunca como sustituto silencioso del workspace activo.

## 4.4 Reto arquitectónico y veredicto

### Problema real

El visor actual no está incompleto por falta de botones: mezcla dos contratos distintos. `panelVisor.ts` recibe contenido leído por `leer_archivo` y cambios de tools ya convertidos a HTML; no tiene un contrato para enumerar el filesystem ni una fuente de diff local. Además, `real.ts` trunca el diff del evento a 600 caracteres antes de entregarlo al visor. Por tanto, añadir un árbol encima del visor actual sin corregir la procedencia produciría una UI convincente pero incompleta.

### Hechos confirmados frente a supuestos

**Confirmado en el código:**

- `desktop/src-tauri/src/archivo.rs` solo implementa `leer_archivo`, con contención bajo el workspace, rechazo de carpetas, UTF-8 y límite de 256 KiB.
- `main.rs` registra `archivo::leer_archivo`, pero no comandos de listar, buscar, Git, watcher ni terminal.
- `desktop/ui/src/tauri/real.ts` expone eventos de tools y `onCambioArchivo`, pero el contrato actual entrega `diffHtml`, no un patch estructurado.
- `panelChat.ts` conserva cambios por conversación, pero `panelVisor.ts` deduplica por ruta y no distingue origen.
- `desktop/src-tauri/Cargo.toml` no declara actualmente crates de watcher, Git ni PTY.
- El modo web no tiene acceso al filesystem: `panelVisor.ts` informa que la lectura solo está disponible en Tauri. La verificación de filesystem real debe hacerse en la ventana Tauri, no en el navegador web.
- El vault existente respalda archivos para rewind/restauración; no es todavía un snapshot navegable del workspace y no debe reutilizarse con una semántica distinta sin contrato.

**Supuestos que quedan prohibidos hasta verificar:**

- que una librería Git o PTY ya esté disponible transitivamente;
- que el watcher pueda observar un workspace completo sin límites en Windows;
- que la UI pueda mostrar un patch completo usando el HTML truncado actual;
- que el gate de Sentinel cubra TypeScript: `sentinel.config.json` declara patrones de análisis de Rust, `Cargo.toml` y `Cargo.lock`; `tsc` y Vite son comprobaciones separadas;
- que la terminal de tools (`comando`) sea una terminal interactiva. Un proceso puntual no sustituye una PTY.

### Veredicto

**VIABLE CON RESERVAS.** La dirección es correcta solo si el primer entregable demuestra una ruta local real con límites y errores. Se rechaza crear de entrada una jerarquía de interfaces, un índice persistente, un event bus o un modelo de snapshots completo. Se usarán módulos concretos y DTOs pequeños; una abstracción común se introduce únicamente cuando existan dos fuentes reales que la consuman.

### Autorización

Queda **AUTORIZADO PARA EJECUTAR** el ciclo local de diseño, edición, tests, builds, gate aplicable y commit de cada bloque. No queda autorizado ningún push, deploy, escritura remota ni operación por SSH. La terminal futura será local y no se habilitará como canal de producción.

## 5. Principios de arquitectura

### 5.1 SRP y mínima abstracción

Cada módulo responderá por una sola clase de información, pero inicialmente serán módulos concretos, no traits/interfaces por reflejo:

- `archivo.rs`/módulo filesystem: listar, leer, buscar y validar rutas;
- módulo Git futuro: consultar status y diff local;
- historial de tools existente: conservar cambios declarados por `file_write` y `file_patch`;
- watcher futuro: emitir invalidaciones;
- terminal futura: controlar una sesión PTY.

No se crea un `WorkspaceService` omnipotente ni un `WorkspaceSnapshots` en la primera fase. La fachada Tauri puede traducir los módulos a DTOs, pero no debe contener la lógica de filesystem, Git y procesos. Un contrato común de cambios solo se consolidará cuando filesystem y Git/tool tengan implementaciones verificadas; hasta entonces se normalizan datos en el borde de la UI sin ocultar la fuente.

### 5.2 Revisión SOLID

- **SRP:** filesystem, Git, historial de tools, watcher y PTY tienen ciclos de fallo diferentes; permanecen separados.
- **OCP:** añadir Git o watcher no debe cambiar la validación de rutas ni `panelChat`.
- **LSP:** no se tratará “sin Git” o “sin watcher” como una implementación vacía de otra fuente; son estados explícitos.
- **ISP:** el frontend consumirá comandos pequeños (`listar`, `leer`, `buscar`, `status`, `diff`) y no un DTO gigante de workspace.
- **DIP:** solo se extraerá un puerto si aparece un segundo consumidor real (por ejemplo, web local o tests). Tauri es el consumidor confirmado; no se abstrae por anticipación.

### 5.2 Modelo común de cambio

El modelo común sirve para representar cambios en la UI, no para borrar las diferencias entre sus fuentes.

Campos mínimos propuestos:

```text
CambioWorkspace {
  ruta: string                 // ruta relativa y normalizada al workspace
  estado: nuevo | modificado | eliminado | renombrado | sin_cambios | desconocido
  adiciones: number | null
  eliminaciones: number | null
  patch: string | null
  origen: git | tool | snapshot | filesystem
  referencia: string | null    // id de conversación, tool, snapshot o commit local
  actualizadoEn: string
}
```

Reglas:

- `ruta` siempre es relativa al workspace y nunca puede escapar mediante `..`;
- `origen` es obligatorio y se muestra en la UI cuando las fuentes difieren;
- `patch: null` significa que todavía no se calculó o que la fuente no lo soporta, no que el archivo no cambió;
- un cambio de tool no se sobreescribe con Git: se presenta junto al estado Git relacionado;
- un archivo eliminado puede tener patch aunque ya no pueda leerse desde el filesystem;
- los contadores son opcionales porque no todas las fuentes los calculan de igual manera.

### 5.3 Modelo de archivo

```text
EntradaWorkspace {
  ruta: string
  nombre: string
  tipo: archivo | directorio | enlace | desconocido
  tamano: number | null
  modificadoEn: string | null
  ignorado: boolean
  hijos: EntradaWorkspace[] | null
}
```

`hijos: null` significa que el directorio todavía no se ha cargado, no que esté vacío. El árbol se cargará bajo demanda para evitar leer todo el workspace al abrir la aplicación.

### 5.4 Modelo de errores

Los DTOs de IPC deben distinguir, como mínimo:

- workspace no configurado;
- ruta fuera del workspace;
- archivo inexistente;
- archivo binario o demasiado grande para el visor;
- permiso denegado;
- Git no disponible o ruta no versionada;
- watcher no disponible;
- terminal no disponible;
- proceso terminado con código o señal;
- sesión PTY perdida.

El frontend debe mostrar estos estados con una acción siguiente. Un error no debe convertirse en una lista vacía o en un diff en blanco sin explicación.

## 6. Contratos funcionales propuestos

### 6.1 Filesystem

Comandos iniciales:

- `workspace_listar_entrada(ruta_relativa, profundidad)`
- `workspace_leer_archivo(ruta_relativa, limite_lineas o limite_bytes)`
- `workspace_buscar(consulta, ruta_relativa?, limite_resultados)`
- `workspace_info()`

Requisitos:

- raíz resuelta y canónica del workspace activo;
- validación de contención antes de abrir, listar o buscar;
- orden determinista: directorios primero y nombres de forma estable;
- límite de profundidad, resultados, bytes y tiempo;
- exclusión configurable de carpetas pesadas (`target`, `node_modules`, `.git` y equivalentes), sin ocultar una exclusión en silencio;
- lectura con codificación UTF-8 cuando sea texto y estado explícito para binarios;
- no seguir enlaces fuera de la raíz sin una política explícita.

### 6.2 Árbol y búsqueda

El árbol debe:

- abrir el directorio raíz del workspace activo;
- cargar hijos al expandir un directorio;
- conservar qué carpetas están expandidas al cambiar de tab;
- permitir refrescar una carpeta concreta;
- marcar archivos con cambios sin reconstruir todo el árbol;
- ofrecer búsqueda de nombres y, en una fase posterior, contenido;
- mostrar cargando, vacío, error y resultados truncados.

La primera versión debe preferir una búsqueda acotada y fiable frente a un indexador persistente. Solo se añadirá indexación si la medición con workspaces reales demuestra que el límite es necesario.

### 6.3 Apertura de archivos

Al seleccionar un archivo:

1. el frontend solicita el contenido al backend;
2. el backend valida la ruta y aplica límites;
3. el visor muestra ruta, nombre, tamaño y estado de lectura;
4. el panel conserva la tab o selección según el patrón actual de `panelVisor`;
5. un cambio posterior invalida el contenido y ofrece recargarlo, sin perder silenciosamente el contenido que estaba abierto.

El visor no debe editar archivos en esta fase. La edición se mantiene en las tools del agente hasta que exista un contrato separado para editor, guardado, conflictos y undo.

### 6.4 Git local

Cuando el workspace sea un repositorio Git:

- consultar `status --porcelain` mediante una API de proceso controlada o una librería ya aprobada;
- obtener diff por archivo y resumen agregado;
- identificar nuevo, modificado, eliminado y renombrado;
- respetar el directorio de trabajo y no ejecutar comandos concatenados desde entrada del usuario;
- aplicar timeout, límite de salida y cancelación;
- mostrar claramente que la información proviene de Git local.

Si no es un repositorio Git, la UI debe seguir mostrando filesystem y cambios de tools. “Sin Git” no equivale a “sin cambios”.

La elección entre proceso Git controlado y crate debe hacerse después de revisar las dependencias ya presentes. No se añadirá una capa de abstracción por anticipado si un adaptador pequeño cubre el contrato.

### 6.5 Cambios de tools

Los eventos existentes `AgenteEvento` y `CambioArchivoPanel` son la fuente directa de los cambios conocidos por `file_write` y `file_patch`.

El registro debe conservar:

- ruta relativa normalizada;
- conversación y turno de origen;
- tool y operación (`write` o `patch`);
- patch o resumen disponible;
- hora y orden del evento;
- resultado de la operación;
- relación con el archivo actualmente visible, si existe.

Estos cambios deben seguir siendo visibles aunque Git no esté instalado o el archivo aún no tenga un commit base. Si después se consulta Git, ambas procedencias se muestran como capas distintas.

### 6.6 Watcher

El watcher debe ser independiente de los eventos del agente. Su función es detectar cambios externos y marcar recursos como obsoletos.

Comportamiento mínimo:

- observar el workspace activo con exclusiones y límites;
- agrupar ráfagas de eventos para evitar repintados continuos;
- invalidar solo la carpeta, archivo o consulta afectada;
- detectar creación, modificación, eliminación y renombrado cuando el backend lo permita;
- avisar si el watcher se detiene o no puede observar una ruta;
- cancelar la suscripción al cambiar de workspace o cerrar la sesión.

Los eventos de tools pueden disparar una invalidación inmediata, pero no sustituyen al watcher: otro proceso puede editar el mismo archivo y el agente puede fallar antes de emitir un evento final.

### 6.7 Terminal PTY

La terminal será una sesión PTY real, no una consola simulada con `textarea`.

Contrato mínimo:

- `terminal_crear(cwd, shell, columnas, filas)`;
- `terminal_escribir(id, bytes o texto)`;
- `terminal_redimensionar(id, columnas, filas)`;
- `terminal_cerrar(id)`;
- eventos de salida por chunks;
- evento de proceso terminado con código/señal;
- estado explícito de desconexión y cierre.

Requisitos de seguridad y operación:

- el directorio inicial debe estar dentro del workspace o en una ubicación permitida explícitamente;
- no concatenar comandos del usuario en un shell no interactivo;
- no almacenar secretos de entorno en logs, historial de UI ni eventos de diagnóstico;
- aplicar límites de sesiones, tamaño de buffer y tiempo de inactividad;
- matar el árbol de procesos al cerrar, de forma compatible con Windows;
- limpiar sesiones al cambiar de workspace y al cerrar la aplicación;
- soportar resize real para evitar programas que crean una salida corrupta;
- decidir la implementación Windows después de verificar ConPTY y las dependencias existentes; no elegir un crate por analogía con OpenCode sin comprobar el contrato local.

La terminal no debe tener acceso implícito a producción ni convertirse en un canal remoto. Su alcance inicial es el proceso local del usuario.

## 7. Integración con la UI existente

### 7.1 Panel derecho y tabs

Se reutilizarán:

- `panelDerecho.ts` para montar la tab de Files y, posteriormente, Terminal;
- `panelVisor.ts` como base del visor de archivo/cambios;
- el launcher estilo Synara ya previsto en `089A-4` para ofrecer Files, Changes y Terminal;
- los estilos monocromos y tokens existentes;
- el menú contextual compartido en `menu.ts` si se necesita una acción de archivo.

No se creará una segunda navegación paralela para Files ni una segunda implementación de tabs.

### 7.2 Estados visibles

Cada superficie debe definir los estados:

- sin workspace;
- cargando;
- vacío;
- resultado normal;
- resultado truncado;
- error recuperable;
- archivo obsoleto por cambio externo;
- archivo eliminado;
- Git ausente/no aplicable;
- terminal iniciando, activa, terminada y desconectada.

La UI debe ser usable con teclado y mantener el contraste/estética del proyecto en 320px, 390px, 768px y escritorio.

## 8. Fases de implementación

### Fase 0 — Contratos y fixtures

**Objetivo:** fijar los DTOs, errores, límites y ejemplos antes de crear UI adicional.

Trabajo:

- revisar los contratos actuales de `leer_archivo`, workspace y eventos de tools;
- definir tipos Rust/TypeScript para entradas, cambios, errores y eventos;
- crear fixtures pequeños de workspace: archivo nuevo, modificado, eliminado, binario, ruta inválida y cambio externo;
- decidir límites iniciales de bytes, profundidad, resultados y timeout;
- documentar las decisiones de Git local y origen de cambios.

Salida: contratos revisables y una prueba mínima que falle si se pierde la procedencia del cambio.

### Fase 1 — Adaptador filesystem local

**Objetivo:** listar, leer y buscar dentro del workspace activo.

Trabajo:

- extraer la validación de ruta reutilizable desde `archivo.rs` sin duplicarla;
- implementar listado incremental y lectura limitada;
- añadir búsqueda acotada;
- devolver errores estructurados;
- conectar comandos IPC y adaptador Tauri real;
- añadir tests de contención, límites, archivo inexistente y lectura válida.

No se toca todavía Git ni terminal.

### Fase 2 — Árbol y apertura de archivos

**Objetivo:** que Files muestre el workspace real y abra archivos en el visor.

Trabajo:

- añadir la vista de árbol dentro del panel existente;
- carga lazy por directorio;
- selección y apertura mediante `panelVisor`;
- estados de carga, error y archivo no legible;
- refresco de una rama del árbol;
- búsqueda de nombres y navegación al resultado.

Criterio: el usuario puede abrir un archivo que exista en el workspace aunque no esté en GitHub ni tenga cambios de Git.

### Fase 3 — Git status y diff local

**Objetivo:** mostrar el estado VCS local cuando exista.

Trabajo:

- detectar si la raíz es un repositorio Git;
- consultar status con límites y timeout;
- obtener diff agregado y por ruta;
- mapear estados a `CambioWorkspace` con `origen: git`;
- mostrar estado “Git no aplicable” sin bloquear Files;
- tests con fixture Git local y workspace sin Git.

No se implementan commit, push, pull ni operaciones destructivas.

### Fase 4 — Resumen de cambios reales del agente

**Objetivo:** representar con precisión lo que hicieron las tools.

Trabajo:

- adaptar `CambioArchivoPanel` al modelo común;
- conservar la relación conversación → turno → tool → archivo;
- mostrar resumen agregado por conversación y por workspace;
- seleccionar un archivo desde el resumen y abrir su diff;
- distinguir cambio declarado por tool de estado Git posterior;
- cubrir `file_write`, `file_patch`, error de tool y archivo creado sin base Git.

Criterio: el usuario puede responder “qué editó realmente el agente” con evidencia del evento de tool, incluso sin Git.

### Fase 5 — Watcher e invalidación

**Objetivo:** mantener el árbol y el visor razonablemente sincronizados.

Trabajo:

- implementar suscripción de cambios del workspace;
- agrupar eventos de filesystem;
- invalidar rutas concretas;
- marcar archivos abiertos como obsoletos;
- refrescar status/diff Git con debounce controlado;
- liberar watcher al cambiar workspace o cerrar la aplicación;
- probar cambios externos mientras la UI está abierta.

La primera versión puede ofrecer un botón de recarga en vez de recargar contenido automáticamente si existen conflictos de UX. Esa decisión debe quedar explícita y no ocultar el estado obsoleto.

### Fase 6 — Terminal PTY

**Objetivo:** añadir una terminal local interactiva y acotada.

Trabajo:

- confirmar API Windows disponible y dependencia mínima;
- crear sesión PTY con cwd validado;
- transmitir salida por eventos con backpressure/límite de buffer;
- escribir entrada y redimensionar;
- mostrar salida, prompt y finalización sin reinterpretar ANSI de forma insegura;
- cerrar el árbol de procesos y limpiar sesiones;
- integrar Terminal como tab del panel derecho/launcher;
- probar comandos inocuos, resize, cierre, proceso que termina y cambio de workspace.

La fase no se inicia hasta que Files/Changes tengan contratos y errores verificables.

## 9. Riesgos y mitigaciones

| Riesgo | Consecuencia | Mitigación |
|---|---|---|
| Leer todo el workspace al abrir | latencia y memoria excesivas | árbol lazy, límites y exclusiones visibles |
| Confundir tool diff con Git diff | resumen falso de lo que hizo el agente | `origen` obligatorio y vistas separadas |
| Cambios externos durante una lectura | visor obsoleto o pérdida de contexto | watcher, hash/mtime y aviso de recarga |
| Symlink fuera de la raíz | lectura de archivos no autorizados | canonicalización y comprobación de contención |
| Git ausente o workspace sin Git | Files inutilizable si se acopla a Git | Git como adaptador opcional |
| salida de Git o terminal sin límite | bloqueo, consumo de RAM o UI congelada | timeout, cancelación, chunks y límites |
| shell/terminal con entorno sensible | exposición de secretos | no registrar env, redactar diagnósticos y limitar cwd |
| proceso hijo huérfano en Windows | fugas y procesos inesperados | cierre de árbol y limpieza al desmontar |
| watcher con demasiados eventos | repintado continuo | debounce, coalescing e invalidación por ruta |
| snapshot completo prematuro | duplicación de datos y almacenamiento | posponer snapshots hasta un caso real |
| cambios visuales ajenos | conflictos y regresiones | editar por módulo y revisar diff antes de validar |

## 10. Decisiones de simplicidad

- El árbol y la lectura se implementan primero con APIs locales y nativas ya disponibles.
- Se evita un índice persistente hasta tener una medición que lo justifique.
- Git es opcional y de solo lectura en la primera versión.
- Los eventos de tools se reutilizan; no se construye un segundo registro de operaciones.
- El modelo común es un DTO de presentación, no una jerarquía de clases o un event bus nuevo.
- La terminal se limita a una sesión local PTY por tab antes de añadir multiplexado, historial complejo o reconexión entre reinicios.
- No se añade una dependencia de terminal, watcher o Git sin revisar primero las existentes y el tamaño/contrato de la solución.

## 11. Criterios de aceptación

### Filesystem y árbol

- [ ] Con workspace configurado, Files muestra entradas reales del directorio local.
- [ ] Expandir un directorio solo solicita sus hijos y respeta los límites.
- [ ] Una ruta fuera del workspace falla con un error explícito.
- [ ] Un archivo de texto válido se abre en el visor.
- [ ] Un binario o archivo demasiado grande muestra estado explicado, no contenido corrupto.
- [ ] La búsqueda devuelve resultados limitados y permite abrir el archivo.
- [ ] Un workspace sin Git sigue funcionando.

### Cambios y diffs

- [ ] Se muestran cambios de `file_write` y `file_patch` con conversación, tool y ruta.
- [ ] Git local muestra status/diff cuando aplica.
- [ ] La UI diferencia visualmente `git`, `tool`, `snapshot` y `filesystem`.
- [ ] Seleccionar un archivo desde el resumen abre su diff o explica por qué no hay patch.
- [ ] Un archivo creado por una tool aparece aunque no exista en el índice Git.
- [ ] Un cambio externo marca el archivo o carpeta afectada como obsoleto.
- [ ] Un error de Git no oculta el árbol ni los cambios de tools.

### Terminal

- [ ] La terminal crea una sesión PTY en el workspace activo.
- [ ] La entrada, salida por chunks y resize funcionan.
- [ ] La finalización del proceso muestra código/señal.
- [ ] Cerrar la tab termina la sesión y no deja procesos hijos propios.
- [ ] El cambio de workspace no reutiliza una sesión con cwd anterior.
- [ ] No aparecen secretos en los eventos, logs ni resumen de UI.

### Calidad

- [ ] `tsc --noEmit` pasa para `desktop/ui`.
- [ ] `npm run build` pasa para `desktop/ui`.
- [ ] Los tests Rust relevantes pasan con target en `C:\tmp`.
- [ ] El gate declarado por el proyecto se ejecuta con el comando y manifest reales, si está listo para gate.
- [ ] Se revisan `git diff --check`, `git diff` y `git status --short`.
- [ ] Se verifica al menos una ruta funcional en el navegador web y otra en la ventana Tauri cuando el cambio llegue al ciclo real.

## 12. Evidencia y documentación de cierre

Mientras el plan esté activo, conservar:

- contratos y decisiones en este plan;
- tests o fixtures que reproduzcan cada fuente de cambio;
- comandos ejecutados y resultado real;
- limitaciones de watcher, Git o terminal claramente registradas.

Al cerrar una fase:

1. actualizar el estado de esta fase y su evidencia;
2. registrar la tarea en `Agente/completados/tareas-YYYY-MM-DD.md` cuando el bloque quede terminado;
3. actualizar `roadmap.md` solo con el siguiente bloque realmente ejecutable;
4. mover este plan a `Agente/planes/completados/` únicamente cuando todas las fases comprometidas estén cerradas o se archive explícitamente con pendientes;
5. no afirmar que el workspace es “real” si solo se verificó el mock web o el build.

## 13. Evaluación de eficiencia, escala y operación

### Modelo de carga inicial

Este es un producto local de un usuario, con un proceso Tauri y hasta dos paneles de conversación ya soportados. No hay objetivo de servidor multiusuario ni de escalar el árbol entre máquinas. El plan no debe vender “escalabilidad” sin medición.

Presupuestos iniciales, sujetos a fixture y medición en la Fase 0:

- archivo de texto visible: conservar el límite existente de 256 KiB;
- hijos devueltos por una expansión: máximo propuesto de 500 entradas por llamada;
- resultados de búsqueda: máximo propuesto de 1.000, con indicador de truncamiento;
- salida de Git o terminal retenida en UI: máximo propuesto de 1 MiB por sesión/consulta antes de recortar por chunks;
- timeout de una consulta local: máximo propuesto de 5 s para filesystem/Git, cancelable;
- watcher: debounce de eventos y una única invalidación por ruta durante una ráfaga.

Estos números no son una afirmación de rendimiento: se validan contra workspaces pequeños, medianos y un fixture pesado antes de adoptarse.

### Rutas calientes y costes

- listar una carpeta: $O(n)$ en entradas de esa carpeta; no se recorre todo el workspace al abrir Files;
- búsqueda v1: recorrido acotado $O(n)$ con exclusiones explícitas; sin índice persistente;
- diff Git: un proceso local por consulta o una consulta agrupada, nunca un proceso por fila del árbol;
- render del árbol: actualizar la rama afectada, no reconstruir todo el DOM;
- watcher: coalescer eventos antes de pedir status/diff;
- terminal: chunks acotados y backpressure; nunca concatenar toda la salida en memoria sin límite.

La observabilidad mínima será duración, resultado, bytes/entradas devueltos y motivo de truncamiento. No se registrará contenido sensible ni variables de entorno.

## 14. Seguridad y mitigaciones obligatorias

- **Rutas:** canonicalizar y comprobar contención antes de listar/leer; rechazar rutas vacías, escapes y enlaces fuera de raíz. Añadir tests de `..`, ruta absoluta externa, symlink y archivo eliminado durante la consulta.
- **Procesos Git:** ejecutar el binario con argumentos estructurados y `current_dir` validado; nunca `cmd /c`, `sh -c`, interpolación de comandos ni entrada concatenada. Limitar timeout, stdout, stderr y procesos hijos.
- **PTY:** cwd dentro del workspace permitido; no copiar secretos al historial; no serializar el entorno completo a la UI; cerrar el árbol de procesos propio en Windows.
- **HTML de diffs:** no introducir `innerHTML` con contenido de archivos, patches o tool results. El visor existente usa HTML para resultados de tools y debe auditarse antes de reutilizarlo; la v1 del diff debe preferir texto/DOM construido con `textContent` o un sanitizador ya aprobado.
- **Errores:** no transformar permisos, binarios, timeout o Git ausente en listas vacías. Todos deben llegar como estados visibles y tipados.
- **Concurrencia:** invalidaciones y respuestas deben incluir la ruta/workspace que las originó para no pintar datos de un workspace anterior después de cambiar de proyecto.

## 15. Alternativas rechazadas

1. **GitHub como árbol principal:** descartada; rompe el caso local y oculta cambios no publicados.
2. **Snapshot completo continuo:** descartado por I/O, almacenamiento y ambigüedad con el vault de rewind.
3. **Indexador persistente desde v1:** descartado hasta medir que la búsqueda acotada no cumple.
4. **Una clase/trait universal para todas las fuentes:** descartado como abstracción prematura; las fuentes tienen errores y semánticas diferentes.
5. **Terminal simulada con `textarea` o reutilización de `comando`:** descartada; no proporciona stdin/resize/exit/PTY.
6. **Implementar todas las fases en un bloque:** descartado; impide atribuir regresiones y hace imposible verificar seguridad antes de añadir procesos.

## 16. Documentación, gate y Definition of Done por bloque

Documentos afectados:

- este plan: contratos, límites, decisiones y evidencia de fases;
- `roadmap.md`: solo el siguiente bloque ejecutable y su estado;
- `Agente/completados/tareas-YYYY-MM-DD.md`: evidencia al cerrar cada bloque;
- `Agente/documentacion/`: solo si se confirma un contrato duradero de filesystem, Git, watcher o terminal;
- `Agente/prevencion/`: solo para un riesgo reproducible que permanezca abierto.

Definition of Done de Fase 0 + Fase 1:

- DTOs y errores definidos sin confundir tool/Git/filesystem;
- tests de contención, límites y workspace no configurado;
- listado y lectura funcionan en una ventana Tauri con un workspace real;
- `panelVisor` puede consumir una ruta relativa sin depender de una ruta absoluta expuesta;
- no se añadió Git, watcher, snapshot ni PTY por especulación;
- `npm run type-check` y `npm run build` de `desktop/ui` pasan;
- Rust se valida con el comando oficial del proyecto y `CARGO_TARGET_DIR` en `C:\tmp`;
- se ejecuta el gate real solo tras comprobar `doctor`, capacidades, política y manifest; no se afirma PASS por `tsc`/Vite ni por una salida parcial del gate;
- `git diff --check`, diff revisado y estado final documentado.

Para Git/watcher/PTY, cada fase tendrá su propio Definition of Done y no se marcará como completa por tener DTOs o botones sin prueba funcional.

## 17. Riesgos abiertos que bloquean una decisión confiada

- ¿El gate fijado dispone de un adaptador que cubra el desktop y el frontend, o solo el conjunto Rust declarado hoy?
- ¿Qué crate/licencia y API Windows se aceptarán para watcher y ConPTY, si las dependencias actuales no bastan?
- ¿El producto quiere una consulta Git por working tree o una referencia configurable (HEAD, índice, snapshot)? Sin esta decisión, “diff” es ambiguo.
- ¿El cambio externo debe recargar automáticamente o solo marcar obsolescencia? La opción segura inicial es marcar y ofrecer recarga.
- ¿Se necesita búsqueda de contenido en v1? Si no existe un caso concreto, se mantiene búsqueda de nombres para evitar indexación y costes innecesarios.
- ¿Qué política de exclusión se mostrará al usuario? Ocultar `.git`, `target` o `node_modules` sin indicar el motivo produciría una falsa vista completa.

## 18. Siguiente acción verificable

1. Crear únicamente los tipos/fixtures de Fase 0 y una prueba de contención reutilizable basada en la lógica de `archivo.rs`.
2. Confirmar los límites de 256 KiB, 500 entradas, 1.000 resultados y timeouts con fixtures; ajustar si la medición los contradice.
3. Implementar listado/lectura como comandos Tauri concretos y registrar cada comando en `main.rs`; no tocar aún Git, watcher, snapshots ni terminal.
4. Conectar el árbol al panel existente, reutilizando `panelDerecho.ts`/`panelVisor.ts`, y probar en Tauri real.
5. Solo después de esa evidencia, decidir si se abre la fase Git.

La primera implementación sigue siendo **Fase 0 + Fase 1**, sin terminal y sin snapshots. El criterio de progreso es demostrable: una ruta local real permite saber qué archivo existe, qué contenido tiene, qué cambio declaró el agente y qué límite/error se aplicó. 
