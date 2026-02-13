set -e

cd apps/tauri/src-tauri

if [ "${XDG_SESSION_TYPE:-}" = "wayland" ]; then
  export GDK_BACKEND=wayland
  export WINIT_UNIX_BACKEND=wayland
else
  export GDK_BACKEND=x11
  export WINIT_UNIX_BACKEND=x11
fi

export OPENWHISPERAI_UI_SERVER=0
export GDK_GL=disable
export WEBKIT_DISABLE_COMPOSITING_MODE=1

cargo run
