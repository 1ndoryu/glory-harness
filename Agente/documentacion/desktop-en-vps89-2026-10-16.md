# Desktop de glory-harness en la VPS 89 (16AA-1, 2026-10-16)

Cómo se corre la app de escritorio (Tauri) en la 89 para no cargar el laptop, y
qué hizo falta para que compilara. Complementa
`taller-remoto-vps89-2026-10-14.md` y `15AA-8` (escritorio LXQt de la 89).

## Qué se montó

| Pieza | Dónde |
|---|---|
| Réplica del proyecto | `~/taller/glory-harness` (Mutagen `taller-glory-harness`, one-way-replica laptop→89) |
| Target de Cargo | `/var/tmp/glory-target/glory-harness_linux` |
| sccache | `~/bin/sccache` (0.18.0, prebuilt musl) |
| Lanzador dev | `scripts/desktop-correr-linux.sh` (tmux `gh-desktop`, log `/var/tmp/gh-dev.log`) |
| Ventana | escritorio LXQt de `:1`; se entra con `workspace-manager/scripts/remoto/escritorio-89.sh` |

## Dependencias de sistema (root, una vez)

`dev` no tiene sudo; se instaló con `ssh root@89.117.16.170`:

```bash
apt-get install -y --no-install-recommends libwebkit2gtk-4.1-dev build-essential \
  curl wget file libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev \
  libgtk-3-dev libsoup-3.0-dev
# + imagemagick, x11-utils, wmctrl (para capturar/inspeccionar la ventana de :1)
```

Antes de esto `pkg-config --modversion webkit2gtk-4.1` no existía: había las `.so`
de runtime pero no los `.pc` ni las cabeceras, que es lo que exige el enlace de
Tauri v2 en Linux.

## Port a Linux del shell (`desktop/src-tauri`)

El backend Tauri era Windows-only en dos puntos, y ambos rompían el build:

1. `Cargo.toml`: `webview2-com` y `windows` iban como dependencias
   incondicionales. `webview2-com-0.38.2/src/lib.rs` usa
   `windows::Win32::UI::WindowsAndMessaging` sin `cfg`, así que **no compila
   fuera de Windows** (el crate `windows` sí: es `#![cfg(windows)]` y queda
   vacío). Se movieron a `[target.'cfg(windows)'.dependencies]`.
2. `src/archivos/filesystem.rs`: `use windows::…` y `SHOpenWithDialog` sin
   `cfg` (diálogo "Abrir con"). Se gateó con una rama `not(windows)` que usa
   `xdg-open`.

De paso, `src/proyecto/workspaces.rs` (`workspace_revelar`) usaba `explorer`
literal: se partió en `revelar_en_sistema` (`explorer` / `xdg-open`).

`src/navegador/` ya tenía ramas `not(windows)` funcionales (stubs que devuelven
`Err("… solo está implementada en Windows")`), así que el navegador interno
WebView2 es lo único que queda exclusivo de Windows.

## Trampas encontradas

- **`.cargo/config.toml` del proyecto es Windows-only**: fija
  `target-dir = "C:/tmp/glory-target/glory-harness"` y
  `rustc-wrapper = "sccache"`. En la 89 el `target-dir` lo pisa `CARGO_TARGET_DIR`
  (lo exporta el lanzador) y `sccache` hay que instalarlo, o cargo no arranca
  (`could not execute process 'sccache …'`).
- **WebKitGTK sin GPU**: en el VNC hay que forzar
  `LIBGL_ALWAYS_SOFTWARE=1`, `WEBKIT_DISABLE_COMPOSITING_MODE=1` y
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`; sin ellos la ventana no se pinta.
- **El aviso `Failed to connect to atspi registry`** es ruido de accesibilidad,
  no un fallo.
- **El mando no sirve para esta app**: `Arranque up` exige que cada
  `puertosRemotos` escuche y responda 200 en un `healths`. Una GUI no tiene
  puerto, así que no se dio de alta en `registro.json`; el sync se creó a mano
  con el nombre de convención (`taller-glory-harness`) y los mismos `ignorar` de
  `taller.json`.
- **`tauri dev` corre fuera de `cm-heavy`** (si no, retendría el lock del
  taller durante toda la sesión de desarrollo). El primer build tardó ~4m18s con
  4 vCPU.

## Ciclo de trabajo

1. Se edita en el laptop (fuente de verdad).
2. `mutagen sync flush taller-glory-harness`.
3. La UI recarga sola (vite en `:1420` con HMR); si se toca Rust, `tauri dev`
   recompila y reinicia la ventana.
4. Se mira la ventana por el escritorio VNC.
