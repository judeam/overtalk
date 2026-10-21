#!/usr/bin/env bash
# Shared Unix entry point: build the matching helper, then launch Tauri.
set -euo pipefail

mode=${1:-}
case "$mode" in
    build) profile=release; profile_args=(--release) ;;
    dev) profile=debug; profile_args=() ;;
    *) echo 'Usage: run-gpu.sh build|dev' >&2; exit 2 ;;
esac
case "$OSTYPE" in
    darwin*|linux-gnu*) ;;
    *) echo "Unsupported OS: $OSTYPE" >&2; exit 1 ;;
esac

frontend_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
workspace_dir=$(dirname -- "$frontend_dir")
cd -- "$frontend_dir"
if command -v pnpm >/dev/null 2>&1; then
    package_manager=pnpm
elif command -v npm >/dev/null 2>&1; then
    package_manager=npm
else
    echo 'Install pnpm or npm first.' >&2
    exit 1
fi

export TAURI_GPU_FEATURE=${TAURI_GPU_FEATURE:-$(node scripts/auto-detect-gpu.js)}
helper_args=()
case "$TAURI_GPU_FEATURE" in
    coreml) helper_args=(--features metal) ;;
    metal|cuda|vulkan) helper_args=(--features "$TAURI_GPU_FEATURE") ;;
    ''|none|openblas|openmp|hipblas) ;; # The helper supports only Metal/CUDA/Vulkan.
    *) echo "Unsupported GPU feature: $TAURI_GPU_FEATURE" >&2; exit 2 ;;
esac
if [[ "$OSTYPE" == linux-gnu* && "$TAURI_GPU_FEATURE" == cuda ]]; then
    export CMAKE_CUDA_ARCHITECTURES=${CMAKE_CUDA_ARCHITECTURES:-native}
    export CMAKE_CUDA_STANDARD=17 CMAKE_POSITION_INDEPENDENT_CODE=ON
fi

cargo build --manifest-path "$workspace_dir/Cargo.toml" -p llama-helper \
    "${profile_args[@]}" "${helper_args[@]}"
target_triple=$(rustc -vV | awk '/^host:/ {print $2}')
test -n "$target_triple"
# Cargo resolves a relative CARGO_TARGET_DIR from this command's working directory.
target_dir=${CARGO_TARGET_DIR:-$workspace_dir/target}
destination="$frontend_dir/src-tauri/binaries/llama-helper-$target_triple"
mkdir -p -- "$(dirname -- "$destination")"
cp -- "$target_dir/$profile/llama-helper" "$destination"

if [[ "$mode" == build ]]; then export NO_STRIP=true; fi
exec "$package_manager" run "tauri:$mode"
