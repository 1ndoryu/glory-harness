#!/usr/bin/env bash
# Lanza el desktop en modo dev dentro de la sesión gráfica de la 89 (VNC :1).
#
# En la 89:   bash ~/taller/glory-harness/scripts/desktop-correr-linux.sh
# Desde el laptop: ssh dev89 'bash ~/taller/glory-harness/scripts/desktop-correr-linux.sh'
# Parar:      bash .../desktop-correr-linux.sh --parar
#
# Corre en tmux (`gh-desktop`) para que sobreviva al cierre del ssh. El log queda
# en /var/tmp/gh-dev.log. La app abre su ventana en el escritorio LXQt de :1, al
# que se entra desde el laptop con `workspace-manager/scripts/remoto/escritorio-89.sh`.
set -euo pipefail

RAIZ="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NODE_BIN=/home/dev/.local/share/fnm/node-versions/v24.21.0/installation/bin
SESION=gh-desktop
LOG=/var/tmp/gh-dev.log

if [[ "${1:-}" == "--parar" ]]; then
  tmux kill-session -t "$SESION" 2>/dev/null && echo "parado" || echo "no corría"
  exit 0
fi

if tmux has-session -t "$SESION" 2>/dev/null; then
  echo "ya corre en tmux '$SESION' (log: $LOG)"
  echo "parar: bash $0 --parar"
  exit 0
fi

# sccache: el .cargo/config.toml del proyecto lo pone de rustc-wrapper; en la 89
# se instaló en ~/bin. Sin él, cargo no arranca (el binario no existía).
export PATH="$HOME/bin:$HOME/.cargo/bin:$NODE_BIN:$PATH"
export DISPLAY="${DISPLAY:-:1}"
# El target-dir del config es una ruta de Windows; aquí manda el del taller.
export CARGO_TARGET_DIR=/var/tmp/glory-target/glory-harness_linux
# WebKitGTK en VNC no tiene GPU: render por software y sin compositing/dmabuf
# (con los defaults la ventana sale en blanco o muere al primer pintado).
export LIBGL_ALWAYS_SOFTWARE=1
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export WEBKIT_DISABLE_DMABUF_RENDERER=1
export SCCACHE_BASEDIRS="$RAIZ"
export SCCACHE_CACHE_SIZE=5G

# El entorno se pasa explícito en la línea de tmux: el servidor tmux no hereda
# las variables exportadas por este shell.
tmux new-session -d -s "$SESION" \
  "cd '$RAIZ/desktop' && PATH='$PATH' DISPLAY='$DISPLAY' CARGO_TARGET_DIR='$CARGO_TARGET_DIR' LIBGL_ALWAYS_SOFTWARE=1 WEBKIT_DISABLE_COMPOSITING_MODE=1 WEBKIT_DISABLE_DMABUF_RENDERER=1 SCCACHE_BASEDIRS='$RAIZ' SCCACHE_CACHE_SIZE=5G '$RAIZ/desktop/ui/node_modules/.bin/tauri' dev 2>&1 | tee '$LOG'"

echo "lanzado en tmux '$SESION'; log: $LOG"
echo "la ventana aparece en el escritorio VNC (:1)"
