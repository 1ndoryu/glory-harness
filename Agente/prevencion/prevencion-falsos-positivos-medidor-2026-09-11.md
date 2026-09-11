# Falsos positivos del medidor vistos desde glory-harness (11-09)

Riesgos **reproducibles y aún no corregidos** en las herramientas de medición que el
gate y la consola ejecutan sobre este repo. Ninguno es deuda de código de
`glory-harness`: la capa responsable está en otra herramienta, así que aquí solo se
registra el caso mínimo, quién debe arreglarlo y cómo comprobarlo. Referenciado desde
`roadmap.md` (`109A-10`).

## 1. `todo-pendiente` marca prosa en español dentro de un doc comentario

- **Caso mínimo (reproducible):** `core/src/nucleo/context.rs:972` —

  ```rust
  /// Ventana pequeña con cola mínima: hay tramo resumible (la cola no absorbe
  /// todo el historial) sin que la ocupación llegue al umbral automático.
  ```

  El gate lo reporta como `INFO … todo-pendiente: Marcador pendiente detectado`.
- **Por qué coincide:** la regla es
  `/(?:\/\/|\/\*|#|<!--)\s*(?:TODO|FIXME|HACK|PENDIENTE|XXX)\b/i`
  (`glory-sentinel/src/config/defaultRules.ts:208`). Sobre `/// todo el historial…` la
  coincidencia **no** empieza en el primer `//`: el motor arranca en el índice 1,
  donde `//` casa con los dos `/` siguientes, `\s*` consume el espacio y `todo` casa
  con `TODO` por el flag `/i` y el `\b`. Es decir, basta que un comentario de línea
  empiece por `// ` o `/// ` seguido de la palabra «todo» para que dispare.
- **Capa responsable:** `glory-sentinel` (`src/config/defaultRules.ts`, dueño de la
  regla). No se corrige desde este repo: es un artefacto compartido por los
  consumidores del área.
- **Alcance medido:** es el **único** hint que le queda al corte del gate canónico de
  este repo (11-09, tras 109A-12: 0 errores, 0 warnings, 1 info). El mismo falso
  positivo aparece en los hints de TASKS, RESTAURANTE y `coolify-manager-rs`.
- **Detección esperada:** el hallazgo trae severidad `hint`, regla `todo-pendiente` y
  apunta a una línea que **no** contiene un marcador declarado (`TODO:`, `FIXME`,
  `HACK`, `PENDIENTE:`); un marcador real se lee como tal en la propia línea.
- **Fix propuesto:** exigir delimitador de marcador (dos puntos, paréntesis, cierre
  del comentario o fin de línea) o mantener `TODO` sensible a mayúsculas y dejar
  `FIXME|HACK|PENDIENTE|XXX` como están. Con test de regresión en `src/test/suite/`
  que cubra prosa en español («todo el historial») **y** marcadores reales
  (`// TODO: …`, `# TODO`, `/* FIXME */`).
- **Propagación:** el arreglo necesita commit publicado en `glory-sentinel` y el bump
  único de consumidores; este repo fija hoy `1587c59` (`v0.7.8`) en
  `quality-tools.json` (`provisionPath: ../.quality-tools-harness/sentinel`), mientras
  la fuente va por `v0.7.9` (`a3f5607`), que arregla **otros** falsos positivos
  (HMAC, clave computada, `style={{}}` en comentario) pero **no** este.
- **Workaround descartado:** reformular el comentario para que no empiece por «todo» es
  legítimo como prosa, pero adoptarlo aquí bajaría el conteo **sin arreglar el
  medidor** y dejaría el defecto vivo para el resto del área; se descarta a propósito.

## 2. `claseHuerfana` (VarSense) no resuelve nombres de clase compuestos

- **Caso mínimo (i) composición por plantilla:** `gitDiff.ts` escribe
  `` `git-diff-linea git-diff-${tipo}` `` con `tipo ∈ {adicion, eliminacion, contexto}`;
  `.git-diff-adicion` y `.git-diff-eliminacion` sí se aplican y la regla no las
  resuelve.
- **Caso mínimo (ii) concatenación por ternario:** `'ic' + (pequeno ? ' ic-xs' : '')`.
- **Capa responsable:** VarSense (`orphan-classes` / `claseHuerfana`). El gate de este
  repo **no ejecuta** etapa VarSense, así que estas marcas no aparecen en su corte; se
  ven en la consola del área.
- **Detección esperada:** marcas en `desktop/ui/src/estilos` cuyo nombre de clase no
  aparece literal en el código: antes de retirar CSS hay que buscar la composición
  (plantilla o ternario) además de la cadena completa. Con ese cotejo, 109A-8 retiró
  las 7 que sí eran CSS muerto real y dejó ~302 marcas sin uso real.

## 3. El alcance de `orphan-classes` es el workspace, no el proyecto

- `orphan-classes` barre el árbol completo del área (**4516 archivos**, incluido
  `data/referencias-cli/**`), así que su total global no es comparable con el corte del
  gate de este repo (**224 archivos propios**). El número útil es el recuento por
  carpeta propia; comparar agregados produce conclusiones falsas sobre el mismo código.

## Ya corregidos upstream (historia, no deuda pendiente)

- `unwrap-produccion-rs` ×30 sobre dos módulos solo-test con `#![cfg(test)]` →
  corregido en `1587c59`.
- `axum-ruta-sintaxis-rs` ×19 sobre rutas axum 0.8 (el consejo de `:param` habría roto
  las rutas) → corregido en `08aaf25`, version-aware según `Cargo.lock`.

Ambos son los que la consola contaba con el checkout compartido `902c45e` mientras el
gate de este repo ya usaba `1587c59`: causa raíz registrada como `039A-4` en
`workspace-manager/roadmap.md` (el medidor debe resolver el `provisionPath` de cada
proyecto, no solo el checkout compartido).
