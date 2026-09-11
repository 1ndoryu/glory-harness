# Disco lleno: el build falla con un error que parece del compilador (11-09)

Riesgo **reproducible** que costó una sesión entera de diagnóstico el 11-09 al intentar
compilar el shell de escritorio. No es deuda de código de `glory-harness`: la capa
responsable es el **volumen `C:` del equipo**, no el repositorio. Se registra con caso
mínimo, detección barata y fix propuesto, y se referencia desde `roadmap.md`
(«Siguiente bloque ejecutable», donde quedó como prerrequisito del Bloque B).

## 1. `no space on device` disfrazado de defecto de código

- **Caso mínimo (reproducible):** `cargo build -p glory-harness-desktop` con
  `CARGO_TARGET_DIR = C:\tmp\glory-target\glory-harness` y el volumen `C:` a **0 GB
  libres**. La salida mezcla tres errores y **solo uno** nombra el disco:

  ```text
  warning: build failed, waiting for other jobs to finish...
  rustc-LLVM ERROR: IO failure on output stream: no space on device
  error: could not compile `glory-harness` (lib)
  Caused by:
    process didn't exit successfully: `sccache … rustc.exe --crate-name glory_harness …`
      (exit code: 0xc0000409, STATUS_STACK_BUFFER_OVERRUN)
  error: failed to write to `C:\tmp\glory-target\glory-harness\debug\deps\rmeta9ATZCU\full.rmeta`:
    Espacio en disco insuficiente. (os error 112)
  error: could not compile `webview2-com-sys` (lib) due to 1 previous error
  ```

- **Por qué engaña:** `rustc-LLVM ERROR` y `STATUS_STACK_BUFFER_OVERRUN` se leen como
  *crash del compilador* y `could not compile webview2-com-sys` como *crate roto*;
  además el crate señalado es una dependencia de terceros, no código propio. La única
  pista fiable es `os error 112` / «Espacio en disco insuficiente» en la línea del
  `full.rmeta`.
- **Capa responsable:** entorno del equipo (volumen `C:`). La purga del área
  (`GloryTmpSweep`, cada hora) acota **`C:\tmp`**, no el resto del volumen: con el disco
  al 100%, `C:\tmp` puede estar en 1,2 GB y el build seguir fallando.
- **Detección esperada (barata y previa a compilar):** `Get-PSDrive C` — si
  `Free/1GB` es menor que **8**, purgar antes de construir. Señal de confirmación en la
  salida de cargo: `os error 112`.
- **Presupuesto de disco a recordar (medido 11-09):** corte `core` + `cli` con pruebas
  ≈ **5 GB**; el shell de escritorio añade ≈ **2,3 GB** (árbol tauri/wry/webview2).
- **Medición del 11-09:** `C:` `Used 237.57 GB` / `Free 0.00 GB`;
  `C:\tmp\glory-target\glory-harness` = **7,51 GB** (pico de **8,98 GB** según
  `C:\tmp\mantenimiento\sweep.log` a las 22:50). Consumo del perfil ≈ 55 GB (`OneDrive`
  21,85 · `AppData\Local` 20,31 · `AppData\Roaming` 10,34 · `Downloads` 2,44 · `.cargo`
  1,76 · `.rustup` 1,34) + `ProgramData` 5,66. **El resto del volumen no se atribuyó**: es
  diagnóstico del equipo, fuera del alcance de este repo.
- **Mitigación aplicada (11-09):** purga manual de `C:\tmp\glory-target\glory-harness` y
  `C:\tmp\sentinel-probe` → 6,32 GB libres; `GloryTmpSweep` completó la purga por techo a
  las 23:50 (`techo: purga glory-harness (7.34 GB)` → `total C:\tmp 1.17 GB`). El build
  del shell **no** se reintentó: el espacio seguía por debajo del presupuesto de 8 GB.
- **Fix propuesto:** (a) *preflight* de espacio en la etapa `rust` del gate
  (`scripts/quality/sentinel-rust.mjs`) que aborte con un mensaje explícito («espacio
  insuficiente en el volumen del target: X GB libres, se necesitan ~Y») en vez de dejar
  que cargo escriba un target a medias; (b) en
  `scripts/mantenimiento/limpiar-tmp.ps1`, además del techo de `C:\tmp`, registrar un
  aviso cuando el **libre del volumen** baje de un umbral, que es la condición que
  realmente rompe los builds.
- **No hacer:** interpretar el error como defecto de código, «arreglar»
  `webview2-com-sys` ni tocar `Cargo.toml` por este síntoma.

## 2. Tarea programada en rojo (mismo volumen, otro dueño)

- **Caso:** `GloryCargoTargetCleanup` terminó en rojo el 10-09 23:55
  (`LastTaskResult = 1`). Ejecuta `wscript //B //Nologo
  "…\glory-rust-template\scripts\run-cargo-cleanup-hidden.vbs"
  "…\glory-rs\scripts\clean-cargo-target.ps1" "C:\tmp\glory-target" 15360`, es decir un
  techo de **15 GB** sobre el mismo `C:\tmp\glory-target` que este repo usa como target.
- **Capa responsable:** tooling de `glory-rust-template` (no este repositorio). Se
  registra porque compite por el mismo volumen y su techo (15 GB) es más laxo que el de
  `GloryTmpSweep` (6 GB): mientras falle, el área pierde un mecanismo de contención.
- **Detección esperada:** `Get-ScheduledTask -TaskName GloryCargoTargetCleanup |
  Get-ScheduledTaskInfo` → `LastTaskResult`.
- **Fix propuesto:** que su dueño revise el VBS/script (ruta, permisos y código de salida)
  y que el techo sea coherente con el de `GloryTmpSweep`.

## Estado y lecciones

- `GloryTmpSweep` **funciona**: `sweep.log` muestra purgas reales el 10-09 a las 20:49
  (5,22 GB), 22:50 (8,98 GB) y 23:50 (7,34 GB por techo). No hay que «arreglarla»: hay que
  recordar que su ámbito es `C:\tmp`, no el volumen.
- Lección de conducta: una compilación pesada empieza por `Get-PSDrive C`; el disco es un
  recurso acotado tanto como el tiempo o la memoria (regla del área).
- Lección documental: la nota «desktop compilation bloqueada por errores pre-existentes en
  `navegador.rs`» (069A-1, 06-09) se leyó durante días como un bloqueo de código cuando el
  shell ya compilaba (`tmp-rust-desktop.log`, 10-09 02:14) y el bloqueo real del 11-09 era
  el disco. Una excepción sin fecha de revisión envejece como un dato falso.
