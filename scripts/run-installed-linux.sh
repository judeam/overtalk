#!/bin/sh
# Launch the unpacked local Linux installation; no display settings are changed.
set -eu
app_run="${SONEFOLD_APP_RUN:-$HOME/.local/opt/sonefold/app/AppRun}"
if [ ! -x "$app_run" ]; then
  printf '%s\n' "Overtalk AppRun missing: $app_run. Install an unpacked AppImage or set SONEFOLD_APP_RUN." >&2
  exit 1
fi
# Keep WebKit's fontconfig cache separate from newer caches written by browsers.
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}/sonefold/runtime"
mkdir -p "$XDG_CACHE_HOME"
# Workstation-specific NVIDIA/WebKit painting workaround. Inference still uses CUDA.
export WEBKIT_DISABLE_DMABUF_RENDERER="${WEBKIT_DISABLE_DMABUF_RENDERER:-1}"
export WEBKIT_DISABLE_COMPOSITING_MODE="${WEBKIT_DISABLE_COMPOSITING_MODE:-1}"
# Optional launcher for GPU sharing; it receives the application command as its arguments.
responsive="${OVERTALK_GPU_LAUNCHER:-}"
if [ -n "$responsive" ] && [ -x "$responsive" ]; then
  exec "$responsive" "$app_run" "$@"
fi
exec "$app_run" "$@"
