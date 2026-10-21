# GPU Acceleration Guide

Overtalk can use a GPU for Whisper transcription (through `whisper-rs`) and for the local notes helper (`llama-helper`, through `llama-cpp-2`). The default Parakeet speech model uses ONNX Runtime on the CPU. The optional Python speech worker selects its own device; see [speech worker setup](../speech-worker/README.md). The Linux packages are CPU-only, so GPU acceleration on Linux needs a source build.

## Supported Backends

Acceleration is chosen with Cargo features. The `sonefold` service crate (`frontend/src-tauri/Cargo.toml`) offers these for Whisper:

*   **CUDA** (`cuda`): NVIDIA GPUs on Linux and Windows.
*   **ROCm** (`hipblas`): AMD GPUs on Linux.
*   **Vulkan** (`vulkan`): AMD and Intel GPUs on Linux and Windows.
*   **Metal** and **Core ML** (`metal`, `coreml`): always enabled for Whisper on macOS.
*   **OpenBLAS** and **OpenMP** (`openblas`, `openmp`): CPU optimizations.

`llama-helper` offers `cuda`, `vulkan` and `metal`. With any other choice it is built for the CPU.

## Choosing a backend

Do not edit the default features in `Cargo.toml`. Pass the feature when building.

For the native app (the default interface), build the service and the helper with the same feature, using the environment from [Build and run from source](BUILDING.md#build-and-run-from-source-linux):

```sh
cargo build --release --locked -p sonefold --bin sonefold-service --features cuda
cargo build --release --locked -p llama-helper --features cuda
```

For CUDA on Linux, also set `CMAKE_POSITION_INDEPENDENT_CODE=ON`, and set `CMAKE_CUDA_ARCHITECTURES` when building without the GPU present ([GPU builds](BUILDING.md#gpu-builds)).

For the web interface, `frontend/dev-gpu.sh` and `frontend/build-gpu.sh` detect the hardware (see below). Set `TAURI_GPU_FEATURE` to override the detection, or to `none` for a CPU build (an empty value still runs detection):

```sh
TAURI_GPU_FEATURE=cuda frontend/build-gpu.sh
```

`pnpm run tauri:build:cuda` and the other `tauri:dev:*`/`tauri:build:*` scripts in `frontend/package.json` select a feature directly, but they do not rebuild `llama-helper`.

## Automatic GPU Detection

`frontend/dev-gpu.sh` and `frontend/build-gpu.sh` run `frontend/scripts/run-gpu.sh`, which calls `frontend/scripts/auto-detect-gpu.js` unless `TAURI_GPU_FEATURE` is set. Detection order:

1.  **macOS:** Apple Silicon uses `coreml` (with Metal); Intel Macs use `metal`.
2.  **CUDA (NVIDIA):** `nvidia-smi` is present and either `CUDA_PATH` is set or `nvcc` is found. If `nvidia-smi` is present without the CUDA toolkit, the build uses the CPU.
3.  **ROCm (AMD, Linux):** `rocm-smi` is present and either `ROCM_PATH` is set or `hipcc` is found.
4.  **Vulkan:** `vulkaninfo` is found (or `C:\VulkanSDK` exists on Windows), and both `VULKAN_SDK` and `BLAS_INCLUDE_DIRS` are set.
5.  **OpenBLAS (CPU):** `BLAS_INCLUDE_DIRS` is set.

Otherwise the build uses the CPU. `run-gpu.sh` then builds `llama-helper` with the matching feature (`coreml` maps to `metal`) and, for CUDA on Linux, defaults `CMAKE_CUDA_ARCHITECTURES` to `native`.

## Sharing the GPU with the desktop

`OVERTALK_GPU_LAUNCHER` names an optional launcher, such as a GPU-sharing wrapper, that GPU workers run under. Set it to the absolute path of an executable in the environment that starts Overtalk. The launcher receives the worker command as its arguments. If the variable is unset, or is not an absolute path to a file, workers start directly.

The service uses it for the Python speech worker (unless its device is `cpu`) and the local notes helper on Linux, and for GPU model workers. On Linux the launcher runs inside the worker sandbox, with the same restrictions as the worker. The service sets `TRAIN_CPU_NICE=5` for launchers that read it.

`scripts/run-installed-linux.sh` runs the web interface under `OVERTALK_GPU_LAUNCHER` when it is set. `python3 scripts/install-native-linux.py --gpu-launcher PATH` writes a launcher that runs the native app under `PATH` and passes it on to the workers.

## Platform-Specific Instructions

### Linux

See [GPU builds on Linux](building_in_linux.md) for toolkit setup and the web interface scripts, and [Build Overtalk](BUILDING.md#gpu-builds) for CUDA build settings.

### macOS

Whisper always builds with Metal and Core ML on macOS. Build `llama-helper` with `--features metal` for GPU notes generation; `run-gpu.sh` does this automatically.

### Windows

Install the toolkit for your GPU (for example the CUDA Toolkit for NVIDIA GPUs), then build with the corresponding feature, as shown above.
