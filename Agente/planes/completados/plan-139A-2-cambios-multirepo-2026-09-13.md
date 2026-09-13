# Plan 139A-2 — Cambios multi-repo estilo VSCode

Objetivo: el panel "Cambios" descubre todos los repos bajo la carpeta de la
conversación, agrupa por repositorio con su rama (anidados colapsados al
padre), muestra los cambios del vault de ESTA conversación que caen fuera de
todo repo en `Sin repositorio`, y se refresca al fin de turno / abrir
conversación / abrir panel / manual. Sin duplicar: lo que está en un repo con
git se revisa solo en su sección git (regla 139A-1, ahora por repo).

## Alcance / no alcance

- Sí: descubrimiento descendente configurable, `rama` por repo, secciones por
  repo + huérfanos, refresco en los 4 puntos, revalidación antes de
  aceptar/rechazar, ignorados por defecto, smoke CDP multi-repo.
- No: commit/stage/push desde el panel (solo lectura + aceptar/rechazar del
  vault); watcher continuo del FS (fase posterior si hace falta); repos fuera
  de la carpeta de la conversación (no se sube, decisión del usuario).

## Dependencias

- 139A-1 (dedupe vault↔git por ruta, `fijar`/`seleccionar`) — base directa.
- 129A-7 (aceptar=revisado, rechazar=vault+purga), 129A-8 (`revelar`).

## Fases verificables

### F1 — Backend: `git.repos` + `rama` (Rust `desktop/src-tauri/src/proyecto/git.rs`)

- Nuevo comando `git_repos(raiz: PathBuf, profundidad: u8) -> Vec<RepoGit>`:
  barrido descendente desde la carpeta de la conversación (nunca hacia
  arriba), `profundidad` default 2 configurable (`deteccion.profundidad`);
  detecta dir con `.git` (fichero o carpeta); un repo dentro de otro se
  colapsa al más externo (`padre: Option<String>` informativo, no sección).
- Ignorados por defecto (no se entra): `node_modules`, `target`, `dist`,
  `build`, `.git`, `tmp`, `Temp`, `__pycache__`, `.venv`, `*.lock` dirs,
  `C:\tmp`-like; configurables (`deteccion.ignorar: string[]`).
- `EstadoGit` gana `rama: Option<String>` (`git branch --show-current`;
  detached → `HEAD@<7>`; sin commits → `sin commits`). Sin rama no se bloquea.
- Tests Rust: fixture temporal con 2 repos + anidado + `node_modules/.git`
  (ignorado) → repos esperados, ramas correctas.
- Verificación: `cargo test -p glory-harness-desktop git` (vía wrapper con
  `CARGO_TARGET_DIR=C:\tmp`, gate aparte por disco).

### F2 — Frente: secciones por repo + huérfanos (sin duplicar)

- `GitTransport` gana `repos(): Promise<RepoGit[]>`; `estado(rutaRepo?)`:
  el adaptador real lo cablea a los comandos Tauri; el mock de tests devuelve
  fixture multi-repo.
- `panelCambios.cargar()`: pide `repos()` una vez; por cada repo pide su
  estado y monta una sección `git-seccion` con cabecera `nombre · rama`
  reutilizando `panelGit.fijar()`; los vault de la conversación se clasifican
  por prefijo de repo más largo: si la ruta cae en un repo y ya está en sus
  entradas git → se omite (139A-1 por repo); si cae en repo pero git no la
  lista (p.ej. ignorada) → fila vault dentro de esa sección; si no cae en
  ningún repo → sección `Sin repositorio` (clases git, como 139A-1 sin-git).
- `revelar(ruta)`: busca en secciones git (`seleccionar`) y luego en vault.
- Invariante: el vault solo lista cambios de ESTA conversación (`convId`);
  el backend ya filtra por conversación — se añade assert en smoke.
- Verificación: `tsc` + smoke CDP con fixture (repoA `main` + repoB
  `feature/x` + `repoA/tools/sub` colapsado + huérfano): 4 secciones
  esperadas, rama en cabecera, dedupe por repo, revelar en cada modo.

### F3 — Refresco: fin de turno, abrir conversación/panel, manual

- `panelCambios.recargar()` (ya existe) se invoca desde: fin de turno
  (`aplicarEventos` tras `turno-fin`), apertura de conversación, selección de
  la tab Cambios, y botón recargar en la cabecera del panel.
- Guardia anti-doble: `secuencia` a nivel panel (como `panelGit`) — una sola
  recarga en vuelo; las tardías se descartan.
- Revalidación anti-obsoleto: aceptar/rechazar re-pide el estado de SU repo;
  si la ruta ya no está sucia → toast "ya no hay cambios" y recarga.
- Verificación: smoke provoca cambio tras pintar y acepta → toast + fila
  fuera; recarga manual re-pinta.

### F4 — Cierre (como quedó)

- `panelCambios.ts` superó 300 líneas efectivas (warning `limite-lineas`):
  split en `vaultCambios.ts` (vista vault: pintar/selección/aceptar/rechazar/
  revisados, `crearVaultCambios`) + `util/reposUtil.ts` (puro testeable:
  `normalizarRutaArea`, `dentroDePrefijo`, `relativaEnRepo`). Host 259 líneas,
  vault 255.
- Carrera en tests Rust (`arbol_repos()` compartía `gh-repos-{pid}` en hilos
  paralelos → fallo intermitente `descubre_repos_colapsa...`): sufijo por test.
- Sin captura CDP: sin backend Tauri en navegador no hay repos que pintar
  (limitación registrada); evidencia alternativa: smoke node 14/14 de la
  lógica de atribución (`C:\tmp\gh-repos-smoke.cjs` vía esbuild) + gate.
- Evidencia: `tsc` EXIT 0; analyze 0 errores/0 warnings/7 info (patrón ISP
  preexistente); `sentinel check 139A-2` **PASS** (coverage/sccache/sentinel/
  rust, 473+1 tests ok); roadmap sin 139A-2; plan archivado; SIN commit/push
  (decisión del usuario para esta tarea).

## Definition of Done

`tsc` limpio + analyze 0 errores/0 warnings + smoke 14/14 + gate PASS +
completadas con evidencia. Cerrada 2026-09-13 SIN commit/push (decisión del
usuario); el árbol queda con los cambios sin commitear.

## Riesgos

- Barrido lento en áreas grandes → tope `profundidad` + ignorados + límite
  de repos (p.ej. 20, aviso si se supera).
- Rutas Windows/prefijos (`C:\` vs `c:\`, `/` vs `\`) en el match vault↔repo
  → normalizar a minúsculas + separador `/` antes de comparar.
- Disco <8 GB bloquea gate Rust → F1 con tests vía wrapper y gate diferido
  con evidencia registrada (como 129A-8fix).
